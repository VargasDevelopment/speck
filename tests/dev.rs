use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn development_command_streams_frames_and_stops_with_game() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_speck"))
        .current_dir(root)
        .args([
            "dev",
            "--frames",
            "3",
            "--port",
            "0",
            "examples/moving_rectangle.spk",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("development command should start");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child
            .try_wait()
            .expect("development command status should be readable")
            .is_some()
        {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("development command did not stop after its frame limit");
        }
        thread::sleep(Duration::from_millis(25));
    }

    let output = child
        .wait_with_output()
        .expect("development command output should be collected");
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
    assert!(root.join("build/moving_rectangle_dev").is_file());
}
