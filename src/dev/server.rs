use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::protocol::{self, BrowserInput, ControlMessage, Frame};

const VIEWER_HTML: &[u8] = include_bytes!("viewer.html");
const INPUT_LEASE_TIMEOUT: Duration = Duration::from_secs(1);
const HTTP_HEADER_MAX_BYTES: usize = 8192;

#[derive(Clone, Default)]
pub struct FrameStore {
    shared: Arc<(Mutex<FrameState>, Condvar)>,
}

#[derive(Default)]
struct FrameState {
    latest: Option<Frame>,
    stopped: bool,
}

struct FrameSnapshot {
    frame: Option<Frame>,
    stopped: bool,
}

impl FrameStore {
    pub fn publish(&self, frame: Frame) {
        let (state, changed) = &*self.shared;
        let mut state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.latest = Some(frame);
        changed.notify_all();
    }

    pub fn stop(&self) {
        let (state, changed) = &*self.shared;
        let mut state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.stopped = true;
        changed.notify_all();
    }

    pub fn latest_sequence(&self) -> Option<u64> {
        let (state, _) = &*self.shared;
        let state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.latest.as_ref().map(|frame| frame.sequence)
    }

    fn wait_after(&self, sequence: u64, timeout: Duration) -> FrameSnapshot {
        let (state, changed) = &*self.shared;
        let state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (state, _) = changed
            .wait_timeout_while(state, timeout, |state| {
                !state.stopped
                    && state
                        .latest
                        .as_ref()
                        .is_none_or(|frame| frame.sequence <= sequence)
            })
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let frame = state
            .latest
            .as_ref()
            .filter(|frame| frame.sequence > sequence)
            .cloned();
        FrameSnapshot {
            frame,
            stopped: state.stopped,
        }
    }
}

#[derive(Clone, Default)]
pub struct InputControl {
    shared: Arc<Mutex<InputControlState>>,
}

#[derive(Default)]
struct InputControlState {
    game: Option<TcpStream>,
    owner: Option<String>,
    last_seen: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputResult {
    Accepted,
    Ignored,
    Busy,
    GameUnavailable,
}

impl InputControl {
    pub fn connect_game(&self, stream: TcpStream) -> Result<(), String> {
        stream
            .set_write_timeout(Some(Duration::from_secs(1)))
            .map_err(|error| format!("could not configure input control timeout: {error}"))?;
        let mut state = self
            .shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.game = Some(stream);
        state.owner = None;
        state.last_seen = None;
        Ok(())
    }

    pub fn disconnect_game(&self) {
        let mut state = self
            .shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.game = None;
        state.owner = None;
        state.last_seen = None;
    }

    pub fn apply(&self, input: BrowserInput) -> InputResult {
        if matches!(&input, BrowserInput::UnsupportedKey { .. }) {
            return InputResult::Ignored;
        }
        let releases_control = matches!(&input, BrowserInput::ReleaseAll { .. });
        let mut state = self
            .shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.game.is_none() {
            return InputResult::GameUnavailable;
        }
        let client = input.client();
        if state.owner.as_deref().is_some_and(|owner| owner != client) {
            return InputResult::Busy;
        }
        if state.owner.is_none() {
            state.owner = Some(client.to_owned());
        }
        state.last_seen = Some(Instant::now());

        let message = match input {
            BrowserInput::Key { key, down, .. } => Some(ControlMessage::Key { key, down }),
            BrowserInput::ReleaseAll { .. } => Some(ControlMessage::ReleaseAll),
            BrowserInput::Heartbeat { .. } => None,
            BrowserInput::UnsupportedKey { .. } => unreachable!(),
        };
        if let Some(message) = message
            && !send_control(&mut state, message)
        {
            return InputResult::GameUnavailable;
        }
        if releases_control {
            state.owner = None;
            state.last_seen = None;
        }
        InputResult::Accepted
    }

    pub fn expire_lease(&self) {
        let mut state = self
            .shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.owner.is_some()
            && state
                .last_seen
                .is_some_and(|last_seen| last_seen.elapsed() >= INPUT_LEASE_TIMEOUT)
        {
            let _ = send_control(&mut state, ControlMessage::ReleaseAll);
            state.owner = None;
            state.last_seen = None;
        }
    }

    pub fn release_and_disconnect(&self) {
        let mut state = self
            .shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _ = send_control(&mut state, ControlMessage::ReleaseAll);
        state.game = None;
        state.owner = None;
        state.last_seen = None;
    }
}

fn send_control(state: &mut InputControlState, message: ControlMessage) -> bool {
    let encoded = protocol::encode_control(message);
    let sent = state
        .game
        .as_mut()
        .is_some_and(|stream| stream.write_all(&encoded).is_ok());
    if !sent {
        state.game = None;
        state.owner = None;
        state.last_seen = None;
    }
    sent
}

pub fn spawn_input_watchdog(controls: InputControl, shutdown: Arc<AtomicBool>) -> JoinHandle<()> {
    thread::spawn(move || {
        while !shutdown.load(Ordering::Acquire) {
            controls.expire_lease();
            thread::sleep(Duration::from_millis(50));
        }
        controls.release_and_disconnect();
    })
}

#[derive(Debug)]
pub struct HttpBinding {
    pub listener: TcpListener,
    pub address: SocketAddr,
    pub used_fallback_port: bool,
}

pub fn bind_http(bind: IpAddr, port: u16, allow_fallback: bool) -> Result<HttpBinding, String> {
    match TcpListener::bind((bind, port)) {
        Ok(listener) => binding(listener, false),
        Err(error)
            if allow_fallback
                && port != 0
                && matches!(
                    error.kind(),
                    io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied
                ) =>
        {
            TcpListener::bind((bind, 0))
                .map_err(|fallback| {
                    format!(
                        "could not bind development viewer to {bind}:{port} ({error}); automatic \
                         fallback also failed: {fallback}"
                    )
                })
                .and_then(|listener| binding(listener, true))
        }
        Err(error) => Err(format!(
            "could not bind development viewer to {bind}:{port}: {error}"
        )),
    }
}

fn binding(listener: TcpListener, used_fallback_port: bool) -> Result<HttpBinding, String> {
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("could not configure development HTTP listener: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("could not inspect development HTTP listener: {error}"))?;
    Ok(HttpBinding {
        listener,
        address,
        used_fallback_port,
    })
}

pub fn spawn_http_server(
    listener: TcpListener,
    frames: FrameStore,
    controls: InputControl,
    shutdown: Arc<AtomicBool>,
    fatal: Sender<String>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        while !shutdown.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let frames = frames.clone();
                    let controls = controls.clone();
                    thread::spawn(move || {
                        if let Err(error) = handle_http(stream, &frames, &controls) {
                            eprintln!("Speck viewer request failed: {error}");
                        }
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => {
                    let message = format!("development HTTP server failed: {error}");
                    let _ = fatal.send(message);
                    shutdown.store(true, Ordering::Release);
                    break;
                }
            }
        }
    })
}

pub fn bind_frame_listener() -> Result<TcpListener, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("could not bind loopback frame receiver: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("could not configure frame receiver: {error}"))?;
    Ok(listener)
}

pub fn spawn_frame_receiver(
    listener: TcpListener,
    frames: FrameStore,
    controls: InputControl,
    shutdown: Arc<AtomicBool>,
    fatal: Sender<String>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut stream = loop {
            if shutdown.load(Ordering::Acquire) {
                frames.stop();
                return;
            }
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => {
                    let message = format!("frame receiver failed before game connected: {error}");
                    let _ = fatal.send(message);
                    shutdown.store(true, Ordering::Release);
                    frames.stop();
                    return;
                }
            }
        };

        if let Err(error) = stream.set_nonblocking(false) {
            let _ = fatal.send(format!(
                "could not configure frame stream blocking mode: {error}"
            ));
            shutdown.store(true, Ordering::Release);
            frames.stop();
            return;
        }
        if let Err(error) = stream.set_read_timeout(Some(Duration::from_secs(2))) {
            let _ = fatal.send(format!("could not configure frame stream timeout: {error}"));
            shutdown.store(true, Ordering::Release);
            frames.stop();
            return;
        }
        let control_stream = match stream.try_clone() {
            Ok(stream) => stream,
            Err(error) => {
                let _ = fatal.send(format!("could not clone game stream for input: {error}"));
                shutdown.store(true, Ordering::Release);
                frames.stop();
                return;
            }
        };
        if let Err(error) = controls.connect_game(control_stream) {
            let _ = fatal.send(error);
            shutdown.store(true, Ordering::Release);
            frames.stop();
            return;
        }
        let mut last_sequence = 0;
        loop {
            match protocol::read_frame(&mut stream) {
                Ok(Some(frame)) if frame.sequence > last_sequence => {
                    last_sequence = frame.sequence;
                    frames.publish(frame);
                }
                Ok(Some(frame)) => {
                    let _ = fatal.send(format!(
                        "invalid native frame sequence {}; previous frame was {last_sequence}",
                        frame.sequence
                    ));
                    shutdown.store(true, Ordering::Release);
                    break;
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = fatal.send(format!("invalid native frame stream: {error}"));
                    shutdown.store(true, Ordering::Release);
                    break;
                }
            }
        }
        controls.disconnect_game();
        frames.stop();
    })
}

fn handle_http(
    mut stream: TcpStream,
    frames: &FrameStore,
    controls: &InputControl,
) -> Result<(), String> {
    stream
        .set_nonblocking(false)
        .map_err(|error| format!("could not configure HTTP connection blocking mode: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| format!("could not configure HTTP request timeout: {error}"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(|error| format!("could not configure HTTP response timeout: {error}"))?;
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            return respond(
                &mut stream,
                "400 Bad Request",
                "text/plain",
                &[],
                format!("{error}\n").as_bytes(),
            );
        }
    };

    if request.method == "GET" && request.path == "/" {
        return respond(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            &[("Cache-Control", "no-store")],
            VIEWER_HTML,
        );
    }
    if request.method == "GET" && request.path == "/favicon.ico" {
        return respond(&mut stream, "204 No Content", "text/plain", &[], &[]);
    }
    if request.method == "GET"
        && let Some(query) = request.path.strip_prefix("/frame")
    {
        if !query.is_empty() && !query.starts_with('?') {
            return respond(
                &mut stream,
                "404 Not Found",
                "text/plain",
                &[],
                b"not found\n",
            );
        }
        let after = parse_after(query)?;
        let snapshot = frames.wait_after(after, Duration::from_secs(2));
        if let Some(frame) = snapshot.frame {
            let sequence = frame.sequence.to_string();
            return respond(
                &mut stream,
                "200 OK",
                "application/octet-stream",
                &[
                    ("Cache-Control", "no-store"),
                    ("X-Speck-State", "live"),
                    ("X-Speck-Sequence", &sequence),
                    ("X-Speck-Width", "320"),
                    ("X-Speck-Height", "180"),
                    ("X-Speck-Format", "RGB8"),
                ],
                &frame.pixels,
            );
        }
        let state = if snapshot.stopped {
            "stopped"
        } else {
            "running"
        };
        return respond(
            &mut stream,
            "204 No Content",
            "application/octet-stream",
            &[("Cache-Control", "no-store"), ("X-Speck-State", state)],
            &[],
        );
    }
    if request.path == "/input" {
        if request.method != "POST" {
            return respond(
                &mut stream,
                "405 Method Not Allowed",
                "text/plain",
                &[("Allow", "POST")],
                b"input requires POST\n",
            );
        }
        let input = match protocol::parse_browser_input(&request.body) {
            Ok(input) => input,
            Err(error) => {
                return respond(
                    &mut stream,
                    "400 Bad Request",
                    "text/plain",
                    &[],
                    format!("{error}\n").as_bytes(),
                );
            }
        };
        return match controls.apply(input) {
            InputResult::Accepted | InputResult::Ignored => {
                respond(&mut stream, "204 No Content", "text/plain", &[], &[])
            }
            InputResult::Busy => respond(
                &mut stream,
                "409 Conflict",
                "text/plain",
                &[],
                b"another viewer currently controls input\n",
            ),
            InputResult::GameUnavailable => respond(
                &mut stream,
                "503 Service Unavailable",
                "text/plain",
                &[],
                b"game input is not connected\n",
            ),
        };
    }

    respond(
        &mut stream,
        "404 Not Found",
        "text/plain",
        &[],
        b"not found\n",
    )
}

struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    let header_end = loop {
        if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        if request.len() >= HTTP_HEADER_MAX_BYTES {
            return Err(format!(
                "HTTP request headers exceeded {HTTP_HEADER_MAX_BYTES} bytes"
            ));
        }
        let read = stream
            .read(&mut buffer)
            .map_err(|error| format!("could not read HTTP request: {error}"))?;
        if read == 0 {
            return Err("HTTP request ended before its headers were complete".into());
        }
        request.extend_from_slice(&buffer[..read]);
    };
    if header_end > HTTP_HEADER_MAX_BYTES {
        return Err(format!(
            "HTTP request headers exceeded {HTTP_HEADER_MAX_BYTES} bytes"
        ));
    }
    let headers = std::str::from_utf8(&request[..header_end])
        .map_err(|_| "HTTP request headers were not UTF-8".to_owned())?;
    let mut lines = headers.lines();
    let mut parts = lines
        .next()
        .ok_or_else(|| "HTTP request was empty".to_owned())?
        .split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "HTTP request did not include a method".to_owned())?
        .to_owned();
    if !matches!(method.as_str(), "GET" | "POST") {
        return Err("development viewer only accepts GET and POST requests".into());
    }
    let path = parts
        .next()
        .ok_or_else(|| "HTTP request did not include a path".to_owned())?
        .to_owned();
    let mut content_length = 0_usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("Content-Length")
        {
            content_length = value
                .trim()
                .parse()
                .map_err(|_| "HTTP Content-Length was not an unsigned integer".to_owned())?;
        }
    }
    if content_length > protocol::BROWSER_INPUT_MAX_BYTES {
        return Err(format!(
            "HTTP request body exceeded {} bytes",
            protocol::BROWSER_INPUT_MAX_BYTES
        ));
    }
    while request.len() < header_end + content_length {
        let remaining = header_end + content_length - request.len();
        let chunk = remaining.min(buffer.len());
        let read = stream
            .read(&mut buffer[..chunk])
            .map_err(|error| format!("could not read HTTP request body: {error}"))?;
        if read == 0 {
            return Err("HTTP request body was truncated".into());
        }
        request.extend_from_slice(&buffer[..read]);
    }
    Ok(HttpRequest {
        method,
        path,
        body: request[header_end..header_end + content_length].to_vec(),
    })
}

fn parse_after(query: &str) -> Result<u64, String> {
    if query.is_empty() || query == "?" {
        return Ok(0);
    }
    query
        .trim_start_matches('?')
        .split('&')
        .find_map(|part| part.strip_prefix("after="))
        .unwrap_or("0")
        .parse()
        .map_err(|_| "`after` frame sequence must be an unsigned integer".into())
}

fn respond(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Result<(), String> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    )
    .map_err(|error| format!("could not write HTTP response: {error}"))?;
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n")
            .map_err(|error| format!("could not write HTTP response: {error}"))?;
    }
    stream
        .write_all(b"\r\n")
        .and_then(|_| stream.write_all(body))
        .and_then(|_| stream.flush())
        .map_err(|error| format!("could not finish HTTP response: {error}"))
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;

    use super::*;
    use crate::dev::protocol::{
        FRAME_HEIGHT, FRAME_PAYLOAD_BYTES, FRAME_WIDTH, Key, decode_control,
    };

    #[test]
    fn serves_viewer_and_complete_binary_frame() {
        let binding = bind_http(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, false)
            .expect("HTTP listener should bind");
        let address = binding.address;
        let frames = FrameStore::default();
        let controls = InputControl::default();
        let shutdown = Arc::new(AtomicBool::new(false));
        let (fatal_tx, _fatal_rx) = mpsc::channel();
        let thread = spawn_http_server(
            binding.listener,
            frames.clone(),
            controls,
            shutdown.clone(),
            fatal_tx,
        );

        let page = get(address, "/");
        assert!(page.starts_with(b"HTTP/1.1 200 OK"));
        assert!(page.windows(7).any(|window| window == b"<canvas"));
        let page_text = String::from_utf8_lossy(&page);
        assert!(page_text.contains("event.code"));
        assert!(page_text.contains("event.repeat"));
        assert!(page_text.contains("event.preventDefault()"));
        assert!(page_text.contains("visibilitychange"));
        assert!(page_text.contains("pagehide"));
        assert!(page_text.contains("releaseAll"));
        assert!(page_text.contains("heartbeat"));
        assert!(page_text.contains("inputQueue = inputQueue.then"));

        let pixels = vec![73_u8; FRAME_PAYLOAD_BYTES];
        frames.publish(Frame {
            sequence: 9,
            pixels: pixels.clone(),
        });
        let response = get(address, "/frame?after=0");
        let body_start = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("response should contain headers")
            + 4;
        assert!(
            response[..body_start]
                .windows(19)
                .any(|window| window == b"X-Speck-Sequence: 9")
        );
        assert_eq!(&response[body_start..], pixels);

        shutdown.store(true, Ordering::Release);
        thread.join().expect("HTTP thread should stop");
    }

    #[test]
    fn accepted_http_connections_wait_for_delayed_requests() {
        let binding = bind_http(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, false)
            .expect("HTTP listener should bind");
        let address = binding.address;
        let frames = FrameStore::default();
        let controls = InputControl::default();
        let shutdown = Arc::new(AtomicBool::new(false));
        let (fatal_tx, _fatal_rx) = mpsc::channel();
        let thread = spawn_http_server(
            binding.listener,
            frames,
            controls,
            shutdown.clone(),
            fatal_tx,
        );

        let mut stream = TcpStream::connect(address).expect("test should connect");
        thread::sleep(Duration::from_millis(50));
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .expect("delayed request should write");
        let response = read_response(&mut stream);
        assert!(response.starts_with(b"HTTP/1.1 200 OK"));

        shutdown.store(true, Ordering::Release);
        thread.join().expect("HTTP thread should stop");
    }

    fn get(address: SocketAddr, path: &str) -> Vec<u8> {
        let mut stream = TcpStream::connect(address).expect("test should connect");
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        )
        .expect("request should write");
        read_response(&mut stream)
    }

    fn post(address: SocketAddr, path: &str, body: &[u8]) -> Vec<u8> {
        let mut stream = TcpStream::connect(address).expect("test should connect");
        write!(
            stream,
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .expect("request headers should write");
        stream.write_all(body).expect("request body should write");
        read_response(&mut stream)
    }

    fn read_response(stream: &mut TcpStream) -> Vec<u8> {
        let mut response = Vec::new();
        let mut buffer = [0_u8; 1024];
        let body_start = loop {
            if let Some(position) = response.windows(4).position(|window| window == b"\r\n\r\n") {
                break position + 4;
            }
            let read = stream.read(&mut buffer).expect("response should read");
            assert!(read > 0, "response headers should be complete");
            response.extend_from_slice(&buffer[..read]);
        };
        let headers =
            std::str::from_utf8(&response[..body_start]).expect("response headers should be UTF-8");
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("Content-Length").then(|| {
                    value
                        .trim()
                        .parse::<usize>()
                        .expect("content length should parse")
                })
            })
            .expect("response should declare content length");
        let response_length = body_start + content_length;
        while response.len() < response_length {
            let remaining = response_length - response.len();
            let chunk = remaining.min(buffer.len());
            let read = stream
                .read(&mut buffer[..chunk])
                .expect("response body should read");
            assert!(read > 0, "response body should be complete");
            response.extend_from_slice(&buffer[..read]);
        }
        response.truncate(response_length);
        response
    }

    fn connected_controls() -> (InputControl, TcpStream) {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("control listener should bind");
        let address = listener.local_addr().expect("address should exist");
        let game = TcpStream::connect(address).expect("game peer should connect");
        game.set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should configure");
        let (host, _) = listener.accept().expect("host peer should accept");
        let controls = InputControl::default();
        controls
            .connect_game(host)
            .expect("game control should connect");
        (controls, game)
    }

    fn read_control(game: &mut TcpStream) -> ControlMessage {
        let mut bytes = [0_u8; protocol::CONTROL_MESSAGE_BYTES];
        game.read_exact(&mut bytes)
            .expect("control record should arrive");
        decode_control(&bytes).expect("control record should be valid")
    }

    #[test]
    fn browser_input_reaches_game_with_single_controller_ownership() {
        let binding = bind_http(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, false)
            .expect("HTTP listener should bind");
        let address = binding.address;
        let (controls, mut game) = connected_controls();
        let frames = FrameStore::default();
        let shutdown = Arc::new(AtomicBool::new(false));
        let (fatal_tx, _fatal_rx) = mpsc::channel();
        let thread = spawn_http_server(
            binding.listener,
            frames,
            controls.clone(),
            shutdown.clone(),
            fatal_tx,
        );

        let response = post(address, "/input", b"viewer-1 down ArrowLeft");
        assert!(response.starts_with(b"HTTP/1.1 204 No Content"));
        assert_eq!(
            read_control(&mut game),
            ControlMessage::Key {
                key: Key::Left,
                down: true
            }
        );

        let repeated = post(address, "/input", b"viewer-1 down ArrowLeft");
        assert!(repeated.starts_with(b"HTTP/1.1 204 No Content"));
        assert_eq!(
            read_control(&mut game),
            ControlMessage::Key {
                key: Key::Left,
                down: true
            }
        );

        let busy = post(address, "/input", b"viewer-2 down KeyD");
        assert!(busy.starts_with(b"HTTP/1.1 409 Conflict"));

        let release = post(address, "/input", b"viewer-1 release -");
        assert!(release.starts_with(b"HTTP/1.1 204 No Content"));
        assert_eq!(read_control(&mut game), ControlMessage::ReleaseAll);

        let next = post(address, "/input", b"viewer-2 down KeyD");
        assert!(next.starts_with(b"HTTP/1.1 204 No Content"));
        assert_eq!(
            read_control(&mut game),
            ControlMessage::Key {
                key: Key::D,
                down: true
            }
        );

        shutdown.store(true, Ordering::Release);
        thread.join().expect("HTTP thread should stop");
    }

    #[test]
    fn malformed_oversized_and_unsupported_browser_input_is_safe() {
        let binding = bind_http(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, false)
            .expect("HTTP listener should bind");
        let address = binding.address;
        let (controls, _game) = connected_controls();
        let shutdown = Arc::new(AtomicBool::new(false));
        let (fatal_tx, _fatal_rx) = mpsc::channel();
        let thread = spawn_http_server(
            binding.listener,
            FrameStore::default(),
            controls,
            shutdown.clone(),
            fatal_tx,
        );

        assert!(post(address, "/input", b"broken").starts_with(b"HTTP/1.1 400 Bad Request"));
        assert!(
            post(
                address,
                "/input",
                &[b'x'; protocol::BROWSER_INPUT_MAX_BYTES + 1]
            )
            .starts_with(b"HTTP/1.1 400 Bad Request")
        );
        assert!(
            post(address, "/input", b"viewer-1 down KeyQ").starts_with(b"HTTP/1.1 204 No Content")
        );

        shutdown.store(true, Ordering::Release);
        thread.join().expect("HTTP thread should stop");
    }

    #[test]
    fn disconnect_and_expired_controller_lease_release_all_keys() {
        let (controls, mut game) = connected_controls();
        assert_eq!(
            controls.apply(BrowserInput::Heartbeat {
                client: "viewer-1".into()
            }),
            InputResult::Accepted
        );
        thread::sleep(INPUT_LEASE_TIMEOUT + Duration::from_millis(20));
        controls.expire_lease();
        assert_eq!(read_control(&mut game), ControlMessage::ReleaseAll);

        assert_eq!(
            controls.apply(BrowserInput::Heartbeat {
                client: "viewer-2".into()
            }),
            InputResult::Accepted
        );
        controls.release_and_disconnect();
        assert_eq!(read_control(&mut game), ControlMessage::ReleaseAll);
        assert_eq!(
            controls.apply(BrowserInput::Heartbeat {
                client: "viewer-2".into()
            }),
            InputResult::GameUnavailable
        );
    }

    #[test]
    fn advertised_dimensions_match_protocol() {
        assert_eq!(FRAME_WIDTH, 320);
        assert_eq!(FRAME_HEIGHT, 180);
    }

    #[test]
    fn falls_back_safely_when_default_port_is_busy() {
        let blocker = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("blocker should bind");
        let occupied = blocker.local_addr().expect("address should exist").port();
        let binding = bind_http(IpAddr::V4(Ipv4Addr::LOCALHOST), occupied, true)
            .expect("fallback should bind");
        assert!(binding.used_fallback_port);
        assert_ne!(binding.address.port(), occupied);

        let error = bind_http(IpAddr::V4(Ipv4Addr::LOCALHOST), occupied, false)
            .expect_err("explicit conflict should fail");
        assert!(error.contains("could not bind development viewer"));
    }
}
