use std::ffi::OsString;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::{compile_to_llvm_for_target, dev, render_diagnostics, toolchain};

pub fn run(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let args: Vec<OsString> = args.into_iter().collect();
    match args.as_slice() {
        [_program, command, source] if command == "build" => build(Path::new(source)),
        [_program, command, rest @ ..] if command == "dev" => development(rest),
        [_program, flag] if flag == "--help" || flag == "-h" => {
            print_help();
            ExitCode::SUCCESS
        }
        [_program] => {
            print_help();
            ExitCode::from(2)
        }
        _ => {
            eprintln!("error: expected `speck build <game.spk>` or `speck dev <game.spk>`\n");
            print_help();
            ExitCode::from(2)
        }
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
        "Speck — a tiny language for tiny games.\n\nUsage:\n  speck build <game.spk>\n  speck dev <game.spk> [--bind IP] [--port PORT] [--frames COUNT]\n\nCommands:\n  build    Check, compile, and link a game with headless/PPM CRuMB\n  dev      Run a native game with the development browser presenter"
    );
}
