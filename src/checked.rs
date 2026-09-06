use crate::ast::Program;
use crate::diagnostic::Diagnostic;
use crate::source::{AnalysisError, SourceMap};
use crate::{lexer, parser, sema};
use std::path::{Path, PathBuf};

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
/// let program = speck::CheckedProgram { ast, sources: Default::default() };
/// ```
#[derive(Debug)]
pub struct CheckedProgram {
    ast: Program,
    sources: SourceMap,
}

impl CheckedProgram {
    /// Source text and dependency paths used by this checked compilation.
    pub fn sources(&self) -> &SourceMap {
        &self.sources
    }

    fn validate(mut ast: Program, sources: SourceMap) -> Result<Self, AnalysisError> {
        match sema::check(&mut ast) {
            Ok(()) => Ok(Self { ast, sources }),
            Err(diagnostics) => Err(AnalysisError {
                sources,
                diagnostics,
            }),
        }
    }

    /// Borrow the validated AST for read-only inspection.
    pub fn ast(&self) -> &Program {
        &self.ast
    }
}

/// Parse and validate source, retaining the single AST in an immutable owner.
pub fn analyze(source: &str) -> Result<CheckedProgram, Vec<Diagnostic>> {
    let tokens = lexer::lex(source)?;
    let ast = parser::parse(tokens)?;
    let mut sources = SourceMap::default();
    sources.add(PathBuf::from("<source>"), source.to_owned());
    CheckedProgram::validate(ast, sources).map_err(|error| error.diagnostics)
}

/// Load an entry file and its import closure, resolve names, and validate once.
pub fn analyze_path(path: &Path) -> Result<CheckedProgram, AnalysisError> {
    let (ast, sources) = crate::modules::load(path)?;
    CheckedProgram::validate(ast, sources)
}
