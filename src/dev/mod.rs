pub mod protocol;
pub mod server;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use crate::toolchain::{self, BuildEnvironment};

#[derive(Clone, Debug)]
pub struct Options {
    pub bind: IpAddr,
    pub port: u16,
    pub port_explicit: bool,
    pub frame_limit: Option<u32>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8787,
            port_explicit: false,
            frame_limit: None,
        }
    }
}

pub fn run(
    source_path: &Path,
    llvm_ir: &str,
    environment: &BuildEnvironment,
    options: &Options,
) -> Result<(), String> {
    let artifacts = toolchain::build_for_development(source_path, llvm_ir, environment)?;
    let http = server::bind_http(options.bind, options.port, !options.port_explicit)?;
    let frame_listener = server::bind_frame_listener()?;
    let frame_port = frame_listener
        .local_addr()
        .map_err(|error| format!("could not inspect frame receiver: {error}"))?
        .port();

    let frames = server::FrameStore::default();
    let controls = server::InputControl::default();
    let shutdown = Arc::new(AtomicBool::new(false));
    let interrupted = Arc::new(AtomicBool::new(false));
    let (fatal_tx, fatal_rx) = mpsc::channel();
    let http_thread = server::spawn_http_server(
        http.listener,
        frames.clone(),
        controls.clone(),
        shutdown.clone(),
        fatal_tx.clone(),
    );
    let frame_thread = server::spawn_frame_receiver(
        frame_listener,
        frames.clone(),
        controls.clone(),
        shutdown.clone(),
        fatal_tx,
    );
    let input_thread = server::spawn_input_watchdog(controls, shutdown.clone());

    let interrupt_shutdown = shutdown.clone();
    let interrupt_flag = interrupted.clone();
    if let Err(error) = ctrlc::try_set_handler(move || {
        interrupt_flag.store(true, Ordering::Release);
        interrupt_shutdown.store(true, Ordering::Release);
    }) {
        shutdown.store(true, Ordering::Release);
        frames.stop();
        let _ = http_thread.join();
        let _ = frame_thread.join();
        let _ = input_thread.join();
        return Err(format!("could not install Ctrl-C handler: {error}"));
    }

    let mut command = Command::new(&artifacts.executable);
    command
        .env("SPECK_FRAME_STREAM_PORT", frame_port.to_string())
        .env_remove("SPECK_FRAME_LIMIT")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(limit) = options.frame_limit {
        command.env("SPECK_FRAME_LIMIT", limit.to_string());
    }
    let child = command.spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            shutdown.store(true, Ordering::Release);
            frames.stop();
            let _ = http_thread.join();
            let _ = frame_thread.join();
            let _ = input_thread.join();
            return Err(format!(
                "could not launch development game `{}`: {error}",
                artifacts.executable.display()
            ));
        }
    };

    if http.used_fallback_port {
        println!(
            "Port {} was unavailable; selected safe fallback port {}.",
            options.port,
            http.address.port()
        );
    }
    if !options.bind.is_loopback() {
        println!(
            "Warning: development viewer explicitly bound to non-loopback address {}.",
            options.bind
        );
    }
    println!("Development game: {}", artifacts.executable.display());
    match options.frame_limit {
        Some(limit) => println!("Frame limit: {limit}"),
        None => println!("Frame limit: unbounded (use Speck `quit()` or press Ctrl-C to stop)"),
    }
    println!("Viewer URL: {}", viewer_url(http.address));
    if options.bind.is_loopback() {
        println!(
            "Remote access: ssh -L {0}:localhost:{0} <anfibio-host>",
            http.address.port()
        );
        println!("Then open: http://localhost:{}/", http.address.port());
    }

    let mut fatal_error = None;
    let mut interrupt_deadline = None;
    let mut interrupt_error = None;
    let status = loop {
        if let Ok(error) = fatal_rx.try_recv() {
            fatal_error = Some(error);
            shutdown.store(true, Ordering::Release);
        }
        if interrupted.load(Ordering::Acquire) && interrupt_deadline.is_none() {
            match interrupt_child(&child) {
                Ok(()) => interrupt_deadline = Some(Instant::now() + Duration::from_secs(2)),
                Err(error) => {
                    interrupt_error = Some(error);
                    terminate(&mut child);
                }
            }
        } else if shutdown.load(Ordering::Acquire) && !interrupted.load(Ordering::Acquire) {
            terminate(&mut child);
        }
        match child
            .try_wait()
            .map_err(|error| format!("could not inspect development game: {error}"))?
        {
            Some(status) => break status,
            None => {
                if interrupt_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                    terminate(&mut child);
                    interrupt_error =
                        Some("development game did not stop within two seconds of Ctrl-C".into());
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
    };

    shutdown.store(true, Ordering::Release);
    frames.stop();
    http_thread
        .join()
        .map_err(|_| "development HTTP server thread panicked".to_owned())?;
    frame_thread
        .join()
        .map_err(|_| "frame receiver thread panicked".to_owned())?;
    input_thread
        .join()
        .map_err(|_| "input watchdog thread panicked".to_owned())?;

    let frames_received = frames.latest_sequence().unwrap_or(0);

    if interrupted.load(Ordering::Acquire) {
        if let Some(error) = interrupt_error {
            return Err(error);
        }
        println!("Development game stopped cleanly after Ctrl-C.");
        return Ok(());
    }
    if let Some(error) = fatal_error.or_else(|| fatal_rx.try_recv().ok()) {
        return Err(error);
    }
    if !status.success() {
        return Err(format!("development game exited with {status}"));
    }
    if let Some(limit) = options.frame_limit
        && frames_received != u64::from(limit)
    {
        return Err(format!(
            "development game completed, but the server received {frames_received} of {limit} frames"
        ));
    }
    println!("Frames received: {frames_received}");
    println!("Development game stopped cleanly after streaming its final frame.");
    Ok(())
}

fn terminate(child: &mut Child) {
    if matches!(child.try_wait(), Ok(None)) {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn interrupt_child(child: &Child) -> Result<(), String> {
    type CInt = std::ffi::c_int;
    const SIGINT: CInt = 2;

    unsafe extern "C" {
        fn kill(process: CInt, signal: CInt) -> CInt;
    }

    let process = CInt::try_from(child.id())
        .map_err(|_| "development game process identifier does not fit the host ABI".to_owned())?;
    // SAFETY: the PID belongs to the live child returned by `Command`, and SIGINT is accepted by
    // POSIX `kill` on both supported Speck hosts.
    if unsafe { kill(process, SIGINT) } == 0 {
        Ok(())
    } else {
        Err(format!(
            "could not forward Ctrl-C to development game: {}",
            std::io::Error::last_os_error()
        ))
    }
}

fn viewer_url(address: SocketAddr) -> String {
    let host = match address.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => "localhost".into(),
        IpAddr::V6(ip) if ip.is_unspecified() => "localhost".into(),
        IpAddr::V4(ip) => ip.to_string(),
        IpAddr::V6(ip) => format!("[{ip}]"),
    };
    format!("http://{host}:{}/", address.port())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_defaults_are_loopback_only_and_unbounded() {
        let options = Options::default();
        assert_eq!(options.bind, IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(options.port, 8787);
        assert!(!options.port_explicit);
        assert_eq!(options.frame_limit, None);
    }

    #[test]
    fn formats_ipv4_and_ipv6_viewer_urls() {
        assert_eq!(
            viewer_url("127.0.0.1:8787".parse().expect("address should parse")),
            "http://127.0.0.1:8787/"
        );
        assert_eq!(
            viewer_url("[::1]:8787".parse().expect("address should parse")),
            "http://[::1]:8787/"
        );
    }
}
