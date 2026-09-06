pub mod ast;
mod builtins;
mod checked;
pub mod cli;
pub mod codegen;
pub mod dev;
pub mod diagnostic;
pub mod lexer;
pub mod parser;
mod runtime_sources;
pub mod sema;
pub mod toolchain;

use std::path::Path;

use diagnostic::Diagnostic;

pub use checked::{CheckedProgram, analyze};

pub fn compile_to_llvm(source: &str) -> Result<String, Vec<Diagnostic>> {
    let program = analyze(source)?;
    Ok(codegen::llvm::emit(&program))
}

pub fn compile_to_llvm_for_target(
    source: &str,
    target_triple: &str,
) -> Result<String, Vec<Diagnostic>> {
    let program = analyze(source)?;
    Ok(codegen::llvm::emit_for_target(
        &program,
        Some(target_triple),
    ))
}

pub fn render_diagnostics(path: &Path, source: &str, diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.render(path, source))
        .collect::<Vec<_>>()
        .join("\n")
}
