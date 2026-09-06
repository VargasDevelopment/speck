#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

pub mod support;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

#[test]
fn native_command_builds_runs_and_stops_after_its_frame_limit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directory = support::workspace();
    let work = directory.path();
    let output = support::run_with_timeout(
        Command::new(env!("CARGO_BIN_EXE_speck"))
            .current_dir(work)
            .args(["run", "--frames", "3"])
            .arg(root.join("examples/moving_rectangle.spk")),
        Duration::from_secs(15),
    );
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
    assert!(work.join("build/moving_rectangle_native").is_file());
}
