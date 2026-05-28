use std::f32::consts::PI;

use crate::config::AgcConfig;
use crate::SAMPLE_RATE;

const MIN_RMS: f32 = 1.0e-6;
const MAX_ATTENUATION_DB: f32 = -40.0;

pub struct Agc {
    config: AgcConfig,
    high_pass_alpha: f32,

    level_db: f32,
    gain_db: f32,
    hp_prev_input: f32,
    hp_prev_output: f32,
}

impl Agc {
    pub fn new(config: &AgcConfig) -> Self {
        let mut config = config.clone();
        config.max_gain_db = config.max_gain_db.max(0.0);
        config.attack_ms = config.attack_ms.max(1.0);
        config.release_ms = config.release_ms.max(1.0);
        config.rms_window_ms = config.rms_window_ms.max(1.0);
        config.high_pass_hz = config.high_pass_hz.max(1.0);

        let dt = 1.0 / SAMPLE_RATE as f32;
        let rc = 1.0 / (2.0 * PI * config.high_pass_hz);

        Self {
            level_db: config.target_db,
            config,
            high_pass_alpha: rc / (rc + dt),
            gain_db: 0.0,
            hp_prev_input: 0.0,
            hp_prev_output: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.level_db = self.config.target_db;
        self.gain_db = 0.0;
        self.hp_prev_input = 0.0;
        self.hp_prev_output = 0.0;
    }

    pub fn process(&mut self, audio: &mut [i16]) {
        if audio.is_empty() {
            return;
        }

        let dt_ms = audio.len() as f32 * 1000.0 / SAMPLE_RATE as f32;
        let mut samples = Vec::with_capacity(audio.len());
        let mut sum_sq = 0.0_f32;

        for &sample in audio.iter() {
            let x = sample as f32 / 32768.0;
            let filtered = self.high_pass(x);
            sum_sq += filtered * filtered;
            samples.push(filtered);
        }

        let frame_db = rms_to_db(sum_sq, audio.len());

        self.level_db = smooth(self.level_db, frame_db, self.config.rms_window_ms, dt_ms);

        let target_gain_db = (self.config.target_db - self.level_db)
            .clamp(MAX_ATTENUATION_DB, self.config.max_gain_db);
        let prev_gain_db = self.gain_db;
        self.gain_db = smooth_gain(
            self.gain_db,
            target_gain_db,
            self.config.attack_ms,
            self.config.release_ms,
            dt_ms,
        );

        let start_gain = db_to_gain(prev_gain_db);
        let end_gain = db_to_gain(self.gain_db);
        let denom = (audio.len().saturating_sub(1)).max(1) as f32;

        for (i, sample) in samples.into_iter().enumerate() {
            let t = i as f32 / denom;
            let gain = start_gain + (end_gain - start_gain) * t;
            audio[i] = float_to_i16(soft_limit(sample * gain));
        }
    }

    fn high_pass(&mut self, x: f32) -> f32 {
        let y = self.high_pass_alpha * (self.hp_prev_output + x - self.hp_prev_input);
        self.hp_prev_input = x;
        self.hp_prev_output = y;
        y
    }
}

fn rms_to_db(sum_sq: f32, len: usize) -> f32 {
    let rms = (sum_sq / len as f32).sqrt().max(MIN_RMS);
    20.0 * rms.log10()
}

fn smooth(current: f32, target: f32, tau_ms: f32, dt_ms: f32) -> f32 {
    let a = (-dt_ms / tau_ms).exp();
    a * current + (1.0 - a) * target
}

fn smooth_gain(current: f32, target: f32, attack_ms: f32, release_ms: f32, dt_ms: f32) -> f32 {
    // Reduce gain quickly when audio gets louder; recover/boost more slowly.
    let tau = if target < current {
        attack_ms
    } else {
        release_ms
    };
    smooth(current, target, tau, dt_ms)
}

fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

fn float_to_i16(x: f32) -> i16 {
    (x * 32768.0).round().clamp(-32768.0, 32767.0) as i16
}

fn soft_limit(x: f32) -> f32 {
    const LIMIT: f32 = 0.98;

    let abs = x.abs();
    if abs <= LIMIT {
        return x;
    }

    let excess = abs - LIMIT;
    let compressed = LIMIT + excess / (1.0 + excess * 8.0);
    compressed.min(1.0).copysign(x)
}
