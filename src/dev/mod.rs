pub mod protocol;
pub mod server;
mod session;
mod watch;
pub use watch::run as watch;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::process::Child;
use std::thread;
use std::time::Duration;

use crate::toolchain::{self, BuildEnvironment};

#[derive(Clone, Debug)]
pub struct Options {
    pub bind: IpAddr,
    pub port: u16,
    pub port_explicit: bool,
    pub frame_limit: Option<u32>,
    pub watch: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8787,
            port_explicit: false,
            frame_limit: None,
            watch: false,
        }
    }
}

pub fn run(
    source_path: &Path,
    llvm_ir: &str,
    environment: &BuildEnvironment,
    options: &Options,
) -> Result<(), String> {
    let mut session = session::Session::start(options)?;
    let environment = environment.with_cancellation(session.cancelled.clone());
    let result = (|| {
        let artifacts = toolchain::build_for_development(source_path, llvm_ir, &environment)?;
        if session.is_cancelled() {
            return Ok(());
        }
        session.launch(&artifacts.executable, options)?;
        loop {
            session.check_server()?;
            if session.is_cancelled() {
                break;
            }
            if let Some(status) = session.poll_game()? {
                session.stop_game()?;
                if !status.success() {
                    return Err(format!("development game exited with {status}"));
                }
                println!(
                    "Frames received: {}",
                    session.frames.latest_sequence().unwrap_or(0)
                );
                println!("Development game stopped cleanly after streaming its final frame.");
                return Ok(());
            }
            thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    })();
    session.stop_game()?;
    if session.is_interrupted() {
        println!("Development game stopped cleanly after Ctrl-C.");
        Ok(())
    } else {
        result
    }
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
