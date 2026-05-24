use super::{VadError, SAMPLE_RATE};

pub const VAD_FRAME_SAMPLES: usize = 512;
const VAD_STATE_LEN: usize = 2 * 1 * 128;
const VAD_STATE_DIM: [i64; 3] = [2, 1, 128];
const VAD_CONTEXT_SAMPLES: usize = 64;
const SILERO_VAD_MODEL: &[u8] = include_bytes!("./silero_vad.onnx");

impl From<ort::Error> for VadError {
    fn from(value: ort::Error) -> Self {
        Self(value.to_string())
    }
}

pub struct SileroVad {
    session: ort::session::Session,
    state: Box<[f32]>,
    context: [f32; VAD_CONTEXT_SAMPLES],
}

impl SileroVad {
    pub fn load() -> Result<Self, VadError> {
        let session = ort::session::Session::builder()
            .map_err(|e| VadError(e.to_string()))?
            .commit_from_memory(SILERO_VAD_MODEL)
            .map_err(|e| VadError(e.to_string()))?;

        Ok(Self {
            session,
            state: Box::new([0.0; VAD_STATE_LEN]),
            context: [0.0; VAD_CONTEXT_SAMPLES],
        })
    }

    pub fn is_speech(&mut self, frame: &[i16], threshold: f32) -> Result<bool, VadError> {
        self.predict(frame).map(|p| p > threshold)
    }

    pub fn predict(&mut self, frame: &[i16]) -> Result<f32, VadError> {
        use ort::value::Tensor;

        // 1. Prepare input
        debug_assert_eq!(frame.len(), VAD_FRAME_SAMPLES);

        let mut input: Vec<f32> = Vec::with_capacity(VAD_CONTEXT_SAMPLES + VAD_FRAME_SAMPLES);
        input.extend(self.context);
        input.extend(frame.iter().map(|&s| s as f32 / 32768.0));

        let new_context = input.last_chunk::<VAD_CONTEXT_SAMPLES>().unwrap();
        self.context.clone_from_slice(new_context);

        // 2. Build tensors
        let input_val = Tensor::from_array((
            [1i64, input.len() as i64],
            input,
        ))?;

        let state_val = Tensor::from_array((
            VAD_STATE_DIM,
            std::mem::replace(&mut self.state, Box::new([0.0; VAD_STATE_LEN])),
        ))?;

        let sr_val = Tensor::from_array((
            [1i64],
            vec![SAMPLE_RATE as i64],
        ))?;

        // 3. Run inference
        let outputs = self.session.run(ort::inputs![input_val, state_val, sr_val])?;

        let prob = *outputs.get("output")
            .expect("output field should exist in outputs")
            .try_extract_tensor::<f32>()?.1
            .first()
            .expect("output field is invalid");

        let state = outputs.get("stateN")
            .expect("stateN field should exist in outputs")
            .try_extract_tensor::<f32>()?.1
            .as_array::<{VAD_STATE_LEN}>()
            .expect("stateN field is invalid");

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
