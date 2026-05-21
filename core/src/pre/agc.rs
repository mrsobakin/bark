use crate::config::AgcConfig;

pub struct Agc {
    target_db: f32,
}

impl Agc {
    pub fn new(config: &AgcConfig) -> Self {
        Self {
            target_db: config.target_db,
        }
    }

    pub fn process(&self, audio: &mut [i16]) {
        if audio.len() < 400 {
            return;
        }

        let sum_sq: f32 = audio
            .iter()
            .map(|&s| {
                let f = s as f32 / 32768.0;
                f * f
            })
            .sum();
        let mean_sq = sum_sq / audio.len() as f32;
        if mean_sq <= 0.0 {
            return;
        }

        let rms_db = 10.0 * mean_sq.log10();
        let gain_db = self.target_db - rms_db;
        let gain = 10.0_f32.powf(gain_db / 20.0);

        for s in audio.iter_mut() {
            let scaled = (*s as f32 * gain).clamp(-32768.0, 32767.0);
            *s = scaled as i16;
        }
    }
}
