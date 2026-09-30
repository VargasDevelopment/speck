//! Immutable game data and the runtime lookup ABI, separate from expression emission.
use std::fmt::Write;

use crate::ast::Program;

pub(super) fn emit(output: &mut String, program: &Program) {
    let mut identity = program.title.as_bytes().to_vec();
    identity.push(0);
    writeln!(
        output,
        "@spk_storage_identity_data = private unnamed_addr constant [{} x i8] {}, align 1",
        identity.len(),
        llvm_byte_string(&identity)
    )
    .expect("writing to a string cannot fail");
    let mut sounds = program.sounds.iter().collect::<Vec<_>>();
    sounds.sort_by_key(|sound| sound.handle);
    for sound in &sounds {
        writeln!(
            output,
            "@spk_sound_asset_{} = private unnamed_addr constant [{} x i8] {}, align 2",
            sound.handle,
            sound.pcm.len(),
            llvm_byte_string(&sound.pcm)
        )
        .expect("writing to a string cannot fail");
    }
    output.push_str("\ndeclare void @crumb_storage_init(ptr, i64)\n\n");
    output.push_str(
        "define i32 @spk_sound_lookup(i32 %handle, ptr %data, ptr %sample_count) {\nentry:\n",
    );
    if sounds.is_empty() {
        output.push_str(
            "  store ptr null, ptr %data\n  store i32 0, ptr %sample_count\n  ret i32 0\n}\n\n",
        );
        return;
    }
    output.push_str("  switch i32 %handle, label %missing [\n");
    for sound in &sounds {
        writeln!(
            output,
            "    i32 {}, label %sound_{}",
            sound.handle, sound.handle
        )
        .expect("writing to a string cannot fail");
    }
    output.push_str("  ]\n");
    for sound in &sounds {
        writeln!(
            output,
            "sound_{}:\n  store ptr @spk_sound_asset_{}, ptr %data\n  store i32 {}, ptr %sample_count\n  ret i32 1",
            sound.handle,
            sound.handle,
            sound.pcm.len() / 2
        )
        .expect("writing to a string cannot fail");
    }
    output.push_str("missing:\n  store ptr null, ptr %data\n  store i32 0, ptr %sample_count\n  ret i32 0\n}\n\n");
}

fn llvm_byte_string(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(3).saturating_add(3));
    output.push_str("c\"");
    for &byte in bytes {
        if matches!(byte, 0x20..=0x7e) && byte != b'"' && byte != b'\\' {
            output.push(char::from(byte));
        } else {
            write!(output, "\\{byte:02X}").expect("writing to a string cannot fail");
        }
    }
    output.push('"');
    output
}
