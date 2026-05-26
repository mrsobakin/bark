#!/usr/bin/env python3
"""
Patch upstream Silero VAD to a fixed-16k model and export a tract NNEF bundle.

The script performs three steps:
  1. inline the top-level 16 kHz branch from the upstream If node,
  2. fix the public tensor shapes for the streaming Silero contract,
  3. invoke tract from PATH to emit a compact NNEF archive.

The upstream model is expected to be the fixed-point ifless export from Silero VAD.
"""

from __future__ import annotations

import copy
import hashlib
import shutil
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path

import onnx
from onnx import TensorProto, checker, helper, shape_inference


UPSTREAM_MODEL_URL = (
    "https://github.com/snakers4/silero-vad/raw/master/"
    "src/silero_vad/data/silero_vad_op18_ifless.onnx"
)
UPSTREAM_MODEL_SHA256 = "7671cd04b004e9076da0d4a7b1a5aec36adf161c39230c1cb94a4fd5db6bbd28"
NNEF_OUT = Path(__file__).resolve().parents[1] / "src/pre/vad/silero_vad.nnef.tgz"


def set_shape(value_info: onnx.ValueInfoProto, elem_type: int, dims: list[int | str]) -> None:
    tensor = value_info.type.tensor_type
    tensor.elem_type = elem_type
    del tensor.shape.dim[:]

    for dim in dims:
        entry = tensor.shape.dim.add()
        if isinstance(dim, str):
            entry.dim_param = dim
        else:
            entry.dim_value = int(dim)


def select_16k_branch(model: onnx.ModelProto) -> onnx.ModelProto:
    graph = model.graph
    if_nodes = [node for node in graph.node if node.op_type == "If"]
    if len(if_nodes) != 1:
        raise RuntimeError(f"expected exactly one top-level If, found {len(if_nodes)}")

    if_node = if_nodes[0]
    branches = {attr.name: attr.g for attr in if_node.attribute if attr.type == onnx.AttributeProto.GRAPH}
    branch = branches.get("then_branch")
    if branch is None:
        raise RuntimeError("top-level If does not contain then_branch")

    kept_inputs = [copy.deepcopy(inp) for inp in graph.input if inp.name in ("input", "state")]
    if {inp.name for inp in kept_inputs} != {"input", "state"}:
        raise RuntimeError(f"unexpected model inputs: {[inp.name for inp in graph.input]}")

    for inp in kept_inputs:
        if inp.name == "input":
            set_shape(inp, TensorProto.FLOAT, [1, 576])
        elif inp.name == "state":
            set_shape(inp, TensorProto.FLOAT, [2, 1, 128])

    output_vi = helper.make_tensor_value_info("output", TensorProto.FLOAT, [1, 1])
    state_vi = helper.make_tensor_value_info("stateN", TensorProto.FLOAT, [2, 1, 128])

    nodes = [copy.deepcopy(node) for node in branch.node]
    for node in nodes:
        for index, name in enumerate(node.output):
            if name == branch.output[0].name:
                node.output[index] = "output"
            elif name == branch.output[1].name:
                node.output[index] = "stateN"

    new_graph = helper.make_graph(
        nodes=nodes,
        name="silero_vad_16k_stateful_if_inlined",
        inputs=kept_inputs,
        outputs=[output_vi, state_vi],
        initializer=[copy.deepcopy(init) for init in graph.initializer],
    )
    new_model = helper.make_model(
        new_graph,
        producer_name="bark_silero_patch",
        opset_imports=[copy.deepcopy(opset) for opset in model.opset_import],
    )
    new_model.ir_version = model.ir_version

    checker.check_model(new_model)
    try:
        new_model = shape_inference.infer_shapes(new_model)
        checker.check_model(new_model)
    except Exception as exc:  # pragma: no cover - tract is tolerant here
        print(f"shape inference warning: {exc!r}", file=sys.stderr)

    return new_model


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def download_file(url: str, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = destination.with_suffix(destination.suffix + ".tmp")
    with urllib.request.urlopen(url) as response, tmp_path.open("wb") as handle:
        shutil.copyfileobj(response, handle)
    tmp_path.replace(destination)


def fetch(url: str, destination: Path, expected_sha256: str) -> Path:
    print(f"downloading {url} -> {destination}")
    download_file(url, destination)
    actual = sha256(destination)
    if actual != expected_sha256:
        raise SystemExit(
            f"sha256 mismatch for {destination.name}: expected {expected_sha256}, got {actual}"
        )
    print(f"verified {destination.name} {actual}")
    return destination


def main() -> int:
    NNEF_OUT.parent.mkdir(parents=True, exist_ok=True)
    temp_dir = None
    try:
        temp_dir = tempfile.TemporaryDirectory(prefix="bark-silero-vad-")
        src = fetch(
            UPSTREAM_MODEL_URL,
            Path(temp_dir.name) / "silero_vad_op18_ifless.onnx",
            UPSTREAM_MODEL_SHA256,
        )
        print(f"using upstream model {src}")
        model = onnx.load(src)
        patched = select_16k_branch(model)
        onnx_out = Path(temp_dir.name) / "silero_vad_16k_if_inlined_pruned_static.onnx"
        onnx.save(patched, onnx_out)
        print(f"wrote {onnx_out} ({onnx_out.stat().st_size} bytes)")
        print(f"sha256 {onnx_out.name} {sha256(onnx_out)}")

        tract = shutil.which("tract")
        if tract is None:
            raise FileNotFoundError("tract not found in PATH")
        subprocess.run(
            [tract, str(onnx_out), "dump", "--nnef", str(NNEF_OUT)],
            check=True,
        )
        print(f"wrote {NNEF_OUT} ({NNEF_OUT.stat().st_size} bytes)")
        print(f"sha256 {NNEF_OUT.name} {sha256(NNEF_OUT)}")
        return 0
    finally:
        if temp_dir is not None:
            temp_dir.cleanup()


if __name__ == "__main__":
    raise SystemExit(main())
