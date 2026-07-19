use rubato::{
    calculate_cutoff, ResampleResult, Resampler as _, ResamplerConstructionError, SincFixedIn,
    SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

use crate::{util::chunker::Chunker, SAMPLE_RATE};

const CHUNK_SIZE: usize = 128;
const SINC_LENGTH: usize = 256;

pub struct Resampler {
    inner: SincFixedIn<f32>,
    chunks: Chunker<f32, CHUNK_SIZE>,
    input_rate: usize,
    input_frames: usize,
    output_frames: usize,
    delay: usize,
}

impl Resampler {
    pub fn new(input_rate: u32) -> Result<Self, ResamplerConstructionError> {
        let window = WindowFunction::BlackmanHarris2;
        let inner = SincFixedIn::new(
            f64::from(SAMPLE_RATE) / f64::from(input_rate),
            1.0,
            SincInterpolationParameters {
                sinc_len: SINC_LENGTH,
                f_cutoff: calculate_cutoff(SINC_LENGTH, window),
                interpolation: SincInterpolationType::Quadratic,
                oversampling_factor: 256,
                window,
            },
            CHUNK_SIZE,
            1,
        )?;
        let delay = inner.output_delay();

        Ok(Self {
            inner,
            chunks: Chunker::new(),
            input_rate: input_rate as usize,
            input_frames: 0,
            output_frames: 0,
            delay,
        })
    }

    pub fn push(&mut self, input: &[f32]) -> ResampleResult<Vec<i16>> {
        self.input_frames += input.len();
        let mut output = Vec::new();
        let Self {
            inner,
            chunks,
            output_frames,
            delay,
            ..
        } = self;

        chunks.feed(input, |chunk| {
            process(inner, chunk, usize::MAX, output_frames, delay, &mut output)
        })?;
        Ok(output)
    }

    pub fn finish(&mut self) -> ResampleResult<Vec<i16>> {
        let scaled = self.input_frames.saturating_mul(SAMPLE_RATE as usize);
        let target = scaled.saturating_add(self.input_rate - 1) / self.input_rate;
        let mut output = Vec::new();
        let Self {
            inner,
            chunks,
            output_frames,
            delay,
            ..
        } = self;

        chunks.finish(|chunk| process(inner, chunk, target, output_frames, delay, &mut output))?;
        let silence = [0.0; CHUNK_SIZE];
        while *output_frames < target {
            process(inner, &silence, target, output_frames, delay, &mut output)?;
        }
        Ok(output)
    }
}

fn process(
    resampler: &mut SincFixedIn<f32>,
    input: &[f32; CHUNK_SIZE],
    limit: usize,
    output_frames: &mut usize,
    delay: &mut usize,
    output: &mut Vec<i16>,
) -> ResampleResult<()> {
    let channels = resampler.process(&[input], None)?;
    let channel = &channels[0];
    let skip = (*delay).min(channel.len());
    *delay -= skip;

    let take = (channel.len() - skip).min(limit.saturating_sub(*output_frames));
    output.extend(channel[skip..skip + take].iter().copied().map(f32_to_i16));
    *output_frames += take;
    Ok(())
}

fn f32_to_i16(sample: f32) -> i16 {
    (sample * 32768.0)
        .round()
        .clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_does_not_depend_on_input_chunks() {
        let input = (0..44_100)
            .map(|i| {
                let phase = 2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44_100.0;
                phase.sin()
            })
            .collect::<Vec<_>>();

        let output = resample(&input, 317);
        assert_eq!(output, resample(&input, 2000));
        assert_eq!(output.len(), SAMPLE_RATE as usize);
    }

    #[test]
    fn finish_flushes_a_partial_chunk() {
        let mut resampler = Resampler::new(48_000).unwrap();
        assert!(resampler.push(&[1.0; 100]).unwrap().is_empty());
        assert_eq!(resampler.finish().unwrap().len(), 34);
    }

    fn resample(input: &[f32], chunk_size: usize) -> Vec<i16> {
        let mut resampler = Resampler::new(44_100).unwrap();
        let mut output = input
            .chunks(chunk_size)
            .flat_map(|chunk| resampler.push(chunk).unwrap())
            .collect::<Vec<_>>();
        output.extend(resampler.finish().unwrap());
        output
    }
}
