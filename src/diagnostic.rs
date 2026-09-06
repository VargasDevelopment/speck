use std::path::Path;

use crate::source::SourceId;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub source: SourceId,
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self {
            source: SourceId::DEFAULT,
            start,
            end,
        }
    }

    pub const fn merge(self, other: Self) -> Self {
        assert!(
            self.source.0 == other.source.0,
            "cannot merge spans from different files"
        );
        Self {
            source: self.source,
            start: self.start,
            end: other.end,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub span: Span,
}

impl Diagnostic {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }

    pub fn render(&self, path: &Path, source: &str) -> String {
        let mut start = self.span.start.min(source.len());
        while !source.is_char_boundary(start) {
            start -= 1;
        }
        let line_start = source[..start].rfind('\n').map_or(0, |index| index + 1);
        let line_end = source[start..]
            .find('\n')
            .map_or(source.len(), |index| start + index);
        let line_number = source[..start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1;
        let column = source[line_start..start].chars().count() + 1;
        let line = &source[line_start..line_end];
        let marker_offset = source[line_start..start].chars().count();
        let available = line_end.saturating_sub(start);
        let mut end = start + self.span.end.saturating_sub(self.span.start).min(available);
        while !source.is_char_boundary(end) {
            end -= 1;
        }
        let marker_len = source[start..end].chars().count().max(1);
        let gutter_width = line_number.to_string().len();

        format!(
            "{}:{}:{}: error: {}\n{:width$} |\n{} | {}\n{:width$} | {}{}",
            path.display(),
            line_number,
            column,
            self.message,
            "",
            line_number,
            line,
            "",
            " ".repeat(marker_offset),
            "^".repeat(marker_len),
            width = gutter_width,
        )
    }
}
