use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[test]
fn installed_compiler_builds_without_its_source_tree() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = root.join(format!("target/installation-{}", std::process::id()));
    let source = work.join("source");
    fs::create_dir_all(&source).unwrap();
    for name in ["Cargo.toml", "Cargo.lock"] {
        fs::copy(root.join(name), source.join(name)).unwrap();
    }
    for name in ["src", "runtime"] {
        copy_directory(&root.join(name), &source.join(name));
    }
    let target = work.join("compiler-build");
    // This test executes the resulting compiler, so build explicitly for the
    // Rust host even when Cargo has a configured default build target.
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let version = run(
        Command::new(rustc).arg("-vV"),
        &work,
        Duration::from_secs(5),
    );
    let version = String::from_utf8(version.stdout).unwrap();
    let host = version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .expect("rustc should identify its host target");
    run(
        Command::new(env!("CARGO"))
            .current_dir(&source)
            .args([
                "build",
                "--locked",
                "--offline",
                "--bin",
                "speck",
                "--target-dir",
            ])
            .arg(&target)
            .args(["--target", host]),
        &work,
        Duration::from_secs(120),
    );
    let installed = work.join("speck");
    fs::copy(target.join(host).join("debug/speck"), &installed).unwrap();
    fs::remove_dir_all(&source).unwrap();
    fs::remove_dir_all(&target).unwrap();

    // Only the compiler binary and the game remain. Its compile-time source
    // location no longer exists; the user's actual checkout was never moved.
    fs::write(
        work.join("game.spk"),
        "game \"Installed\"\nstart { print_i32(42) }\nupdate(dt: f32) {}\ndraw {}\n",
    )
    .unwrap();
    run(
        Command::new(&installed)
            .current_dir(&work)
            .args(["build", "game.spk"]),
        &work,
        Duration::from_secs(15),
    );
    let output = run(
        Command::new(work.join("build/game")).current_dir(&work),
        &work,
        Duration::from_secs(5),
    );
    assert_eq!(output.stdout, b"42\n");
    let output = run(
        Command::new(&installed)
            .current_dir(&work)
            .args(["dev", "--frames", "1", "--port", "0", "game.spk"]),
        &work,
        Duration::from_secs(15),
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Frames received: 1"));
    assert!(fs::read_dir(work.join("build")).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".crumb-sources-")
    }));
    fs::remove_dir_all(&work).unwrap();
}

fn copy_directory(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn run(command: &mut Command, work: &Path, timeout: Duration) -> Output {
    static NEXT_LOG: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_LOG.fetch_add(1, Ordering::Relaxed);
    let stdout = work.join(format!("stdout-{id}.log"));
    let stderr = work.join(format!("stderr-{id}.log"));
    let mut child = command
        .stdout(Stdio::from(fs::File::create(&stdout).unwrap()))
        .stderr(Stdio::from(fs::File::create(&stderr).unwrap()))
        .spawn()
        .expect("installation command should start");
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("installation command timed out: {command:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let output = Output {
        status,
        stdout: fs::read(stdout).unwrap(),
        stderr: fs::read(stderr).unwrap(),
    };
    assert!(
        status.success(),
        "{command:?} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
