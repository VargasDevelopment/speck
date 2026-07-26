use std::process::ExitCode;

fn main() -> ExitCode {
    speck::cli::run(std::env::args_os())
}
