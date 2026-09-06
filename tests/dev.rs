pub mod support;

use std::fs;
use std::io::Write;
use std::net::{Shutdown, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use speck::dev::{protocol, server};

#[test]
fn shutdown_drains_queued_frames_and_preserves_transport_errors() {
    let encoded = protocol::encode_frame(1, &vec![0; protocol::FRAME_PAYLOAD_BYTES])
        .expect("frame should encode");
    for bytes in [encoded.as_slice(), &encoded[..10]] {
        let listener = server::bind_frame_listener().expect("frame listener should bind");
        let mut game = TcpStream::connect(listener.local_addr().unwrap())
            .expect("game should connect before the receiver starts");
        game.set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        // The connection is already queued when the host observes child exit.
        let shutdown = Arc::new(AtomicBool::new(true));
        let frames = server::FrameStore::default();
        let (fatal_tx, fatal_rx) = mpsc::channel();
        let receiver = server::spawn_frame_receiver(
            listener,
            frames.clone(),
            server::InputControl::default(),
            shutdown,
            fatal_tx,
        );
        game.write_all(bytes).expect("queued stream should drain");
        game.shutdown(Shutdown::Write).unwrap();
        receiver.join().expect("receiver should finish");
        if bytes.len() == encoded.len() {
            assert_eq!(frames.latest_sequence(), Some(1));
            assert!(fatal_rx.try_recv().is_err());
        } else {
            assert_eq!(frames.latest_sequence(), None);
            assert!(
                fatal_rx
                    .try_recv()
                    .expect("truncation should fail")
                    .contains("truncated frame header")
            );
        }
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
