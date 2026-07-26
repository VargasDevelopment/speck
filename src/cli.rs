use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::{compile_to_llvm_for_target, render_diagnostics, toolchain};

pub fn run(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let args: Vec<OsString> = args.into_iter().collect();
    match args.as_slice() {
        [_program, command, source] if command == "build" => build(Path::new(source)),
        [_program, flag] if flag == "--help" || flag == "-h" => {
            print_help();
            ExitCode::SUCCESS
        }
        [_program] => {
            print_help();
            ExitCode::from(2)
        }
        _ => {
            eprintln!("error: expected `speck build <game.spk>`\n");
            print_help();
            ExitCode::from(2)
        }
    }
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
        "Speck — a tiny language for tiny games.\n\nUsage:\n  speck build <game.spk>\n\nCommands:\n  build    Check, compile, and link a game with CRuMB"
    );
}
