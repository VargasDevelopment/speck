use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::protocol::{self, Frame};

const VIEWER_HTML: &[u8] = include_bytes!("viewer.html");

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
    shutdown: Arc<AtomicBool>,
    fatal: Sender<String>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        while !shutdown.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let frames = frames.clone();
                    thread::spawn(move || {
                        if let Err(error) = handle_http(stream, &frames) {
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

        if let Err(error) = stream.set_read_timeout(Some(Duration::from_secs(2))) {
            let _ = fatal.send(format!("could not configure frame stream timeout: {error}"));
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
        frames.stop();
    })
}

fn handle_http(mut stream: TcpStream, frames: &FrameStore) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| format!("could not configure HTTP request timeout: {error}"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(|error| format!("could not configure HTTP response timeout: {error}"))?;
    let path = read_request_path(&mut stream)?;

    if path == "/" {
        return respond(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            &[("Cache-Control", "no-store")],
            VIEWER_HTML,
        );
    }
    if path == "/favicon.ico" {
        return respond(&mut stream, "204 No Content", "text/plain", &[], &[]);
    }
    if let Some(query) = path.strip_prefix("/frame") {
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

    respond(
        &mut stream,
        "404 Not Found",
        "text/plain",
        &[],
        b"not found\n",
    )
}

fn read_request_path(stream: &mut TcpStream) -> Result<String, String> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while request.len() < 8192 && !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream
            .read(&mut buffer)
            .map_err(|error| format!("could not read HTTP request: {error}"))?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    if request.len() >= 8192 {
        return Err("HTTP request headers exceeded 8192 bytes".into());
    }
    let request = std::str::from_utf8(&request)
        .map_err(|_| "HTTP request headers were not UTF-8".to_owned())?;
    let mut parts = request
        .lines()
        .next()
        .ok_or_else(|| "HTTP request was empty".to_owned())?
        .split_whitespace();
    if parts.next() != Some("GET") {
        return Err("development viewer only accepts GET requests".into());
    }
    parts
        .next()
        .map(str::to_owned)
        .ok_or_else(|| "HTTP request did not include a path".into())
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
    use crate::dev::protocol::{FRAME_HEIGHT, FRAME_PAYLOAD_BYTES, FRAME_WIDTH};

    #[test]
    fn serves_viewer_and_complete_binary_frame() {
        let binding = bind_http(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, false)
            .expect("HTTP listener should bind");
        let address = binding.address;
        let frames = FrameStore::default();
        let shutdown = Arc::new(AtomicBool::new(false));
        let (fatal_tx, _fatal_rx) = mpsc::channel();
        let thread =
            spawn_http_server(binding.listener, frames.clone(), shutdown.clone(), fatal_tx);

        let page = get(address, "/");
        assert!(page.starts_with(b"HTTP/1.1 200 OK"));
        assert!(page.windows(7).any(|window| window == b"<canvas"));

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

    fn get(address: SocketAddr, path: &str) -> Vec<u8> {
        let mut stream = TcpStream::connect(address).expect("test should connect");
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        )
        .expect("request should write");
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .expect("response should read");
        response
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
