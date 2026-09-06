use crate::ast::Program;
use crate::diagnostic::Diagnostic;
use crate::{lexer, parser, sema};

/// An owned program that has successfully completed semantic validation.
///
/// Obtain this through [`analyze`]. The validated AST can be inspected through
/// [`Self::ast`], but cannot be mutated or replaced before LLVM emission.
///
/// ```
/// let source = r#"game "Example"
/// start {} update(dt: f32) {} draw {}"#;
/// let program = speck::analyze(source).unwrap();
/// assert_eq!(program.ast().title, "Example");
/// let llvm = speck::codegen::llvm::emit(&program);
/// assert!(llvm.contains("@spk_start"));
/// let targeted = speck::codegen::llvm::emit_for_target(
///     &program, Some("aarch64-apple-darwin"),
/// );
/// assert!(targeted.contains("target triple = \"aarch64-apple-darwin\""));
/// ```
///
/// Even a mutable binding does not allow changing the checked AST:
///
/// ```compile_fail,E0596
/// let mut program = speck::analyze(
///     r#"game "Example" start {} update(dt: f32) {} draw {}"#,
/// ).unwrap();
/// program.ast().functions.clear();
/// ```
///
/// An unchecked AST cannot be wrapped directly:
///
/// ```compile_fail,E0451
/// let tokens = speck::lexer::lex(
///     r#"game "Example" start {} update(dt: f32) {} draw {}"#,
/// ).unwrap();
/// let ast = speck::parser::parse(tokens).unwrap();
/// let program = speck::CheckedProgram { ast };
/// ```
#[derive(Debug)]
pub struct CheckedProgram {
    ast: Program,
}

impl CheckedProgram {
    /// Borrow the validated AST for read-only inspection.
    pub fn ast(&self) -> &Program {
        &self.ast
    }
}

/// Parse and validate source, retaining the single AST in an immutable owner.
pub fn analyze(source: &str) -> Result<CheckedProgram, Vec<Diagnostic>> {
    let tokens = lexer::lex(source)?;
    let mut ast = parser::parse(tokens)?;
    sema::check(&mut ast)?;
    Ok(CheckedProgram { ast })
}
