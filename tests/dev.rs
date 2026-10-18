pub mod support;

use std::fs;
use std::io::Write;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering::Release};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use speck::dev::{protocol, server};
use speck::resolution::Resolution;

#[test]
fn reaped_games_drain_queued_frames_and_preserve_transport_errors() {
    let encoded = frame(1);
    for bytes in [encoded.as_slice(), &encoded[..10]] {
        let listener = server::bind_frame_listener().expect("frame listener should bind");
        let mut game = TcpStream::connect(listener.local_addr().unwrap())
            .expect("game should connect before the receiver starts");
        game.set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        // The connection and its final bytes are already queued when the host reaps the child.
        game.write_all(bytes).expect("queued stream should buffer");
        game.shutdown(Shutdown::Write).unwrap();
        let (latest, error) = receive(listener, &Arc::new(AtomicBool::new(true))).finish();
        if bytes.len() == encoded.len() {
            assert_eq!((latest, error), (Some(1), None));
        } else {
            assert_eq!(latest, None);
            assert!(error.unwrap().contains("truncated frame header"));
        }
    }
}

#[test]
fn paused_games_keep_streaming_after_silence_between_or_within_frames() {
    // A suspended, debugged, or CPU-starved game can be silent for seconds. Liveness belongs
    // to the game process; this pause outlasts the two-second read timeout that once failed it.
    const PAUSE: Duration = Duration::from_millis(2_500);
    let first = frame(1);
    let stream = [first.clone(), frame(2)].concat();
    let pauses = [
        ("between frames", first.len()),
        ("inside a header", first.len() + 10),
        (
            "inside a payload",
            first.len() + protocol::FRAME_HEADER_BYTES + 100,
        ),
    ];
    let runs = pauses.map(|(place, split)| {
        let stream = stream.clone();
        thread::spawn(move || {
            let listener = server::bind_frame_listener().expect("frame listener should bind");
            let mut game =
                TcpStream::connect(listener.local_addr().unwrap()).expect("game should connect");
            let receiver = receive(listener, &Arc::new(AtomicBool::new(false)));
            game.write_all(&stream[..split]).unwrap();
            thread::sleep(PAUSE);
            // Report the receiver's verdict rather than the reset it causes after rejecting.
            let _ = game.write_all(&stream[split..]);
            let _ = game.shutdown(Shutdown::Write);
            assert_eq!(receiver.finish(), (Some(2), None), "pause {place}");
        })
    });
    for run in runs {
        run.join().expect("paused stream scenario should pass");
    }
}

#[test]
fn reaped_games_cannot_leave_shutdown_waiting_on_a_foreign_connection() {
    // Any local process can connect to the loopback frame port before the game does. Once the
    // game is reaped, that silent connection must not hold the receiver open forever, including
    // when reaping happens while the receiver is already waiting on it.
    for reaped_before_receiving in [true, false] {
        let listener = server::bind_frame_listener().expect("frame listener should bind");
        let address = listener.local_addr().unwrap();
        let _intruder = TcpStream::connect(address).expect("intruder should connect first");
        let mut game = TcpStream::connect(address).expect("game should connect second");
        let reaped = Arc::new(AtomicBool::new(reaped_before_receiving));
        let receiver = receive(listener, &reaped);
        game.write_all(&frame(1)).unwrap();
        drop(game);
        if !reaped_before_receiving {
            thread::sleep(Duration::from_millis(500));
            assert!(
                !receiver.thread.is_finished(),
                "a live game's silence must be tolerated"
            );
            reaped.store(true, Release);
        }
        let (latest, error) = receiver.finish();
        assert_eq!(latest, None);
        let error = error.expect("the open foreign connection should be reported");
        assert!(
            error.contains("stayed open after the development game exited"),
            "{error}"
        );
    }
}

/// A small frame that fits in the socket buffers of a connection that is not yet accepted.
fn frame(sequence: u64) -> Vec<u8> {
    let resolution = Resolution::new(8, 8).unwrap();
    protocol::encode_frame_at_resolution(sequence, resolution, &[0; 8 * 8 * 3]).unwrap()
}

struct Receiver {
    frames: server::FrameStore,
    fatal: mpsc::Receiver<String>,
    thread: JoinHandle<()>,
}

fn receive(listener: TcpListener, reaped: &Arc<AtomicBool>) -> Receiver {
    let frames = server::FrameStore::default();
    let (fatal_tx, fatal) = mpsc::channel();
    let thread = server::spawn_frame_receiver(
        listener,
        frames.clone(),
        server::InputControl::default(),
        reaped.clone(),
        fatal_tx,
    );
    Receiver {
        frames,
        fatal,
        thread,
    }
}

impl Receiver {
    /// The latest frame and any fatal error; fails instead of hanging if the receiver blocks.
    fn finish(self) -> (Option<u64>, Option<String>) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !self.thread.is_finished() {
            assert!(Instant::now() < deadline, "frame receiver did not finish");
            thread::sleep(Duration::from_millis(10));
        }
        self.thread.join().expect("receiver should not panic");
        (self.frames.latest_sequence(), self.fatal.try_recv().ok())
    }
}

#[test]
fn development_command_streams_frames_and_stops_with_game() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directory = support::workspace();
    let work = directory.path();
    let output = support::run_with_timeout(
        Command::new(env!("CARGO_BIN_EXE_speck"))
            .current_dir(work)
            .args(["dev", "--frames", "3", "--port", "0"])
            .arg(root.join("examples/moving_rectangle.spk")),
        Duration::from_secs(10),
    );
    assert!(
        output.status.success(),
        "development command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Viewer URL: http://127.0.0.1:"));
    assert!(stdout.contains("Frames received: 3"));
    assert!(stdout.contains("stopped cleanly"));
    assert!(work.join("build/moving_rectangle_dev").is_file());
}

#[test]
fn development_games_can_quit_before_the_frame_limit() {
    let directory = support::workspace();
    let work = directory.path();
    let source = work.join("quit.spk");
    for (phase, expected_frames) in [("start", 0), ("update", 1), ("draw", 1)] {
        for limit in [None, Some("3")] {
            let body = |name| if phase == name { "quit()" } else { "" };
            fs::write(
                &source,
                format!(
                    "game \"Quit\"\nstart {{ {} }}\nupdate(dt: f32) {{ {} }}\ndraw {{ {} }}\n",
                    body("start"),
                    body("update"),
                    body("draw")
                ),
            )
            .expect("test source should write");
            let mut command = Command::new(env!("CARGO_BIN_EXE_speck"));
            command
                .current_dir(work)
                .args(["dev", "--port", "0"])
                .arg(&source);
            if let Some(limit) = limit {
                command.args(["--frames", limit]);
            }
            let output = support::run_with_timeout(&mut command, Duration::from_secs(10));
            assert!(
                output.status.success(),
                "quit from {phase} with limit {limit:?} failed\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(stdout.contains(&format!("Frame limit: {}", limit.unwrap_or("unbounded"))));
            assert!(
                stdout.contains(&format!("Frames received: {expected_frames}")),
                "quit from {phase} with limit {limit:?}: {stdout}"
            );
            assert!(stdout.contains("stopped cleanly"));
        }
    }
}
