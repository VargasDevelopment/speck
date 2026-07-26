use std::fmt;
use std::io::{self, Read};

pub const FRAME_WIDTH: u16 = 320;
pub const FRAME_HEIGHT: u16 = 180;
pub const FRAME_CHANNELS: usize = 3;
pub const FRAME_PAYLOAD_BYTES: usize =
    FRAME_WIDTH as usize * FRAME_HEIGHT as usize * FRAME_CHANNELS;
pub const FRAME_HEADER_BYTES: usize = 24;

const MAGIC: &[u8; 4] = b"SPKF";
const VERSION: u8 = 1;
const PIXEL_FORMAT_RGB8: u8 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    pub sequence: u64,
    pub pixels: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolError(String);

impl ProtocolError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ProtocolError {}

pub fn encode_frame(sequence: u64, pixels: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    if pixels.len() != FRAME_PAYLOAD_BYTES {
        return Err(ProtocolError::new(format!(
            "RGB8 payload must contain {FRAME_PAYLOAD_BYTES} bytes, found {}",
            pixels.len()
        )));
    }

    let mut encoded = Vec::with_capacity(FRAME_HEADER_BYTES + FRAME_PAYLOAD_BYTES);
    encoded.extend_from_slice(MAGIC);
    encoded.push(VERSION);
    encoded.push(PIXEL_FORMAT_RGB8);
    encoded.extend_from_slice(&(FRAME_HEADER_BYTES as u16).to_be_bytes());
    encoded.extend_from_slice(&FRAME_WIDTH.to_be_bytes());
    encoded.extend_from_slice(&FRAME_HEIGHT.to_be_bytes());
    encoded.extend_from_slice(&(FRAME_PAYLOAD_BYTES as u32).to_be_bytes());
    encoded.extend_from_slice(&sequence.to_be_bytes());
    encoded.extend_from_slice(pixels);
    Ok(encoded)
}

pub fn read_frame(reader: &mut impl Read) -> Result<Option<Frame>, ProtocolError> {
    let mut header = [0_u8; FRAME_HEADER_BYTES];
    if !read_first_byte(reader, &mut header[0])? {
        return Ok(None);
    }
    reader
        .read_exact(&mut header[1..])
        .map_err(|error| truncated("frame header", error))?;
    let sequence = validate_header(&header)?;

    let mut pixels = vec![0_u8; FRAME_PAYLOAD_BYTES];
    reader
        .read_exact(&mut pixels)
        .map_err(|error| truncated("frame payload", error))?;
    Ok(Some(Frame { sequence, pixels }))
}

fn read_first_byte(reader: &mut impl Read, byte: &mut u8) -> Result<bool, ProtocolError> {
    loop {
        match reader.read(std::slice::from_mut(byte)) {
            Ok(0) => return Ok(false),
            Ok(1) => return Ok(true),
            Ok(_) => unreachable!("the read buffer contains exactly one byte"),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                return Err(ProtocolError::new(format!(
                    "could not read frame header: {error}"
                )));
            }
        }
    }
}

fn validate_header(header: &[u8; FRAME_HEADER_BYTES]) -> Result<u64, ProtocolError> {
    if &header[0..4] != MAGIC {
        return Err(ProtocolError::new("invalid frame magic; expected `SPKF`"));
    }
    if header[4] != VERSION {
        return Err(ProtocolError::new(format!(
            "unsupported frame protocol version {}; expected {VERSION}",
            header[4]
        )));
    }
    if header[5] != PIXEL_FORMAT_RGB8 {
        return Err(ProtocolError::new(format!(
            "unsupported pixel format {}; expected RGB8 ({PIXEL_FORMAT_RGB8})",
            header[5]
        )));
    }

    let header_bytes = u16::from_be_bytes([header[6], header[7]]) as usize;
    let width = u16::from_be_bytes([header[8], header[9]]);
    let height = u16::from_be_bytes([header[10], header[11]]);
    let payload_bytes =
        u32::from_be_bytes([header[12], header[13], header[14], header[15]]) as usize;
    if header_bytes != FRAME_HEADER_BYTES {
        return Err(ProtocolError::new(format!(
            "invalid frame header length {header_bytes}; expected {FRAME_HEADER_BYTES}"
        )));
    }
    if width != FRAME_WIDTH || height != FRAME_HEIGHT {
        return Err(ProtocolError::new(format!(
            "invalid framebuffer dimensions {width}x{height}; expected {FRAME_WIDTH}x{FRAME_HEIGHT}"
        )));
    }
    if payload_bytes != FRAME_PAYLOAD_BYTES {
        return Err(ProtocolError::new(format!(
            "invalid frame payload length {payload_bytes}; expected {FRAME_PAYLOAD_BYTES}"
        )));
    }
    Ok(u64::from_be_bytes([
        header[16], header[17], header[18], header[19], header[20], header[21], header[22],
        header[23],
    ]))
}

fn truncated(part: &str, error: io::Error) -> ProtocolError {
    ProtocolError::new(format!("truncated {part}: {error}"))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn sample_pixels() -> Vec<u8> {
        (0..FRAME_PAYLOAD_BYTES)
            .map(|index| (index % 251) as u8)
            .collect()
    }

    #[test]
    fn frame_dimensions_and_byte_count_are_fixed() {
        assert_eq!(FRAME_WIDTH, 320);
        assert_eq!(FRAME_HEIGHT, 180);
        assert_eq!(FRAME_PAYLOAD_BYTES, 172_800);
        assert_eq!(FRAME_HEADER_BYTES, 24);
    }

    #[test]
    fn round_trips_a_complete_rgb_frame() {
        let pixels = sample_pixels();
        let encoded = encode_frame(42, &pixels).expect("frame should encode");
        assert_eq!(encoded.len(), FRAME_HEADER_BYTES + FRAME_PAYLOAD_BYTES);

        let frame = read_frame(&mut Cursor::new(encoded))
            .expect("frame should decode")
            .expect("one frame should be present");
        assert_eq!(frame.sequence, 42);
        assert_eq!(frame.pixels, pixels);
    }

    #[test]
    fn rejects_invalid_metadata() {
        let pixels = sample_pixels();
        let mut encoded = encode_frame(1, &pixels).expect("frame should encode");
        encoded[8..10].copy_from_slice(&321_u16.to_be_bytes());
        let error = read_frame(&mut Cursor::new(encoded)).expect_err("width should be rejected");
        assert!(error.to_string().contains("dimensions 321x180"));

        let mut encoded = encode_frame(1, &pixels).expect("frame should encode");
        encoded[12..16].copy_from_slice(&12_u32.to_be_bytes());
        let error = read_frame(&mut Cursor::new(encoded)).expect_err("length should be rejected");
        assert!(error.to_string().contains("payload length 12"));
    }

    #[test]
    fn rejects_truncated_headers_and_payloads() {
        let pixels = sample_pixels();
        let encoded = encode_frame(1, &pixels).expect("frame should encode");
        let header_error =
            read_frame(&mut Cursor::new(&encoded[..10])).expect_err("partial header should fail");
        assert!(header_error.to_string().contains("truncated frame header"));

        let payload_error = read_frame(&mut Cursor::new(&encoded[..FRAME_HEADER_BYTES + 10]))
            .expect_err("partial payload should fail");
        assert!(
            payload_error
                .to_string()
                .contains("truncated frame payload")
        );
    }

    #[test]
    fn clean_eof_has_no_frame() {
        assert_eq!(
            read_frame(&mut Cursor::new(Vec::<u8>::new())).expect("EOF should be clean"),
            None
        );
    }
}
