use crate::pre::vad::VAD_FRAME_SAMPLES;
use crate::pre::vad::SAMPLE_RATE;
use crate::VadConfig;
use std::collections::VecDeque;

pub struct VadFSM {
    in_speech: bool,
    consecutive_speech: u32,
    consecutive_silence: u32,
    silence_written: u32,
    attack_buffer: VecDeque<Vec<i16>>,

    min_speech_frames: u32,
    min_silence_frames: u32,
    max_silence_frames: u32,
    attack_frames: u32,
}

impl VadFSM {
    pub fn new(config: &VadConfig) -> Self {
        let ms_to_frames =
            |ms: u32| -> u32 { ms * SAMPLE_RATE / (VAD_FRAME_SAMPLES as u32 * 1000) };

        let attack_frames = ms_to_frames(config.attack_ms);

        Self {
            in_speech: false,
            consecutive_speech: 0,
            consecutive_silence: 0,
            silence_written: 0,
            attack_buffer: VecDeque::with_capacity(attack_frames as usize),
            min_speech_frames: ms_to_frames(config.min_speech_ms),
            min_silence_frames: ms_to_frames(config.min_silence_ms),
            max_silence_frames: ms_to_frames(config.max_silence_ms),
            attack_frames,
        }
    }

    pub fn reset(&mut self) {
        self.in_speech = false;
        self.consecutive_speech = 0;
        self.consecutive_silence = 0;
        self.silence_written = 0;
        self.attack_buffer.clear();
    }

    pub fn process(&mut self, is_speech: bool, frame: &[i16]) -> Vec<i16> {
        if is_speech {
            self.on_speech_frame(frame)
        } else {
            self.on_silence_frame(frame)
        }
    }

    fn on_speech_frame(&mut self, frame: &[i16]) -> Vec<i16> {
        self.consecutive_speech += 1;
        self.consecutive_silence = 0;

        if self.in_speech {
            frame.to_vec()
        } else if self.consecutive_speech >= self.min_speech_frames {
            self.in_speech = true;
            self.silence_written = 0;

            let mut out = Vec::with_capacity((1 + self.attack_buffer.len()) * VAD_FRAME_SAMPLES);

            for buf in &self.attack_buffer {
                out.extend_from_slice(buf);
            }
            out.extend_from_slice(frame);

            self.attack_buffer.clear();

            out
        } else {
            if self.attack_buffer.len() == self.attack_frames as usize {
                self.attack_buffer.pop_front();
            }
            self.attack_buffer.push_back(frame.to_vec());

            vec![]
        }
    }

    fn on_silence_frame(&mut self, frame: &[i16]) -> Vec<i16> {
        self.consecutive_silence += 1;
        self.consecutive_speech = 0;

        if self.in_speech {
            if self.consecutive_silence >= self.min_silence_frames {
                self.in_speech = false;
                self.silence_written = 0;
                self.attack_buffer.clear();
            }
            frame.to_vec()
        } else {
            if self.attack_buffer.len() == self.attack_frames as usize {
                self.attack_buffer.pop_front();
            }
            self.attack_buffer.push_back(frame.to_vec());

            if self.silence_written < self.max_silence_frames {
                self.silence_written += 1;
                vec![0i16; frame.len()]
            } else {
                vec![]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_of(fill: i16) -> [i16; VAD_FRAME_SAMPLES] {
        [fill; VAD_FRAME_SAMPLES]
    }

    fn is_filled_with(frame: &[i16], expected: i16) -> bool {
        (frame.len() == VAD_FRAME_SAMPLES) && 
            frame.iter().all(|&s| s == expected)
    }

    #[test]
    fn state_transitions() {
        const FRAME_MS: u32 = (VAD_FRAME_SAMPLES as u32) * 1000 / SAMPLE_RATE;

        let mut fsm = VadFSM::new(&VadConfig {
            max_silence_ms: 10 * FRAME_MS,
            min_silence_ms: 3 * FRAME_MS,
            min_speech_ms: 4 * FRAME_MS,
            attack_ms: 2 * FRAME_MS,
            threshold: 0.0,
        });

        // 1. Silence + truncation
        for i in 0..20 {
            let out = fsm.process(false, &frame_of(42));
            if i < 10 {
                assert!(is_filled_with(&out, 0), "frame {i} should be silenced");
            } else {
                assert!(out.is_empty(), "frame {i} should be truncated");
            }
        }

        // 2. Short speech burst rejected
        let out = fsm.process(true, &frame_of(1));
        assert!(out.is_empty());

        let out = fsm.process(true, &frame_of(2));
        assert!(out.is_empty());

        let out = fsm.process(false, &frame_of(0));
        assert!(out.is_empty()); // still truncated

        // 3. Speech confirmed, attack buffer flushed, one frame discarded
        let out = fsm.process(true, &frame_of(3));
        assert!(out.is_empty());

        let out = fsm.process(true, &frame_of(4));
        assert!(out.is_empty());

        let out = fsm.process(true, &frame_of(5));
        assert!(out.is_empty());

        let out = fsm.process(true, &frame_of(6));
        assert_eq!(out, vec![frame_of(4), frame_of(5), frame_of(6)].concat());

        // 4. Active speech passes through
        let out = fsm.process(true, &frame_of(7));
        assert!(is_filled_with(&out, 7));

        // 5. Short silence passes as-is
        let out = fsm.process(false, &frame_of(8));
        assert!(is_filled_with(&out, 8));

        let out = fsm.process(false, &frame_of(9));
        assert!(is_filled_with(&out, 9));

        let out = fsm.process(false, &frame_of(10));
        assert!(is_filled_with(&out, 10));

        // 6. Speech ends after min_silence, consecutive frames are silenced
        let out = fsm.process(false, &frame_of(11));
        assert!(is_filled_with(&out, 0));
    }
}
