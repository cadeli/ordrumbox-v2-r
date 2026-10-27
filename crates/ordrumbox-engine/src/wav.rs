//! WAV decoding (RIFF/WAVE): integer PCM 8/16/24/32 bits, float 32/64 bits,
//! plain or `WAVE_FORMAT_EXTENSIBLE`. Channels beyond the first two are dropped.

use std::fmt;

use ordrumbox_dsp::SampleBuffer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WavError {
    NotWave,
    MissingChunk(&'static str),
    Unsupported(String),
}

impl fmt::Display for WavError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotWave => f.write_str("not a RIFF/WAVE file"),
            Self::MissingChunk(chunk) => write!(f, "no \"{chunk}\" chunk"),
            Self::Unsupported(what) => write!(f, "unsupported WAV: {what}"),
        }
    }
}

impl std::error::Error for WavError {}

const FORMAT_PCM: u16 = 1;
const FORMAT_FLOAT: u16 = 3;
const FORMAT_EXTENSIBLE: u16 = 0xFFFE;

struct Format {
    code: u16,
    channels: usize,
    sample_rate: u32,
    block_align: usize,
    bits: u16,
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// Decode a whole WAV file.
pub fn decode(bytes: &[u8]) -> Result<SampleBuffer, WavError> {
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(WavError::NotWave);
    }
    let mut format = None;
    let mut data: Option<&[u8]> = None;
    let mut at = 12;
    while let (Some(id), Some(size)) = (bytes.get(at..at + 4), u32_at(bytes, at + 4)) {
        let start = at + 8;
        // A data chunk longer than the file (truncated download) keeps what is there.
        let end = start.saturating_add(size as usize).min(bytes.len());
        let body = &bytes[start..end];
        match id {
            b"fmt " => format = Some(parse_format(body)?),
            b"data" => data = Some(body),
            _ => {}
        }
        at = end + (size as usize & 1); // chunks are word-aligned
    }
    let format = format.ok_or(WavError::MissingChunk("fmt "))?;
    let data = data.ok_or(WavError::MissingChunk("data"))?;
    decode_samples(&format, data)
}

fn parse_format(body: &[u8]) -> Result<Format, WavError> {
    let truncated = || WavError::Unsupported("truncated fmt chunk".into());
    let mut code = u16_at(body, 0).ok_or_else(truncated)?;
    if code == FORMAT_EXTENSIBLE {
        code = u16_at(body, 24).ok_or_else(truncated)?; // first bytes of the sub-format GUID
    }
    Ok(Format {
        code,
        channels: usize::from(u16_at(body, 2).ok_or_else(truncated)?),
        sample_rate: u32_at(body, 4).ok_or_else(truncated)?,
        block_align: usize::from(u16_at(body, 12).ok_or_else(truncated)?),
        bits: u16_at(body, 14).ok_or_else(truncated)?,
    })
}

fn decode_samples(format: &Format, data: &[u8]) -> Result<SampleBuffer, WavError> {
    let bytes_per_sample = usize::from(format.bits.div_ceil(8));
    let read: fn(&[u8]) -> f32 = match (format.code, format.bits) {
        (FORMAT_PCM, 8) => |b| (f32::from(b[0]) - 128.0) / 128.0,
        (FORMAT_PCM, 16) => |b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32_768.0,
        (FORMAT_PCM, 24) => |b| (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0,
        (FORMAT_PCM, 32) => |b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0,
        (FORMAT_FLOAT, 32) => |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        (FORMAT_FLOAT, 64) => |b| f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]) as f32,
        (code, bits) => return Err(WavError::Unsupported(format!("format {code:#06x}, {bits} bits"))),
    };
    if format.channels == 0 || format.sample_rate == 0 || format.block_align < format.channels * bytes_per_sample {
        return Err(WavError::Unsupported("inconsistent fmt chunk".into()));
    }
    let frames = data.len() / format.block_align;
    let channel = |c: usize| -> Vec<f32> {
        (0..frames).map(|f| read(&data[f * format.block_align + c * bytes_per_sample..])).collect()
    };
    Ok(SampleBuffer {
        left: channel(0),
        right: (format.channels >= 2).then(|| channel(1)),
        sample_rate: format.sample_rate,
    })
}

/// A stereo 16-bit PCM WAV file (`bufferToWav`): samples clamped to ±1,
/// negative ones scaled by 32768 and positive ones by 32767, truncated.
pub fn encode(left: &[f32], right: &[f32], sample_rate: u32) -> Vec<u8> {
    let frames = left.len().min(right.len());
    let data_len = (frames * 4) as u32;
    let mut out = Vec::with_capacity(44 + frames * 4);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..frames {
        for sample in [left[i], right[i]] {
            let s = f64::from(sample).clamp(-1.0, 1.0);
            let value = if s < 0.0 { s * 32768.0 } else { s * 32767.0 };
            out.extend_from_slice(&(value.trunc() as i16).to_le_bytes());
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A WAV file from its fmt fields and raw data (also used by other tests).
    pub(crate) fn wav(code: u16, channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let block = channels * bits.div_ceil(8);
        let mut out = Vec::new();
        out.extend(b"RIFF");
        out.extend(((4 + 8 + 16 + 8 + data.len() + 8 + 2) as u32).to_le_bytes());
        out.extend(b"WAVE");
        out.extend(b"LIST");
        out.extend(1u32.to_le_bytes());
        out.extend([0, 0]); // odd chunk + pad byte
        out.extend(b"fmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(code.to_le_bytes());
        out.extend(channels.to_le_bytes());
        out.extend(rate.to_le_bytes());
        out.extend((rate * u32::from(block)).to_le_bytes());
        out.extend(block.to_le_bytes());
        out.extend(bits.to_le_bytes());
        out.extend(b"data");
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        out
    }

    #[test]
    fn pcm_24_mono() {
        let data = [0x00, 0x00, 0x40, 0x00, 0x00, 0xC0, 0xFF, 0xFF, 0x7F];
        let buffer = decode(&wav(1, 1, 44_100, 24, &data)).unwrap();
        assert_eq!(buffer.left, [0.5, -0.5, 1.0 - 1.0 / 8_388_608.0]);
        assert_eq!((buffer.right, buffer.sample_rate), (None, 44_100));
    }

    #[test]
    fn pcm_16_stereo_and_float() {
        let data: Vec<u8> = [16_384i16, -32_768, 0, 8_192].iter().flat_map(|s| s.to_le_bytes()).collect();
        let buffer = decode(&wav(1, 2, 48_000, 16, &data)).unwrap();
        assert_eq!(buffer.left, [0.5, 0.0]);
        assert_eq!(buffer.right, Some(vec![-1.0, 0.25]));
        let data: Vec<u8> = [0.25f32, -1.0].iter().flat_map(|s| s.to_le_bytes()).collect();
        assert_eq!(decode(&wav(3, 1, 8_000, 32, &data)).unwrap().left, [0.25, -1.0]);
    }

    #[test]
    fn rejects_what_it_cannot_read() {
        assert_eq!(decode(b"nope"), Err(WavError::NotWave));
        assert!(matches!(decode(&wav(2, 1, 8_000, 4, &[0])), Err(WavError::Unsupported(_))));
        let mut no_data = wav(1, 1, 8_000, 16, &[]);
        no_data.truncate(no_data.len() - 8);
        assert_eq!(decode(&no_data), Err(WavError::MissingChunk("data")));
    }
}
