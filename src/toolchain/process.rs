//! A cancellable native build command owns and reaps its process group.
use std::ffi::OsString;
use std::io::{self, Read};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

struct OwnedCommand(Child);

impl Drop for OwnedCommand {
    fn drop(&mut self) {
        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        // Each build command starts a fresh group, including compiler descendants.
        if let Ok(pid) = i32::try_from(self.0.id()) {
            // SAFETY: the negative PID names the group created for this owned command.
            unsafe {
                kill(-pid, 9);
            }
        }
        let _ = self.0.wait();
    }
}

pub(super) fn output(
    program: &Path,
    args: &[OsString],
    cancelled: &AtomicBool,
) -> Result<Output, String> {
    if cancelled.load(Ordering::Acquire) {
        return Err("development build cancelled".into());
    }
    let mut child = OwnedCommand(
        Command::new(program)
            .args(args)
            .process_group(0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                format!(
                    "could not run required tool `{}`: {error}",
                    program.display()
                )
            })?,
    );
    let stdout = child.0.stdout.take().unwrap();
    let stderr = child.0.stderr.take().unwrap();
    let stdout = capture(stdout);
    let stderr = capture(stderr);
    let result = loop {
        if cancelled.load(Ordering::Acquire) {
            break Err("development build cancelled".to_owned());
        }
        match child.0.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => break Err(format!("could not inspect native build command: {error}")),
        }
    };
    // Close descendant-owned pipes before joining readers, including cancellation/error paths.
    drop(child);
    let stdout = stdout.join();
    let stderr = stderr.join();
    let stdout = stdout
        .map_err(|_| "native build stdout reader panicked")?
        .map_err(|error| format!("could not read native build stdout: {error}"))?;
    let stderr = stderr
        .map_err(|_| "native build stderr reader panicked")?
        .map_err(|error| format!("could not read native build stderr: {error}"))?;
    Ok(Output {
        status: result?,
        stdout,
        stderr,
    })
}

fn capture(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        pipe.read_to_end(&mut bytes).map(|_| bytes)
    })
}
