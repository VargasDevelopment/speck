pub mod support;

use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn builds_and_executes_crumb_bum() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directory = support::workspace();
    let work = directory.path();
    let compiler = env!("CARGO_BIN_EXE_speck");
    let build = support::run(
        Command::new(compiler)
            .current_dir(work)
            .arg("build")
            .arg(root.join("examples/crumb_bum.spk")),
    );
    assert!(
        build.status.success(),
        "build failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    let build_stdout = String::from_utf8_lossy(&build.stdout);
    assert!(build_stdout.contains("LLVM IR: build/crumb_bum.ll"));
    assert!(build_stdout.contains("Host target: "));
    assert!(build_stdout.contains("LLVM target: "));
    assert!(build_stdout.contains("LLVM validation: "));
    assert!(build_stdout.contains("Size: "));
    assert!(
        work.join("build/crumb_bum")
            .metadata()
            .expect("output executable should exist")
            .len()
            > 0
    );
    let executable =
        fs::read(work.join("build/crumb_bum")).expect("normal game executable should be readable");
    assert!(
        !executable
            .windows(b"SPECK_FRAME_STREAM_PORT".len())
            .any(|window| window == b"SPECK_FRAME_STREAM_PORT"),
        "normal game binary must not contain development transport code"
    );

    let run = support::run(Command::new(work.join("build/crumb_bum")).current_dir(work));
    assert!(
        run.status.success(),
        "game failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "1440\n2\n1\nframe 1: 40.000\nframe 2: 70.000\nframe 3: 100.000\nframe 4: 130.000\nframe 5: 100.000\n"
    );
}
