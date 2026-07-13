use std::f32::consts::PI;

use crate::config::AgcConfig;
use crate::SAMPLE_RATE;

const MIN_POWER: f32 = 1.0e-12;
const NOISE_GATE_DB: f32 = -55.0;
const PEAK_CEILING: f32 = 0.891_250_9; // -1 dBFS
const MAX_ATTENUATION_DB: f32 = -40.0;

pub struct Agc {
    config: AgcConfig,
    high_pass_alpha: f32,
    short_power: f32,
    long_power: f32,
    gain_db: f32,
    active: bool,
    hp_prev_input: f32,
    hp_prev_output: f32,
}

impl Agc {
    pub fn new(config: &AgcConfig) -> Self {
        let mut config = config.clone();
        config.target_db = finite_or(config.target_db, -18.0).clamp(-40.0, -3.0);
        config.max_gain_db = finite_or(config.max_gain_db, 20.0).clamp(0.0, 40.0);
        config.attack_ms = finite_or(config.attack_ms, 30.0).max(1.0);
        config.release_ms = finite_or(config.release_ms, 250.0).max(1.0);
        config.rms_window_ms = finite_or(config.rms_window_ms, 80.0).max(1.0);
        config.long_window_ms = finite_or(config.long_window_ms, 1500.0).max(config.rms_window_ms);
        config.high_pass_hz =
            finite_or(config.high_pass_hz, 80.0).clamp(1.0, SAMPLE_RATE as f32 * 0.45);

        let dt = 1.0 / SAMPLE_RATE as f32;
        let rc = 1.0 / (2.0 * PI * config.high_pass_hz);
        let initial_power = db_to_power(config.target_db);

        Self {
            short_power: initial_power,
            long_power: initial_power,
            config,
            high_pass_alpha: rc / (rc + dt),
            gain_db: 0.0,
            active: false,
            hp_prev_input: 0.0,
            hp_prev_output: 0.0,
        }
    }

    pub fn reset(&mut self) {
        let initial_power = db_to_power(self.config.target_db);
        self.short_power = initial_power;
        self.long_power = initial_power;
        self.gain_db = 0.0;
        self.active = false;
        self.hp_prev_input = 0.0;
        self.hp_prev_output = 0.0;
    }

    pub fn process(&mut self, audio: &mut [i16]) {
        let dt_ms = 1000.0 / SAMPLE_RATE as f32;
        let short_alpha = smoothing_alpha(self.config.rms_window_ms, dt_ms);
        let long_alpha = smoothing_alpha(self.config.long_window_ms, dt_ms);

        for sample in audio {
            let x = *sample as f32 / 32768.0;
            let filtered = self.high_pass(x);
            let power = filtered * filtered;

            self.short_power = short_alpha * self.short_power + (1.0 - short_alpha) * power;
            let short_db = power_to_db(self.short_power);
            let is_active =
                power_to_db(power) >= NOISE_GATE_DB || (self.active && short_db >= NOISE_GATE_DB);

            let target_gain_db = if is_active {
                if !self.active {
                    self.long_power = self.short_power;
                }
                self.long_power = long_alpha * self.long_power + (1.0 - long_alpha) * power;
                let measured_db = short_db.max(power_to_db(self.long_power));
                (self.config.target_db - measured_db)
                    .clamp(MAX_ATTENUATION_DB, self.config.max_gain_db)
            } else {
                // Silence must never teach the AGC to amplify the next onset.
                self.gain_db.min(0.0)
            };

            self.gain_db = smooth_gain(
                self.gain_db,
                target_gain_db,
                self.config.attack_ms,
                self.config.release_ms,
                dt_ms,
            );
            self.active = is_active;

            let requested_gain = db_to_gain(self.gain_db);
            let peak_safe_gain = PEAK_CEILING / filtered.abs().max(1.0e-9);
            *sample = float_to_i16(filtered * requested_gain.min(peak_safe_gain));
        }
    }

    fn high_pass(&mut self, x: f32) -> f32 {
        let y = self.high_pass_alpha * (self.hp_prev_output + x - self.hp_prev_input);
        self.hp_prev_input = x;
        self.hp_prev_output = y;
        y
    }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

fn smoothing_alpha(tau_ms: f32, dt_ms: f32) -> f32 {
    (-dt_ms / tau_ms).exp()
}

fn smooth(current: f32, target: f32, tau_ms: f32, dt_ms: f32) -> f32 {
    let alpha = smoothing_alpha(tau_ms, dt_ms);
    alpha * current + (1.0 - alpha) * target
}

fn smooth_gain(current: f32, target: f32, attack_ms: f32, release_ms: f32, dt_ms: f32) -> f32 {
    let tau = if target < current {
        attack_ms
    } else {
        release_ms
    };
    smooth(current, target, tau, dt_ms)
}

fn power_to_db(power: f32) -> f32 {
    10.0 * power.max(MIN_POWER).log10()
}

fn db_to_power(db: f32) -> f32 {
    10.0_f32.powf(db / 10.0)
}

fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

fn float_to_i16(x: f32) -> i16 {
    (x * 32768.0).round().clamp(-32768.0, 32767.0) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(samples: usize, amplitude: f32) -> Vec<i16> {
        (0..samples)
            .map(|i| {
                let phase = 2.0 * PI * 220.0 * i as f32 / SAMPLE_RATE as f32;
                (phase.sin() * amplitude * i16::MAX as f32) as i16
            })
            .collect()
    }

    #[test]
    fn silence_does_not_raise_gain() {
        let mut agc = Agc::new(&AgcConfig::default());
        let mut silence = vec![0; SAMPLE_RATE as usize];
        agc.process(&mut silence);

        assert_eq!(agc.gain_db, 0.0);
        assert!(silence.iter().all(|sample| *sample == 0));
    }

    #[test]
    fn output_is_independent_of_input_partitions() {
        let input = sine(SAMPLE_RATE as usize, 0.08);
        let mut complete = input.clone();
        let mut partitioned = input;
        let mut whole_agc = Agc::new(&AgcConfig::default());
        let mut chunked_agc = Agc::new(&AgcConfig::default());

        whole_agc.process(&mut complete);
        for chunk in partitioned.chunks_mut(137) {
            chunked_agc.process(chunk);
        }

        assert_eq!(complete, partitioned);
    }

    #[test]
    fn loud_onset_after_silence_does_not_clip() {
        let mut agc = Agc::new(&AgcConfig::default());
        let mut silence = vec![0; SAMPLE_RATE as usize];
        agc.process(&mut silence);

        let mut onset = sine(SAMPLE_RATE as usize / 2, 0.95);
        agc.process(&mut onset);

        assert!(onset.iter().all(|sample| sample.abs() < i16::MAX));
    }
}
