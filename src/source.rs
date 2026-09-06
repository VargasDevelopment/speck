//! Source ownership shared by file diagnostics, code generation, and tooling.
use std::path::{Path, PathBuf};

use crate::diagnostic::{Diagnostic, Span};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SourceId(pub(crate) usize);

impl SourceId {
    pub const DEFAULT: Self = Self(0);
}

#[derive(Clone, Debug)]
pub struct SourceFile {
    path: PathBuf,
    text: String,
    line_starts: Vec<usize>,
}

impl SourceFile {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn text(&self) -> &str {
        &self.text
    }

    /// One-based line and Unicode character column for a byte offset.
    pub fn location(&self, offset: usize) -> (usize, usize) {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        let line = self.line_starts.partition_point(|start| *start <= offset) - 1;
        (
            line + 1,
            self.text[self.line_starts[line]..offset].chars().count() + 1,
        )
    }
}

#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
    dependencies: Vec<PathBuf>,
}

impl SourceMap {
    pub(crate) fn record_dependency(&mut self, path: PathBuf) {
        if !self.dependencies.contains(&path) {
            self.dependencies.push(path);
        }
    }

    /// Files attempted by the loader, including missing imports for edit recovery.
    pub fn dependencies(&self) -> impl Iterator<Item = &Path> {
        self.dependencies.iter().map(PathBuf::as_path)
    }

    pub(crate) fn add(&mut self, path: PathBuf, text: String) -> SourceId {
        let id = SourceId(self.files.len());
        let line_starts = std::iter::once(0)
            .chain(
                text.bytes()
                    .enumerate()
                    .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
            )
            .collect();
        self.files.push(SourceFile {
            path,
            text,
            line_starts,
        });
        id
    }

    pub fn get(&self, id: SourceId) -> Option<&SourceFile> {
        self.files.get(id.0)
    }
    pub fn files(&self) -> impl Iterator<Item = &SourceFile> {
        self.files.iter()
    }
    pub fn location(&self, span: Span) -> Option<(&Path, usize, usize)> {
        let file = self.get(span.source)?;
        let (line, column) = file.location(span.start);
        Some((file.path(), line, column))
    }

    pub fn render(&self, diagnostics: &[Diagnostic]) -> String {
        diagnostics
            .iter()
            .map(|diagnostic| match self.get(diagnostic.span.source) {
                Some(file) => diagnostic.render(file.path(), file.text()),
                None => format!("error: {}", diagnostic.message),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// A failed file compilation retains every source read before the failure.
#[derive(Debug)]
pub struct AnalysisError {
    pub sources: SourceMap,
    pub diagnostics: Vec<Diagnostic>,
}

impl std::fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.sources.render(&self.diagnostics))
    }
}

impl std::error::Error for AnalysisError {}
