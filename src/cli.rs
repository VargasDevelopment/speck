use std::ffi::OsString;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::{compile_to_llvm_for_target, dev, render_diagnostics, toolchain};

pub fn run(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let args: Vec<OsString> = args.into_iter().collect();
    match args.as_slice() {
        [_program, command, source] if command == "build" => build(Path::new(source)),
        [_program, command, rest @ ..] if command == "dev" => development(rest),
        [_program, command, rest @ ..] if command == "run" => native(rest),
        [_program, flag] if flag == "--help" || flag == "-h" => {
            print_help();
            ExitCode::SUCCESS
        }
        [_program] => {
            print_help();
            ExitCode::from(2)
        }
        _ => {
            eprintln!(
                "error: expected `speck build <game.spk>`, `speck dev <game.spk>`, or `speck run \
                 <game.spk>`\n"
            );
            print_help();
            ExitCode::from(2)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct NativeOptions {
    frame_limit: Option<u32>,
}

fn native(args: &[OsString]) -> ExitCode {
    let (path, options) = match parse_native_args(args) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    if path.extension().and_then(|extension| extension.to_str()) != Some("spk") {
        eprintln!("error: Speck source files must use the `.spk` extension");
        return ExitCode::from(2);
    }
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("error: could not read `{}`: {error}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let host_target = match toolchain::HostTarget::detect() {
        Ok(target) => target,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    if host_target != toolchain::HostTarget::MacOsArm64 {
        eprintln!(
            "error: native Cocoa presentation requires macOS ARM64; detected {host_target}. Use \
             `speck build` for deterministic PPM output or `speck dev` for browser presentation on \
             this host"
        );
        return ExitCode::FAILURE;
    }
    let environment = match toolchain::BuildEnvironment::discover(host_target) {
        Ok(environment) => environment,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let llvm_ir = match compile_to_llvm_for_target(&source, environment.llvm_target_triple()) {
        Ok(llvm_ir) => llvm_ir,
        Err(diagnostics) => {
            eprintln!("{}", render_diagnostics(&path, &source, &diagnostics));
            return ExitCode::FAILURE;
        }
    };
    let artifacts = match toolchain::build_for_native(&path, &llvm_ir, &environment) {
        Ok(artifacts) => artifacts,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!(
        "Native game: {}",
        display_path(&artifacts.executable).display()
    );
    println!("Host target: {}", environment.target());
    println!("LLVM target: {}", environment.llvm_target_triple());
    println!("C/Objective-C compiler: {}", environment.clang().display());
    println!("Linker: {}", environment.linker().display());
    println!("LLVM validation: {}", artifacts.llvm_validation);
    println!("Size: {} bytes", artifacts.size);
    match options.frame_limit {
        Some(limit) => println!("Frame limit: {limit}"),
        None => println!("Frame limit: unbounded (close the window or press Ctrl-C to stop)"),
    }

    match launch_native(&artifacts.executable, options.frame_limit) {
        Ok(true) => {
            println!("Native game stopped cleanly after Ctrl-C.");
            ExitCode::SUCCESS
        }
        Ok(false) => {
            if let Some(limit) = options.frame_limit {
                println!("Native game stopped cleanly after {limit} frames.");
            } else {
                println!("Native game stopped cleanly.");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_native_args(args: &[OsString]) -> Result<(PathBuf, NativeOptions), String> {
    let mut options = NativeOptions::default();
    let mut source = None;
    let mut cursor = 0;
    while cursor < args.len() {
        let argument = args[cursor].to_string_lossy();
        match argument.as_ref() {
            "--frames" => {
                cursor += 1;
                let value = args
                    .get(cursor)
                    .ok_or_else(|| "`--frames` requires a positive integer".to_owned())?;
                let frame_limit = value.to_string_lossy().parse::<u32>().map_err(|_| {
                    format!("invalid `--frames` value `{}`", value.to_string_lossy())
                })?;
                if frame_limit == 0 {
                    return Err("`--frames` must be greater than zero".into());
                }
                options.frame_limit = Some(frame_limit);
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown native-run option `{value}`"));
            }
            _ if source.is_none() => source = Some(PathBuf::from(&args[cursor])),
            _ => return Err("native run accepts exactly one `.spk` source file".into()),
        }
        cursor += 1;
    }
    source
        .map(|source| (source, options))
        .ok_or_else(|| "expected `speck run <game.spk>`".into())
}

fn launch_native(executable: &Path, frame_limit: Option<u32>) -> Result<bool, String> {
    let interrupted = Arc::new(AtomicBool::new(false));
    let interrupt_flag = interrupted.clone();
    ctrlc::try_set_handler(move || interrupt_flag.store(true, Ordering::Release))
        .map_err(|error| format!("could not install Ctrl-C handler: {error}"))?;

    let mut command = Command::new(executable);
    command
        .env_remove("SPECK_FRAME_LIMIT")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(limit) = frame_limit {
        command.env("SPECK_FRAME_LIMIT", limit.to_string());
    }
    let mut child = command.spawn().map_err(|error| {
        format!(
            "could not launch native game `{}`: {error}",
            executable.display()
        )
    })?;
    let mut interrupt_deadline = None;

    let status = loop {
        if interrupted.load(Ordering::Acquire) && interrupt_deadline.is_none() {
            interrupt_child(&child)?;
            interrupt_deadline = Some(Instant::now() + Duration::from_secs(2));
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("could not inspect native game: {error}"))?
        {
            break status;
        }
        if interrupt_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            terminate_native(&mut child);
            return Err("native game did not stop within two seconds of Ctrl-C".into());
        }
        thread::sleep(Duration::from_millis(10));
    };

    if !status.success() {
        return Err(format!("native game exited with {status}"));
    }
    Ok(interrupted.load(Ordering::Acquire))
}

fn interrupt_child(child: &Child) -> Result<(), String> {
    type CInt = std::ffi::c_int;
    const SIGINT: CInt = 2;

    unsafe extern "C" {
        fn kill(process: CInt, signal: CInt) -> CInt;
    }

    let process = CInt::try_from(child.id())
        .map_err(|_| "native game process identifier does not fit the host ABI".to_owned())?;
    // SAFETY: `process` is the live child PID returned by `std::process::Child`, and SIGINT is a
    // signal number accepted by POSIX `kill` on every supported Speck host.
    if unsafe { kill(process, SIGINT) } == 0 {
        Ok(())
    } else {
        Err(format!(
            "could not forward Ctrl-C to native game: {}",
            std::io::Error::last_os_error()
        ))
    }
}

fn terminate_native(child: &mut Child) {
    if matches!(child.try_wait(), Ok(None)) {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn development(args: &[OsString]) -> ExitCode {
    let (path, options) = match parse_dev_args(args) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    if path.extension().and_then(|extension| extension.to_str()) != Some("spk") {
        eprintln!("error: Speck source files must use the `.spk` extension");
        return ExitCode::from(2);
    }
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("error: could not read `{}`: {error}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let host_target = match toolchain::HostTarget::detect() {
        Ok(target) => target,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let environment = match toolchain::BuildEnvironment::discover(host_target) {
        Ok(environment) => environment,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let llvm_ir = match compile_to_llvm_for_target(&source, environment.llvm_target_triple()) {
        Ok(llvm_ir) => llvm_ir,
        Err(diagnostics) => {
            eprintln!("{}", render_diagnostics(&path, &source, &diagnostics));
            return ExitCode::FAILURE;
        }
    };

    match dev::run(&path, &llvm_ir, &environment, &options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_dev_args(args: &[OsString]) -> Result<(PathBuf, dev::Options), String> {
    let mut options = dev::Options::default();
    let mut source = None;
    let mut cursor = 0;
    while cursor < args.len() {
        let argument = args[cursor].to_string_lossy();
        match argument.as_ref() {
            "--bind" => {
                cursor += 1;
                let value = args
                    .get(cursor)
                    .ok_or_else(|| "`--bind` requires an IP address".to_owned())?;
                options.bind = value.to_string_lossy().parse::<IpAddr>().map_err(|_| {
                    format!("invalid `--bind` IP address `{}`", value.to_string_lossy())
                })?;
            }
            "--port" => {
                cursor += 1;
                let value = args
                    .get(cursor)
                    .ok_or_else(|| "`--port` requires a number from 0 to 65535".to_owned())?;
                options.port = value
                    .to_string_lossy()
                    .parse::<u16>()
                    .map_err(|_| format!("invalid `--port` value `{}`", value.to_string_lossy()))?;
                options.port_explicit = true;
            }
            "--frames" => {
                cursor += 1;
                let value = args
                    .get(cursor)
                    .ok_or_else(|| "`--frames` requires a positive integer".to_owned())?;
                options.frame_limit = value.to_string_lossy().parse::<u32>().map_err(|_| {
                    format!("invalid `--frames` value `{}`", value.to_string_lossy())
                })?;
                if options.frame_limit == 0 {
                    return Err("`--frames` must be greater than zero".into());
                }
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown development option `{value}`"));
            }
            _ if source.is_none() => source = Some(PathBuf::from(&args[cursor])),
            _ => return Err("development mode accepts exactly one `.spk` source file".into()),
        }
        cursor += 1;
    }
    source
        .map(|source| (source, options))
        .ok_or_else(|| "expected `speck dev <game.spk>`".into())
}

fn build(path: &Path) -> ExitCode {
    if path.extension().and_then(|extension| extension.to_str()) != Some("spk") {
        eprintln!("error: Speck source files must use the `.spk` extension");
        return ExitCode::from(2);
    }
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("error: could not read `{}`: {error}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let host_target = match toolchain::HostTarget::detect() {
        Ok(target) => target,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let environment = match toolchain::BuildEnvironment::discover(host_target) {
        Ok(environment) => environment,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let llvm_ir = match compile_to_llvm_for_target(&source, environment.llvm_target_triple()) {
        Ok(llvm_ir) => llvm_ir,
        Err(diagnostics) => {
            eprintln!("{}", render_diagnostics(path, &source, &diagnostics));
            return ExitCode::FAILURE;
        }
    };
    match toolchain::build(path, &llvm_ir, &environment) {
        Ok(artifacts) => {
            println!("Built: {}", display_path(&artifacts.executable).display());
            println!("LLVM IR: {}", display_path(&artifacts.llvm_ir).display());
            println!(
                "LLVM bitcode: {}",
                display_path(&artifacts.llvm_bitcode).display()
            );
            println!("Host target: {}", environment.target());
            println!("LLVM target: {}", environment.llvm_target_triple());
            println!("C compiler: {}", environment.clang().display());
            println!("Linker: {}", environment.linker().display());
            println!("LLVM validation: {}", artifacts.llvm_validation);
            println!("Size: {} bytes", artifacts.size);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn display_path(path: &Path) -> PathBuf {
    path.strip_prefix(std::env::current_dir().unwrap_or_default())
        .unwrap_or(path)
        .to_owned()
}

fn print_help() {
    println!(
        "Speck — a tiny language for tiny games.\n\nUsage:\n  speck build <game.spk>\n  speck dev <game.spk> [--bind IP] [--port PORT] [--frames COUNT]\n  speck run <game.spk> [--frames COUNT]\n\nCommands:\n  build    Check, compile, and link a game with headless/PPM CRuMB\n  dev      Run a native game with the development browser presenter\n  run      Build and run a game with the native macOS Cocoa presenter"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_run_is_unbounded_by_default_and_accepts_a_finite_override() {
        let (source, options) = parse_native_args(&[OsString::from("game.spk")])
            .expect("default native arguments should parse");
        assert_eq!(source, PathBuf::from("game.spk"));
        assert_eq!(options.frame_limit, None);

        let (_, options) = parse_native_args(&[
            OsString::from("--frames"),
            OsString::from("3"),
            OsString::from("game.spk"),
        ])
        .expect("finite native arguments should parse");
        assert_eq!(options.frame_limit, Some(3));
    }

    #[test]
    fn native_run_rejects_zero_or_missing_frame_limits() {
        let zero = parse_native_args(&[
            OsString::from("game.spk"),
            OsString::from("--frames"),
            OsString::from("0"),
        ])
        .expect_err("zero frames should fail");
        assert!(zero.contains("greater than zero"));

        let missing = parse_native_args(&[OsString::from("game.spk"), OsString::from("--frames")])
            .expect_err("a missing count should fail");
        assert!(missing.contains("requires a positive integer"));
    }
}
