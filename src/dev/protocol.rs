use std::fmt;
use std::io::{self, Read};

pub const FRAME_WIDTH: u16 = 320;
pub const FRAME_HEIGHT: u16 = 180;
pub const FRAME_CHANNELS: usize = 3;
pub const FRAME_PAYLOAD_BYTES: usize =
    FRAME_WIDTH as usize * FRAME_HEIGHT as usize * FRAME_CHANNELS;
pub const FRAME_HEADER_BYTES: usize = 24;
pub const CONTROL_MESSAGE_BYTES: usize = 8;
pub const BROWSER_INPUT_MAX_BYTES: usize = 128;

const MAGIC: &[u8; 4] = b"SPKF";
const VERSION: u8 = 1;
const PIXEL_FORMAT_RGB8: u8 = 1;
const CONTROL_MAGIC: &[u8; 4] = b"SPKI";
const CONTROL_VERSION: u8 = 1;
const CONTROL_KEY: u8 = 1;
const CONTROL_RELEASE_ALL: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Key {
    W = 0,
    A = 1,
    S = 2,
    D = 3,
    Up = 4,
    Down = 5,
    Left = 6,
    Right = 7,
    Space = 8,
    Enter = 9,
    Escape = 10,
}

impl Key {
    fn from_id(id: u8) -> Option<Self> {
        Some(match id {
            0 => Self::W,
            1 => Self::A,
            2 => Self::S,
            3 => Self::D,
            4 => Self::Up,
            5 => Self::Down,
            6 => Self::Left,
            7 => Self::Right,
            8 => Self::Space,
            9 => Self::Enter,
            10 => Self::Escape,
            _ => return None,
        })
    }

    fn from_browser_code(code: &str) -> Option<Self> {
        Some(match code {
            "KeyW" => Self::W,
            "KeyA" => Self::A,
            "KeyS" => Self::S,
            "KeyD" => Self::D,
            "ArrowUp" => Self::Up,
            "ArrowDown" => Self::Down,
            "ArrowLeft" => Self::Left,
            "ArrowRight" => Self::Right,
            "Space" => Self::Space,
            "Enter" => Self::Enter,
            "Escape" => Self::Escape,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlMessage {
    Key { key: Key, down: bool },
    ReleaseAll,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrowserInput {
    Key {
        client: String,
        key: Key,
        down: bool,
    },
    ReleaseAll {
        client: String,
    },
    Heartbeat {
        client: String,
    },
    UnsupportedKey {
        client: String,
    },
}

impl BrowserInput {
    pub fn client(&self) -> &str {
        match self {
            Self::Key { client, .. }
            | Self::ReleaseAll { client }
            | Self::Heartbeat { client }
            | Self::UnsupportedKey { client } => client,
        }
    }
}

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

pub fn encode_control(message: ControlMessage) -> [u8; CONTROL_MESSAGE_BYTES] {
    let mut encoded = [0_u8; CONTROL_MESSAGE_BYTES];
    encoded[..4].copy_from_slice(CONTROL_MAGIC);
    encoded[4] = CONTROL_VERSION;
    match message {
        ControlMessage::Key { key, down } => {
            encoded[5] = CONTROL_KEY;
            encoded[6] = key as u8;
            encoded[7] = u8::from(down);
        }
        ControlMessage::ReleaseAll => encoded[5] = CONTROL_RELEASE_ALL,
    }
    encoded
}

pub fn decode_control(bytes: &[u8]) -> Result<ControlMessage, ProtocolError> {
    if bytes.len() != CONTROL_MESSAGE_BYTES {
        return Err(ProtocolError::new(format!(
            "input control message must contain {CONTROL_MESSAGE_BYTES} bytes, found {}",
            bytes.len()
        )));
    }
    if &bytes[..4] != CONTROL_MAGIC {
        return Err(ProtocolError::new(
            "invalid input control magic; expected `SPKI`",
        ));
    }
    if bytes[4] != CONTROL_VERSION {
        return Err(ProtocolError::new(format!(
            "unsupported input protocol version {}; expected {CONTROL_VERSION}",
            bytes[4]
        )));
    }
    match bytes[5] {
        CONTROL_KEY if bytes[7] <= 1 => {
            let key = Key::from_id(bytes[6])
                .ok_or_else(|| ProtocolError::new("unsupported input key identifier"))?;
            Ok(ControlMessage::Key {
                key,
                down: bytes[7] != 0,
            })
        }
        CONTROL_RELEASE_ALL if bytes[6] == 0 && bytes[7] == 0 => Ok(ControlMessage::ReleaseAll),
        CONTROL_KEY => Err(ProtocolError::new("invalid key transition state")),
        CONTROL_RELEASE_ALL => Err(ProtocolError::new("invalid release-all payload")),
        kind => Err(ProtocolError::new(format!(
            "unsupported input control message kind {kind}"
        ))),
    }
}

pub fn parse_browser_input(body: &[u8]) -> Result<BrowserInput, ProtocolError> {
    if body.len() > BROWSER_INPUT_MAX_BYTES {
        return Err(ProtocolError::new(format!(
            "browser input message exceeded {BROWSER_INPUT_MAX_BYTES} bytes"
        )));
    }
    let text = std::str::from_utf8(body)
        .map_err(|_| ProtocolError::new("browser input message was not UTF-8"))?;
    let fields = text.split_ascii_whitespace().collect::<Vec<_>>();
    if fields.len() != 3 {
        return Err(ProtocolError::new(
            "browser input message must contain `client kind code`",
        ));
    }
    let client = fields[0];
    if client.is_empty()
        || client.len() > 64
        || !client
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ProtocolError::new(
            "invalid browser input client identifier",
        ));
    }
    let client = client.to_owned();
    match fields[1] {
        "down" | "up" => {
            let Some(key) = Key::from_browser_code(fields[2]) else {
                return Ok(BrowserInput::UnsupportedKey { client });
            };
            Ok(BrowserInput::Key {
                client,
                key,
                down: fields[1] == "down",
            })
        }
        "release" if fields[2] == "-" => Ok(BrowserInput::ReleaseAll { client }),
        "heartbeat" if fields[2] == "-" => Ok(BrowserInput::Heartbeat { client }),
        "release" | "heartbeat" => Err(ProtocolError::new(
            "release and heartbeat messages must use `-` as their code",
        )),
        kind => Err(ProtocolError::new(format!(
            "unsupported browser input message kind `{kind}`"
        ))),
    }
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

    #[test]
    fn round_trips_fixed_input_control_messages() {
        for message in [
            ControlMessage::Key {
                key: Key::A,
                down: true,
            },
            ControlMessage::Key {
                key: Key::Escape,
                down: false,
            },
            ControlMessage::ReleaseAll,
        ] {
            let encoded = encode_control(message);
            assert_eq!(encoded.len(), CONTROL_MESSAGE_BYTES);
            assert_eq!(
                decode_control(&encoded).expect("message should decode"),
                message
            );
        }
    }

    #[test]
    fn rejects_malformed_truncated_and_oversized_input_messages() {
        assert!(decode_control(b"SPKI").is_err());
        let mut invalid = encode_control(ControlMessage::ReleaseAll);
        invalid[0] = b'X';
        assert!(decode_control(&invalid).is_err());
        invalid = encode_control(ControlMessage::ReleaseAll);
        invalid[5] = 99;
        assert!(decode_control(&invalid).is_err());
        assert!(parse_browser_input(&[b'x'; BROWSER_INPUT_MAX_BYTES + 1]).is_err());
        assert!(parse_browser_input(b"missing-fields").is_err());
        assert!(parse_browser_input(b"client down \xFF").is_err());
    }

    #[test]
    fn parses_supported_browser_input_and_ignores_unsupported_keys() {
        assert_eq!(
            parse_browser_input(b"viewer-1 down ArrowLeft").expect("input should parse"),
            BrowserInput::Key {
                client: "viewer-1".into(),
                key: Key::Left,
                down: true,
            }
        );
        assert_eq!(
            parse_browser_input(b"viewer-1 up Space").expect("input should parse"),
            BrowserInput::Key {
                client: "viewer-1".into(),
                key: Key::Space,
                down: false,
            }
        );
        assert_eq!(
            parse_browser_input(b"viewer-1 release -").expect("input should parse"),
            BrowserInput::ReleaseAll {
                client: "viewer-1".into(),
            }
        );
        assert!(matches!(
            parse_browser_input(b"viewer-1 down KeyQ").expect("unsupported key should be safe"),
            BrowserInput::UnsupportedKey { .. }
        ));
    }
}
