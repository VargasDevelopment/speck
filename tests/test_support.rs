pub mod support;

use std::fs;
use std::process::Command;
use std::time::Duration;

#[test]
fn independent_verifier_accepts_valid_ir_and_rejects_invalid_dominance() {
    let directory = support::workspace();
    let path = directory.path().join("game.ll");
    fs::write(&path, "define i32 @main() { ret i32 0 }").unwrap();
    support::assert_success(
        "valid LLVM IR",
        &support::verify_ir(&path, &directory.path().join("valid.bc")),
    );
    fs::write(&path, "define i32 @main() { entry: br i1 true, label %left, label %right\nleft: %x = add i32 1, 2\nbr label %end\nright: br label %end\nend: ret i32 %x\n}").unwrap();
    let output = support::verify_ir(&path, &directory.path().join("invalid.bc"));
    assert!(
        !output.status.success(),
        "invalid dominance must fail LLVM verification"
    );
}

#[test]
fn subprocess_capture_drains_both_pipes_and_preserves_failure_status() {
    let output = support::run(Command::new("sh").args(["-c", "dd if=/dev/zero bs=65536 count=32 2>/dev/null; dd if=/dev/zero bs=65536 count=32 >&2 2>/dev/null; exit 17"]));
    assert_eq!(output.status.code(), Some(17));
    assert_eq!(output.stdout.len(), 1024 * 1024);
    assert_eq!(output.stderr.len(), 1024 * 1024);
}

#[test]
fn subprocess_deadline_stops_descendants() {
    let directory = support::workspace();
    let marker = directory.path().join("orphan");
    let result = std::panic::catch_unwind(|| {
        support::run_with_timeout(
            Command::new("sh")
                .args(["-c", "(sleep 1; printf orphan > \"$1\") & wait", "sh"])
                .arg(&marker),
            Duration::from_millis(100),
        );
    });
    let error = result.expect_err("the command should exceed its deadline");
    assert!(
        error
            .downcast_ref::<String>()
            .is_some_and(|message| message.contains("timed out"))
    );
    std::thread::sleep(Duration::from_millis(1200));
    assert!(!marker.exists(), "a timed-out descendant kept running");
}
