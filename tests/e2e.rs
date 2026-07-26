use std::path::Path;
use std::process::Command;

#[test]
fn builds_and_executes_crumb_bum() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let compiler = env!("CARGO_BIN_EXE_speck");
    let build = Command::new(compiler)
        .current_dir(root)
        .args(["build", "examples/crumb_bum.spk"])
        .output()
        .expect("Speck compiler should start");
    assert!(
        build.status.success(),
        "build failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    let build_stdout = String::from_utf8_lossy(&build.stdout);
    assert!(build_stdout.contains("LLVM IR: build/crumb_bum.ll"));
    assert!(build_stdout.contains("Size: "));
    assert!(
        root.join("build/crumb_bum")
            .metadata()
            .expect("output executable should exist")
            .len()
            > 0
    );

    let run = Command::new(root.join("build/crumb_bum"))
        .output()
        .expect("compiled game should start");
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
