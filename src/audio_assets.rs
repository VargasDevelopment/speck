use std::{io::Read, path::Path};

use crate::source::SourceMap;

const MAX_WAV_BYTES: u64 = 32 * 1024 * 1024;
const MAX_SAMPLES: usize = 180 * 48_000;

pub(crate) fn load(path: &Path, sources: &mut SourceMap) -> Result<Vec<u8>, String> {
    sources.record_dependency(path.to_owned());
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("could not read sound asset `{}`: {error}", path.display()))?;
    sources.record_dependency(canonical.clone());
    let metadata = std::fs::metadata(&canonical)
        .map_err(|error| format!("could not read sound asset `{}`: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!(
            "sound asset `{}` is not a regular file",
            path.display()
        ));
    }
    if metadata.len() > MAX_WAV_BYTES {
        return Err(format!(
            "sound asset `{}` exceeds the 32 MiB encoded size limit",
            path.display()
        ));
    }
    // Bound the read itself as well: an authoring tool can grow the file after
    // the metadata check while a watched build is loading it.
    let file = std::fs::File::open(&canonical)
        .map_err(|error| format!("could not read sound asset `{}`: {error}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("could not read sound asset `{}`: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!(
            "sound asset `{}` is not a regular file",
            path.display()
        ));
    }
    let bytes = read_bounded(file)
        .map_err(|error| format!("could not read sound asset `{}`: {error}", path.display()))?;
    if bytes.len() as u64 > MAX_WAV_BYTES {
        return Err(format!(
            "sound asset `{}` exceeds the 32 MiB encoded size limit",
            path.display()
        ));
    }
    validate(&bytes)
        .map_err(|message| format!("invalid sound asset `{}`: {message}", path.display()))
}

fn read_bounded(reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(MAX_WAV_BYTES + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn validate(bytes: &[u8]) -> Result<Vec<u8>, &'static str> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("expected a RIFF/WAVE file");
    }
    let riff_size = little_u32(bytes, 4)? as usize;
    if riff_size.checked_add(8) != Some(bytes.len()) {
        return Err("RIFF size does not match the file length");
    }

    let mut cursor = 12usize;
    let mut format = None;
    let mut data = None;
    while cursor < bytes.len() {
        let header_end = cursor.checked_add(8).ok_or("chunk offset overflow")?;
        if header_end > bytes.len() {
            return Err("truncated WAV chunk header");
        }
        let size = little_u32(bytes, cursor + 4)? as usize;
        let payload_end = header_end.checked_add(size).ok_or("chunk size overflow")?;
        if payload_end > bytes.len() {
            return Err("WAV chunk extends beyond the RIFF data");
        }
        match &bytes[cursor..cursor + 4] {
            b"fmt " if format.is_some() => return Err("contains more than one `fmt ` chunk"),
            b"fmt " => format = Some((header_end, size)),
            b"data" if data.is_some() => return Err("contains more than one `data` chunk"),
            b"data" => data = Some((header_end, payload_end)),
            _ => {}
        }
        cursor = payload_end
            .checked_add(size & 1)
            .ok_or("chunk padding overflow")?;
        if cursor > bytes.len() {
            return Err("truncated WAV chunk padding");
        }
    }

    let (format_start, format_size) = format.ok_or("missing `fmt ` chunk")?;
    if format_size < 16 {
        return Err("`fmt ` chunk is shorter than 16 bytes");
    }
    if little_u16(bytes, format_start)? != 1 {
        return Err("audio format must be uncompressed PCM (format 1)");
    }
    if little_u16(bytes, format_start + 2)? != 1 {
        return Err("PCM must have exactly one channel");
    }
    if little_u32(bytes, format_start + 4)? != 48_000 {
        return Err("PCM sample rate must be 48000 Hz");
    }
    if little_u32(bytes, format_start + 8)? != 96_000 || little_u16(bytes, format_start + 12)? != 2
    {
        return Err("PCM byte rate and block alignment must match mono PCM16");
    }
    if little_u16(bytes, format_start + 14)? != 16 {
        return Err("PCM samples must be 16-bit");
    }

    let (data_start, data_end) = data.ok_or("missing `data` chunk")?;
    let data_len = data_end - data_start;
    if data_len == 0 {
        return Err("PCM data must contain at least one sample");
    }
    if data_len % 2 != 0 {
        return Err("PCM data length must be a whole number of 16-bit samples");
    }
    if data_len / 2 > MAX_SAMPLES {
        return Err("decoded audio exceeds the 180 second duration limit");
    }
    Ok(bytes[data_start..data_end].to_vec())
}

fn little_u16(bytes: &[u8], offset: usize) -> Result<u16, &'static str> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or("truncated WAV integer")?
        .try_into()
        .unwrap();
    Ok(u16::from_le_bytes(value))
}

fn little_u32(bytes: &[u8], offset: usize) -> Result<u32, &'static str> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or("truncated WAV integer")?
        .try_into()
        .unwrap();
    Ok(u32::from_le_bytes(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_read_stops_at_the_limit_even_when_the_input_keeps_growing() {
        let bytes = read_bounded(std::io::repeat(0)).unwrap();
        assert_eq!(bytes.len() as u64, MAX_WAV_BYTES + 1);
    }

    fn wav(samples: &[i16]) -> Vec<u8> {
        let data_len = u32::try_from(samples.len() * 2).unwrap();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&48_000u32.to_le_bytes());
        bytes.extend_from_slice(&96_000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn validates_the_bounded_pcm_subset_and_extracts_samples() {
        let bytes = wav(&[-32768, 0, 32767]);
        assert_eq!(validate(&bytes).unwrap(), bytes[44..]);
        let mut wrong_rate = bytes.clone();
        wrong_rate[24..28].copy_from_slice(&44_100u32.to_le_bytes());
        assert_eq!(
            validate(&wrong_rate),
            Err("PCM sample rate must be 48000 Hz")
        );
        let mut truncated = bytes;
        truncated.pop();
        assert_eq!(
            validate(&truncated),
            Err("RIFF size does not match the file length")
        );
    }

    #[test]
    fn rejects_decoded_audio_beyond_three_minutes() {
        let samples = vec![0; MAX_SAMPLES + 1];
        assert_eq!(
            validate(&wav(&samples)),
            Err("decoded audio exceeds the 180 second duration limit")
        );
    }
}
