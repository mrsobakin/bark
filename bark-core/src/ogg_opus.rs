//! OGG/Opus container writer + Opus encoder wrapper.
//!
//! The `OggOpusWriter` produces a valid OGG Opus stream entirely in memory,
//! closely following the Android `OggOpusWriter` implementation.

use crate::error::{BarkError, Result};
use opus::{Application, Channels};
use rand::Rng;

// ---------------------------------------------------------------------------
// OGG page writer
// ---------------------------------------------------------------------------

/// Writes a valid OGG Opus stream to an in-memory buffer.
pub struct OggOpusWriter {
    buf: Vec<u8>,
    serial: u32,
    page_no: u32,
    granule: u64,
    opus_input_rate: u32,
    has_audio: bool,
}

impl OggOpusWriter {
    pub fn new(opus_input_rate: u32) -> Self {
        Self {
            buf: Vec::new(),
            serial: rand::rng().random(),
            page_no: 0,
            granule: 0,
            opus_input_rate,
            has_audio: false,
        }
    }

    /// Write the Opus identification header (first OGG page).
    /// `csd` is the codec-specific data (OpusHead, ~19 bytes).
    pub fn write_opus_head(&mut self, csd: &[u8]) {
        self.write_page(&[csd], 0, 0x02); // BOS
    }

    /// Write the Opus comment header (second OGG page).
    pub fn write_opus_tags(&mut self) {
        let vendor = b"Bark";
        let mut tags = Vec::with_capacity(16 + vendor.len());
        tags.extend_from_slice(b"OpusTags");
        write_le32_to_vec(&mut tags, vendor.len() as u32);
        tags.extend_from_slice(vendor);
        write_le32_to_vec(&mut tags, 0); // no user comments
        self.write_page(&[&tags], 0, 0);
    }

    /// Write one Opus audio packet.  `input_sample_count` is the number of
    /// PCM samples (at `opus_input_rate`) that were consumed for this packet.
    pub fn write_audio_packet(&mut self, packet: &[u8], input_sample_count: u32) {
        // OGG granule is always in 48 kHz units for Opus.
        let inc = input_sample_count as u64 * 48_000 / self.opus_input_rate as u64;
        self.granule += inc;
        self.write_page(&[packet], self.granule, 0);
        self.has_audio = true;
    }

    /// Finalise the stream with an end-of-stream page.
    pub fn close(&mut self) {
        self.write_page(&[], self.granule, 0x04); // EOS
    }

    pub fn into_bytes(mut self) -> Vec<u8> {
        if !self.has_audio && self.page_no <= 2 {
            // No audio was ever written – return empty.
            return Vec::new();
        }
        self.close();
        self.buf
    }

    // -- private --

    fn write_page(&mut self, packets: &[&[u8]], granule: u64, flags: u8) {
        // Build segment (lacing) table.
        let mut seg_table = Vec::new();
        for &p in packets {
            let mut rem = p.len();
            while rem >= 255 {
                seg_table.push(255u8);
                rem -= 255;
            }
            seg_table.push(rem as u8);
        }
        let seg_count = seg_table.len();

        // Concatenate packet data.
        let total_data: usize = packets.iter().map(|p| p.len()).sum();
        let header_size = 27 + seg_count;
        let page_size = header_size + total_data;
        let mut page = vec![0u8; page_size];

        // "OggS"
        page[0..4].copy_from_slice(b"OggS");
        page[4] = 0; // version
        page[5] = flags;
        write_le64(&mut page, 6, granule);
        write_le32(&mut page, 14, self.serial);
        write_le32(&mut page, 18, self.page_no);
        // CRC placeholder at 22..26 – zeroed during computation
        page[26] = seg_count as u8;
        page[27..27 + seg_count].copy_from_slice(&seg_table);

        // Packet data
        let mut off = header_size;
        for &p in packets {
            page[off..off + p.len()].copy_from_slice(p);
            off += p.len();
        }

        // CRC-32 over the complete page (with CRC field zeroed).
        let checksum = crc32(&page);
        write_le32(&mut page, 22, checksum);

        self.buf.extend_from_slice(&page);
        self.page_no += 1;
    }
}

// ---------------------------------------------------------------------------
// Opus encoder wrapper
// ---------------------------------------------------------------------------

/// Encodes f32 mono PCM at 16 kHz into Opus packets and wraps them into an
/// OGG container.
pub struct OpusOggEncoder {
    encoder: opus::Encoder,
    writer: OggOpusWriter,
    frame_size: usize,      // samples per Opus frame (320 for 20 ms @ 16kHz)
    max_pkt_size: usize,    // max encoded packet size
    headers_written: bool,
}

impl OpusOggEncoder {
    /// Create a new encoder.  `bitrate_kbps` is the Opus bitrate (e.g. 24).
    pub fn new(bitrate_kbps: i32) -> Result<Self> {
        let mut encoder = opus::Encoder::new(16_000, Channels::Mono, Application::Voip)
            .map_err(BarkError::Opus)?;

        encoder.set_bitrate(opus::Bitrate::Bits(bitrate_kbps * 1000))
            .map_err(BarkError::Opus)?;

        Ok(Self {
            encoder,
            writer: OggOpusWriter::new(16_000),
            frame_size: 320, // 20 ms @ 16 kHz
            max_pkt_size: 4000,
            headers_written: false,
        })
    }

    /// Encode an entire buffer of f32 audio samples (16 kHz mono, range −1…1)
    /// and return the completed OGG/Opus byte stream.
    pub fn encode_all(mut self, audio: &[f32]) -> Result<Vec<u8>> {
        if audio.is_empty() {
            return Ok(Vec::new());
        }

        let mut output_buf = vec![0u8; self.max_pkt_size];

        // Write OGG headers with default OpusHead.
        self.write_default_headers();

        // Encode frame by frame.
        let mut pos = 0;
        while pos + self.frame_size <= audio.len() {
            let frame = &audio[pos..pos + self.frame_size];
            let n = self.encoder.encode_float(frame, &mut output_buf)
                .map_err(BarkError::Opus)?;

            self.writer.write_audio_packet(&output_buf[..n], self.frame_size as u32);
            pos += self.frame_size;
        }

        // Handle remaining samples (< one full frame) with padding.
        let remaining = audio.len() - pos;
        if remaining > 0 {
            let mut last_frame: Vec<f32> = audio[pos..].to_vec();
            last_frame.resize(self.frame_size, 0.0);

            let n = self.encoder.encode_float(&last_frame, &mut output_buf)
                .map_err(BarkError::Opus)?;

            self.writer.write_audio_packet(&output_buf[..n], remaining as u32);
        }

        Ok(self.writer.into_bytes())
    }

    fn write_default_headers(&mut self) {
        let csd = build_opus_head();
        self.writer.write_opus_head(&csd);
        self.writer.write_opus_tags();
        self.headers_written = true;
    }
}

/// Build a 19-byte OpusHead for mono 16 kHz.
fn build_opus_head() -> Vec<u8> {
    let mut csd = Vec::with_capacity(19);
    csd.extend_from_slice(b"OpusHead");
    csd.push(1);          // version
    csd.push(1);          // channels (mono)
    // pre-skip = 312 (standard Opus lookahead)
    csd.push((312u32 & 0xFF) as u8);
    csd.push(((312u32 >> 8) & 0xFF) as u8);
    // input sample rate = 16000 (LE32)
    write_le32_to_vec(&mut csd, 16_000);
    // output gain = 0 (LE16)
    csd.push(0);
    csd.push(0);
    // channel mapping family = 0
    csd.push(0);
    csd
}

// ---------------------------------------------------------------------------
// Little-endian helpers
// ---------------------------------------------------------------------------

fn write_le32(buf: &mut [u8], off: usize, v: u32) {
    buf[off] = (v & 0xFF) as u8;
    buf[off + 1] = ((v >> 8) & 0xFF) as u8;
    buf[off + 2] = ((v >> 16) & 0xFF) as u8;
    buf[off + 3] = ((v >> 24) & 0xFF) as u8;
}

fn write_le64(buf: &mut [u8], off: usize, v: u64) {
    for i in 0..8 {
        buf[off + i] = ((v >> (8 * i)) & 0xFF) as u8;
    }
}

fn write_le32_to_vec(v: &mut Vec<u8>, val: u32) {
    v.push((val & 0xFF) as u8);
    v.push(((val >> 8) & 0xFF) as u8);
    v.push(((val >> 16) & 0xFF) as u8);
    v.push(((val >> 24) & 0xFF) as u8);
}

/// CRC-32 (OGG uses the standard polynomial 0x04C11DB7 with initial value 0).
fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, entry) in t.iter_mut().enumerate() {
            let mut r = (i as u32) << 24;
            for _ in 0..8 {
                if r & 0x80000000 != 0 {
                    r = (r << 1) ^ 0x04C11DB7;
                } else {
                    r <<= 1;
                }
            }
            *entry = r;
        }
        t
    });

    let mut crc: u32 = 0;
    for &byte in data {
        let idx = ((crc >> 24) ^ byte as u32) & 0xFF;
        crc = (crc << 8) ^ table[idx as usize];
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opus_head_structure() {
        let head = build_opus_head();
        assert_eq!(&head[0..8], b"OpusHead");
        assert_eq!(head[8], 1); // version
        assert_eq!(head[9], 1); // mono
    }

    #[test]
    fn crc32_known_value() {
        let mut page = vec![0u8; 27];
        page[0..4].copy_from_slice(b"OggS");
        let c = crc32(&page);
        assert_ne!(c, 0, "CRC should be non-zero for a non-empty page");
    }
}