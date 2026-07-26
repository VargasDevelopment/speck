#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn native_command_builds_runs_and_stops_after_its_frame_limit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_speck"))
        .current_dir(root)
        .args(["run", "--frames", "3", "examples/moving_rectangle.spk"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("native command should start");

    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child
            .try_wait()
            .expect("native command status should be readable")
            .is_some()
        {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("native command did not stop after its frame limit");
        }
        thread::sleep(Duration::from_millis(25));
    }

    let output = child
        .wait_with_output()
        .expect("native command output should be collected");
    assert!(
        output.status.success(),
        "native command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Native game: build/moving_rectangle_native"));
    assert!(stdout.contains("Frame limit: 3"));
    assert!(stdout.contains("stopped cleanly after 3 frames"));
    assert!(root.join("build/moving_rectangle_native").is_file());
}
