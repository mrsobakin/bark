use super::EncodeError;
use crate::util::chunker::Chunker;
use crate::SAMPLE_RATE;
use opus::{Application, Bitrate, Channels};
use std::io::Write;

const CHANNELS: u8 = 1;
const OPUS_FRAME_SIZE: usize = 320; // 20 ms at 16 kHz
const GRANULE_PER_FRAME: u64 = 960; // 48 kHz Opus granule units
const BITRATE_KBPS: i32 = 24;

pub struct OpusEncoder<W: Write> {
    inner: InternalOpusEncoder<W>,
    chunker: Chunker<i16, OPUS_FRAME_SIZE>,
}

impl<W: Write> OpusEncoder<W> {
    pub fn new(writer: W) -> Result<Self, EncodeError> {
        Ok(Self {
            inner: InternalOpusEncoder::new(BITRATE_KBPS, writer)?,
            chunker: Chunker::new(),
        })
    }

    pub fn feed(&mut self, pcm: &[i16]) -> Result<(), EncodeError> {
        self.chunker.feed(pcm, |f| self.inner.encode(f))
    }

    pub fn finish(mut self) -> Result<W, EncodeError> {
        self.chunker.finish(|f| self.inner.encode(f))?;
        self.inner.finish()
    }
}

// Get random-enough number
fn ogg_serial() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u32)
        .unwrap_or(413) // https://xkcd.com/221/
}

struct InternalOpusEncoder<W: Write> {
    opus: opus::Encoder,
    ogg: ogg::writing::PacketWriter<'static, W>,
    serial: u32,
    granule_pos: u64,
    pending: Option<Vec<u8>>,
}

impl<W: Write> InternalOpusEncoder<W> {
    fn new(bitrate_kbps: i32, writer: W) -> Result<Self, EncodeError> {
        let mut opus = opus::Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Audio)?;
        opus.set_bitrate(Bitrate::Bits(bitrate_kbps * 1000))?;

        let serial = ogg_serial();
        let ogg = ogg::writing::PacketWriter::new(writer);

        let mut this = Self {
            opus,
            ogg,
            serial,
            granule_pos: 0,
            pending: None,
        };

        this.write_headers()?;
        Ok(this)
    }

    fn write_headers(&mut self) -> Result<(), EncodeError> {
        use ogg::writing::PacketWriteEndInfo;

        let lookahead = self.opus.get_lookahead().unwrap_or(0) as u64;
        let preskip = ((lookahead * 48000) / (SAMPLE_RATE as u64)) as u16;

        // OpusHead
        let mut opus_head = Vec::with_capacity(19);
        opus_head.extend_from_slice(b"OpusHead");
        opus_head.push(1); // version
        opus_head.push(CHANNELS);
        opus_head.extend_from_slice(&preskip.to_le_bytes());
        opus_head.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        opus_head.extend_from_slice(&0i16.to_le_bytes()); // output gain
        opus_head.push(0); // channel mapping family

        self.ogg
            .write_packet(opus_head, self.serial, PacketWriteEndInfo::EndPage, 0)?;

        // OpusTags
        let vendor = b"bark";
        let mut opus_tags = Vec::new();
        opus_tags.extend_from_slice(b"OpusTags");
        opus_tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
        opus_tags.extend_from_slice(vendor);
        opus_tags.extend_from_slice(&0u32.to_le_bytes()); // 0 comment entries

        self.ogg
            .write_packet(opus_tags, self.serial, PacketWriteEndInfo::EndPage, 0)?;

        Ok(())
    }

    fn encode(&mut self, pcm: &[i16; OPUS_FRAME_SIZE]) -> Result<(), EncodeError> {
        let packet = self.opus.encode_vec(pcm, 4000)?;

        if let Some(packet) = self.pending.replace(packet) {
            self.granule_pos += GRANULE_PER_FRAME;
            self.ogg.write_packet(
                packet,
                self.serial,
                ogg::writing::PacketWriteEndInfo::NormalPacket,
                self.granule_pos,
            )?;
        };

        Ok(())
    }

    fn finish(mut self) -> Result<W, EncodeError> {
        let packet = self.pending.take().unwrap_or_default();

        // Technically opus packets of length 0 are considered corrupted.
        // We'll send them anyway for empty streams to singal error for decoder.
        self.granule_pos += GRANULE_PER_FRAME;
        self.ogg.write_packet(
            packet,
            self.serial,
            ogg::writing::PacketWriteEndInfo::EndStream,
            self.granule_pos,
        )?;

        Ok(self.ogg.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_sine(duration_secs: f32) -> Vec<i16> {
        let num_samples = (SAMPLE_RATE as f32 * duration_secs) as usize;
        let frequency = 440.0_f32;

        (0..num_samples)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                let sample = (2.0 * std::f32::consts::PI * frequency * t).sin();
                (sample * i16::MAX as f32) as i16
            })
            .collect()
    }

    #[test]
    #[ignore = "manual"]
    fn encode_sine() {
        let pcm = generate_sine(10f32);
        let mut left: &[i16] = &pcm;

        let mut encoder = OpusEncoder::new(vec![]).unwrap_or_else(|err| {
            panic!("failed to create encoder: {err}");
        });

        let mut feed = |slice| {
            encoder.feed(slice).unwrap_or_else(|err| {
                panic!("failed to feed slice: {err}");
            });
        };

        for sz in [42, 413, 1337, 37, 234, 1000].iter().cycle() {
            let Some((slice, tail)) = left.split_at_checked(*sz as usize) else {
                break;
            };

            feed(slice);

            left = tail;
        }

        if !left.is_empty() {
            feed(left);
        }

        let out = encoder.finish().unwrap_or_else(|err| {
            panic!("failed to finish: {err}");
        });

        std::fs::create_dir_all("test_output").unwrap();
        std::fs::write("test_output/test.ogg", &out).unwrap();
    }
}
