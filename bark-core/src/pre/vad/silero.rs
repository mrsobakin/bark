use super::VadError;

pub const VAD_FRAME_SAMPLES: usize = 512;
const SILERO_VAD_MODEL: &[u8] = include_bytes!("./silero_vad.onnx");

pub struct SileroVad {
    session: ort::session::Session,
    h: Vec<f32>,
    c: Vec<f32>,
}

impl SileroVad {
    pub fn load() -> Result<Self, VadError> {
        let session = ort::session::Session::builder()
            .map_err(|e| VadError(e.to_string()))?
            .commit_from_memory(SILERO_VAD_MODEL)
            .map_err(|e| VadError(e.to_string()))?;

        Ok(Self {
            session,
            h: vec![0.0; 128],
            c: vec![0.0; 128],
        })
    }
}

impl From<ort::Error> for VadError {
    fn from(value: ort::Error) -> Self {
        Self(value.to_string())
    }
}

impl SileroVad {
    pub fn is_speech(&mut self, frame: &[i16], threshold: f32) -> bool {
        self.is_speech_or_err(frame, threshold).unwrap_or(true)
    }

    fn is_speech_or_err(&mut self, frame: &[i16], threshold: f32) -> Result<bool, VadError> {
        use ort::value::Tensor;

        let mut f32_frame = vec![0.0f32; VAD_FRAME_SAMPLES];
        for (i, &s) in frame.iter().enumerate() {
            f32_frame[i] = s as f32 / 32768.0;
        }

        let input_val = Tensor::from_array((
            vec![1i64, VAD_FRAME_SAMPLES as i64],
            f32_frame.into_boxed_slice(),
        ))?;

        let sr_val = Tensor::from_array((vec![1i64], vec![16000i64].into_boxed_slice()))?;
        let h_val = Tensor::from_array((vec![2i64, 1i64, 64i64], self.h.clone().into_boxed_slice()))?;
        let c_val = Tensor::from_array((vec![2i64, 1i64, 64i64], self.c.clone().into_boxed_slice()))?;

        let result = self
            .session
            .run(ort::inputs![input_val, sr_val, h_val, c_val]);

        Ok(match result {
            Ok(outputs) => {
                let prob = outputs[0]
                    .try_extract_tensor::<f32>()
                    .ok()
                    .and_then(|(_, data)| data.first().copied())
                    .unwrap_or(0.0);

                if outputs.len() > 1 {
                    if let Ok((_, hn)) = outputs[1].try_extract_tensor::<f32>() {
                        self.h = hn.to_vec();
                    }
                }
                if outputs.len() > 2 {
                    if let Ok((_, cn)) = outputs[2].try_extract_tensor::<f32>() {
                        self.c = cn.to_vec();
                    }
                }

                prob > threshold
            }
            Err(_) => false,
        })
    }

    pub fn reset(&mut self) {
        self.h = vec![0.0; 128];
        self.c = vec![0.0; 128];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke() {
        let mut vad = SileroVad::load().unwrap_or_else(|err| {
            panic!("vad fails to load: {err}")
        });

        let frame = [0i16; VAD_FRAME_SAMPLES];

        if let Err(err) = vad.is_speech_or_err(&frame, 0f32) {
            panic!("vad fails to process a frame: {err}")
        }
    }
}
