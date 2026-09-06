//! Optional source reporting belongs to development emission, not runtime values.
use std::fmt::Write;

use crate::diagnostic::Span;
use crate::source::SourceMap;

#[derive(Clone, Copy)]
pub(super) enum RuntimeDiagnostics<'a> {
    Plain,
    Located(&'a SourceMap),
}

impl RuntimeDiagnostics<'_> {
    pub(super) fn declarations(self) -> String {
        let Self::Located(sources) = self else {
            return String::new();
        };
        let mut output = String::from("declare void @crumb_source_location(ptr, i32, i32)\n");
        for (id, source) in sources.files().enumerate() {
            let path = source.path().to_string_lossy();
            let mut encoded = String::new();
            // LLVM byte escapes keep quotes, backslashes, newlines and UTF-8
            // inside the string constant, including unusual imported paths.
            for byte in path.as_bytes() {
                write!(encoded, "\\{byte:02X}").expect("string writing cannot fail");
            }
            writeln!(
                output,
                "@spk_source_{id} = private unnamed_addr constant [{} x i8] c\"{encoded}\\00\"",
                path.len() + 1
            )
            .expect("string writing cannot fail");
        }
        output
    }

    pub(super) fn instruction(self, span: Span) -> Option<String> {
        let Self::Located(sources) = self else {
            return None;
        };
        let (_, line, column) = sources
            .location(span)
            .expect("checked expression spans belong to retained source files");
        Some(format!(
            "call void @crumb_source_location(ptr @spk_source_{}, i32 {line}, i32 {column})",
            span.source.0
        ))
    }
}
