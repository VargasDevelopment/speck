use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

use super::*;
use crate::dev::protocol::{FRAME_HEIGHT, FRAME_PAYLOAD_BYTES, FRAME_WIDTH, Key, decode_control};

#[test]
fn serves_viewer_and_complete_binary_frame() {
    let binding =
        bind_http(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, false).expect("HTTP listener should bind");
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
    assert!(page_text.contains("const inputGeneration = generation"));
    assert!(page_text.contains("/input?generation=${inputGeneration}"));

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

    // A restarted native process begins at 1; this same HTTP client must advance past 9.
    frames.begin_run();
    frames.publish(Frame {
        sequence: 1,
        pixels: vec![91; FRAME_PAYLOAD_BYTES],
    });
    frames.set_state(ViewerState::Watching); // Keep a finite game's final frame available.
    let response = get(address, "/frame?after=9");
    let headers = String::from_utf8_lossy(
        &response[..response
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap()],
    );
    assert!(headers.contains("X-Speck-Sequence: 10"));
    assert!(headers.contains("X-Speck-Generation: 1"));
    assert_eq!(*response.last().unwrap(), 91);

    shutdown.store(true, Ordering::Release);
    thread.join().expect("HTTP thread should stop");
}

#[test]
fn accepted_http_connections_wait_for_delayed_requests() {
    let binding =
        bind_http(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, false).expect("HTTP listener should bind");
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
        .connect_game(host, 1)
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
    let binding =
        bind_http(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, false).expect("HTTP listener should bind");
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

    let response = post(address, "/input?generation=1", b"viewer-1 down ArrowLeft");
    assert!(response.starts_with(b"HTTP/1.1 204 No Content"));
    assert_eq!(
        read_control(&mut game),
        ControlMessage::Key {
            key: Key::Left,
            down: true
        }
    );

    let repeated = post(address, "/input?generation=1", b"viewer-1 down ArrowLeft");
    assert!(repeated.starts_with(b"HTTP/1.1 204 No Content"));
    assert_eq!(
        read_control(&mut game),
        ControlMessage::Key {
            key: Key::Left,
            down: true
        }
    );

    let busy = post(address, "/input?generation=1", b"viewer-2 down KeyD");
    assert!(busy.starts_with(b"HTTP/1.1 409 Conflict"));

    let release = post(address, "/input?generation=1", b"viewer-1 release -");
    assert!(release.starts_with(b"HTTP/1.1 204 No Content"));
    assert_eq!(read_control(&mut game), ControlMessage::ReleaseAll);

    let next = post(address, "/input?generation=1", b"viewer-2 down KeyD");
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
fn delayed_input_cannot_cross_a_game_restart_or_steal_its_controller() {
    let binding = bind_http(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, false).unwrap();
    let address = binding.address;
    let (controls, _old_game) = connected_controls();
    let shutdown = Arc::new(AtomicBool::new(false));
    let (fatal_tx, _fatal_rx) = mpsc::channel();
    let server = spawn_http_server(
        binding.listener,
        FrameStore::default(),
        controls.clone(),
        shutdown.clone(),
        fatal_tx,
    );

    // The old request has left the browser, but its body arrives after the new game connects.
    let body = b"old-viewer down ArrowLeft";
    let mut delayed = TcpStream::connect(address).unwrap();
    delayed
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write!(
        delayed,
        "POST /input?generation=1 HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .unwrap();
    controls.disconnect_game();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let mut game = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    game.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    controls
        .connect_game(listener.accept().unwrap().0, 2)
        .unwrap();
    delayed.write_all(body).unwrap();
    let mut response = Vec::new();
    delayed.read_to_end(&mut response).unwrap();
    assert!(response.starts_with(b"HTTP/1.1 412 Precondition Failed"));
    assert!(
        post(address, "/input?generation=1", b"old-viewer heartbeat -")
            .starts_with(b"HTTP/1.1 412")
    );
    assert!(
        post(address, "/input?generation=2", b"new-viewer down KeyD").starts_with(b"HTTP/1.1 204")
    );
    assert_eq!(
        read_control(&mut game),
        ControlMessage::Key {
            key: Key::D,
            down: true
        }
    );

    // A delayed release from the same browser also must not release the new run's lease or keys.
    assert!(
        post(address, "/input?generation=1", b"new-viewer release -").starts_with(b"HTTP/1.1 412")
    );
    assert!(
        post(address, "/input?generation=2", b"other-viewer down KeyA")
            .starts_with(b"HTTP/1.1 409")
    );
    assert!(
        post(address, "/input?generation=2", b"new-viewer up KeyD").starts_with(b"HTTP/1.1 204")
    );
    assert_eq!(
        read_control(&mut game),
        ControlMessage::Key {
            key: Key::D,
            down: false
        }
    );
    shutdown.store(true, Ordering::Release);
    server.join().unwrap();
}

#[test]
fn malformed_oversized_and_unsupported_browser_input_is_safe() {
    let binding =
        bind_http(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, false).expect("HTTP listener should bind");
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

    assert!(post(address, "/input", b"viewer-1 down KeyA").starts_with(b"HTTP/1.1 400"));
    assert!(
        post(address, "/input?generation=1", b"broken").starts_with(b"HTTP/1.1 400 Bad Request")
    );
    assert!(
        post(
            address,
            "/input?generation=1",
            &[b'x'; protocol::BROWSER_INPUT_MAX_BYTES + 1]
        )
        .starts_with(b"HTTP/1.1 400 Bad Request")
    );
    assert!(
        post(address, "/input?generation=1", b"viewer-1 down KeyQ")
            .starts_with(b"HTTP/1.1 204 No Content")
    );

    shutdown.store(true, Ordering::Release);
    thread.join().expect("HTTP thread should stop");
}

#[test]
fn disconnect_and_expired_controller_lease_release_all_keys() {
    let (controls, mut game) = connected_controls();
    assert_eq!(
        controls.apply(
            BrowserInput::Heartbeat {
                client: "viewer-1".into()
            },
            1
        ),
        InputResult::Accepted
    );
    thread::sleep(INPUT_LEASE_TIMEOUT + Duration::from_millis(20));
    controls.expire_lease();
    assert_eq!(read_control(&mut game), ControlMessage::ReleaseAll);

    assert_eq!(
        controls.apply(
            BrowserInput::Heartbeat {
                client: "viewer-2".into()
            },
            1
        ),
        InputResult::Accepted
    );
    controls.release_and_disconnect();
    assert_eq!(read_control(&mut game), ControlMessage::ReleaseAll);
    assert_eq!(
        controls.apply(
            BrowserInput::Heartbeat {
                client: "viewer-2".into()
            },
            1
        ),
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
    let binding =
        bind_http(IpAddr::V4(Ipv4Addr::LOCALHOST), occupied, true).expect("fallback should bind");
    assert!(binding.used_fallback_port);
    assert_ne!(binding.address.port(), occupied);

    let error = bind_http(IpAddr::V4(Ipv4Addr::LOCALHOST), occupied, false)
        .expect_err("explicit conflict should fail");
    assert!(error.contains("could not bind development viewer"));
}
