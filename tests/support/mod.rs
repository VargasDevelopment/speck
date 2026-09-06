use std::io::{self, Read};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{OnceLock, mpsc};
use std::time::{Duration, Instant};

use speck::toolchain::{BuildEnvironment, HostTarget};
use tempfile::TempDir;

pub fn workspace() -> TempDir {
    tempfile::Builder::new()
        .prefix("speck-test-")
        .tempdir()
        .expect("isolated test directory")
}

pub fn environment() -> &'static BuildEnvironment {
    static ENVIRONMENT: OnceLock<BuildEnvironment> = OnceLock::new();
    ENVIRONMENT.get_or_init(|| {
        BuildEnvironment::discover(HostTarget::detect().expect("supported test host"))
            .expect("native test tools should be discoverable; see docs/development-environment.md")
    })
}

pub fn run(command: &mut Command) -> Output {
    run_with_timeout(command, Duration::from_secs(30))
}

pub fn run_with_timeout(command: &mut Command, timeout: Duration) -> Output {
    let child = command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("could not start {command:?}: {error}"));
    let mut process = ProcessGroup {
        child,
        complete: false,
    };
    let (send, receive) = mpsc::channel();
    let stdout = process.child.stdout.take().unwrap();
    let stderr = process.child.stderr.take().unwrap();
    let out_send = send.clone();
    std::thread::spawn(move || {
        let _ = out_send.send((true, capture(stdout)));
    });
    std::thread::spawn(move || {
        let _ = send.send((false, capture(stderr)));
    });
    let deadline = Instant::now() + timeout;
    let mut status = None;
    let mut stdout = None;
    let mut stderr = None;
    loop {
        for (is_stdout, bytes) in receive.try_iter() {
            let bytes = bytes.expect("subprocess output should be readable");
            if is_stdout {
                stdout = Some(bytes);
            } else {
                stderr = Some(bytes);
            }
        }
        if status.is_none() {
            status = process.child.try_wait().expect("subprocess status");
        }
        if status.is_some() && stdout.is_some() && stderr.is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "subprocess timed out after {timeout:?}: {command:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    process.complete = true;
    Output {
        status: status.unwrap(),
        stdout: stdout.unwrap(),
        stderr: stderr.unwrap(),
    }
}

fn capture(mut reader: impl Read) -> io::Result<Vec<u8>> {
    // Continue draining after the capture limit, so noisy failures neither fill
    // a pipe nor exhaust test-runner memory before the deadline can fire.
    let mut bytes = Vec::new();
    reader.by_ref().take(1024 * 1024).read_to_end(&mut bytes)?;
    io::copy(&mut reader, &mut io::sink())?;
    Ok(bytes)
}

struct ProcessGroup {
    child: Child,
    complete: bool,
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if !self.complete {
            unsafe extern "C" {
                fn kill(pid: std::ffi::c_int, signal: std::ffi::c_int) -> std::ffi::c_int;
            }
            let pid = i32::try_from(self.child.id()).expect("POSIX child PID");
            // SAFETY: process_group(0) created a private group led by this child.
            // Negative PID addresses that group, including compiler/game children.
            unsafe {
                kill(-pid, 9);
            }
            let _ = self.child.wait();
        }
    }
}

pub fn assert_success(description: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{description} failed ({})\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn build_in(work: &Path, source: &Path) -> PathBuf {
    let output = run(Command::new(env!("CARGO_BIN_EXE_speck"))
        .current_dir(work)
        .arg("build")
        .arg(source));
    assert_success("Speck build", &output);
    work.join("build")
        .join(source.file_stem().expect("source file stem"))
}

pub fn verify_ir(source: &Path, bitcode: &Path) -> Output {
    // Invoke LLVM's verifier independently of Speck's emitter/build result,
    // using the same discovered Clang available to normal game builds.
    run(environment()
        .clang_command()
        .current_dir(source.parent().expect("IR source directory"))
        .args([
            "-x",
            "ir",
            "-c",
            "-emit-llvm",
            "-Xclang",
            "-llvm-verify-each",
        ])
        .arg(source)
        .arg("-o")
        .arg(bitcode))
}
