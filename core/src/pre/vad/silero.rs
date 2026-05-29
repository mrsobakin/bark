use super::VadError;
use std::io::Cursor;
use std::sync::LazyLock;

use tract_nnef::prelude::*;

pub const VAD_FRAME_SAMPLES: usize = 512;
const VAD_STATE_LEN: usize = 256;
const VAD_STATE_DIM: [usize; 3] = [2, 1, 128];
const VAD_CONTEXT_SAMPLES: usize = 64;
const SILERO_MODEL_BYTES: &[u8] = include_bytes!("./silero_vad.nnef.tgz");

type SileroModel = TypedSimplePlan<Graph<TypedFact, Box<dyn TypedOp>>>;

static SILERO_MODEL: LazyLock<Result<SileroModel, VadError>> = LazyLock::new(|| {
    let mut model_bytes = Cursor::new(SILERO_MODEL_BYTES);

    Ok(tract_nnef::nnef()
        .with_tract_core()
        .model_for_read(&mut model_bytes)?
        .into_decluttered()?
        .into_optimized()?
        .into_runnable()?)
});

impl From<TractError> for VadError {
    fn from(value: TractError) -> Self {
        Self(value.to_string())
    }
}

pub struct SileroVad {
    model: &'static SileroModel,
    state: Box<[f32]>,
    context: [f32; VAD_CONTEXT_SAMPLES],
}

impl SileroVad {
    pub fn load() -> Result<Self, VadError> {
        let model = (*SILERO_MODEL)
            .as_ref()
            .map_err(|e| VadError(e.0.to_owned()))?;

        Ok(Self {
            model,
            state: Box::new([0.0; VAD_STATE_LEN]),
            context: [0.0; VAD_CONTEXT_SAMPLES],
        })
    }

    pub fn is_speech(&mut self, frame: &[i16], threshold: f32) -> Result<bool, VadError> {
        self.predict(frame).map(|p| p > threshold)
    }

    pub fn predict(&mut self, frame: &[i16]) -> Result<f32, VadError> {
        // 1. Prepare input
        debug_assert_eq!(frame.len(), VAD_FRAME_SAMPLES);

        let mut input: Vec<f32> = Vec::with_capacity(VAD_CONTEXT_SAMPLES + VAD_FRAME_SAMPLES);
        input.extend(self.context);
        input.extend(frame.iter().map(|&s| s as f32 / 32768.0));

        let new_context = input.last_chunk::<VAD_CONTEXT_SAMPLES>().unwrap();

        self.context.clone_from_slice(new_context);

        // 2. Build tensors
        let input_val = Tensor::from_shape(&[1, input.len()], &input)?;
        let state_val = Tensor::from_shape(&VAD_STATE_DIM, &self.state)?;

        // 3. Run inference
        let outputs = self.model.run(tvec![input_val.into(), state_val.into()])?;

        let prob = outputs[0].as_slice::<f32>()?[0];
        let state = outputs[1].as_slice::<f32>()?;

        self.state.clone_from_slice(state);

        Ok(prob)
    }

    pub fn reset(&mut self) {
        self.state.fill(0.0);
        self.context.fill(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke() {
        let mut vad = SileroVad::load().unwrap_or_else(|err| panic!("vad fails to load: {err}"));

        let frame = [0i16; VAD_FRAME_SAMPLES];

        if let Err(err) = vad.is_speech(&frame, 0f32) {
            panic!("vad fails to process a frame: {err}")
        }
    }
}
