pub mod ast;
mod builtins;
pub mod cli;
pub mod codegen;
pub mod dev;
pub mod diagnostic;
pub mod lexer;
pub mod parser;
pub mod sema;
pub mod toolchain;

use std::path::Path;

use ast::Program;
use diagnostic::Diagnostic;

pub fn analyze(source: &str) -> Result<Program, Vec<Diagnostic>> {
    let tokens = lexer::lex(source)?;
    let mut program = parser::parse(tokens)?;
    sema::check(&mut program)?;
    Ok(program)
}

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
