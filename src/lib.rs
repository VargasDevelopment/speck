pub mod ast;
mod builtins;
mod checked;
pub mod cli;
pub mod codegen;
pub mod dev;
pub mod diagnostic;
pub mod lexer;
mod modules;
pub mod parser;
mod runtime_sources;
pub mod sema;
pub mod source;
pub mod toolchain;

use std::path::Path;

use diagnostic::Diagnostic;

pub use checked::{CheckedProgram, analyze, analyze_path};
pub use source::AnalysisError;

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

/// Compile a file and its imports for a selected target.
pub fn compile_path_to_llvm_for_target(
    path: &Path,
    target_triple: &str,
) -> Result<String, AnalysisError> {
    let program = analyze_path(path)?;
    Ok(codegen::llvm::emit_for_target(
        &program,
        Some(target_triple),
    ))
}

/// Compile a file and its imports with no target triple embedded in the IR.
pub fn compile_path_to_llvm(path: &Path) -> Result<String, AnalysisError> {
    let program = analyze_path(path)?;
    Ok(codegen::llvm::emit(&program))
}
