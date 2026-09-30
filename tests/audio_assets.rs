pub mod support;

use std::fs;
use std::process::Command;

use speck::ast::ConstantValue;
use support::assert_success;

fn wav(samples: &[i16]) -> Vec<u8> {
    let data_len = u32::try_from(samples.len() * 2).unwrap();
    let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&48_000u32.to_le_bytes());
    bytes.extend_from_slice(&96_000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

#[test]
fn imported_sound_is_relative_embedded_typed_and_watched() {
    let work = support::workspace();
    fs::create_dir_all(work.path().join("library")).unwrap();
    fs::create_dir_all(work.path().join("assets")).unwrap();
    let asset = work.path().join("assets/track.wav");
    fs::write(&asset, wav(&[1000, -1000, 32767, -32768])).unwrap();
    fs::write(
        work.path().join("library/sounds.spk"),
        "sound TRACK = \"../assets/track.wav\"",
    )
    .unwrap();
    let source = work.path().join("game.spk");
    fs::write(
        &source,
        r#"game "Assets"
import "library/sounds.spk" as sound
start {
    sound_play(sound::TRACK, 0.5)
    sound_pause()
    sound_resume()
    sound_seek(0.25)
    print_i32(i32(sound_position()))
    sound_stop()
}
update(dt: f32) {}
draw {}
"#,
    )
    .unwrap();

    let checked = speck::analyze_path(&source).unwrap();
    assert_eq!(checked.ast().sounds.len(), 1);
    assert_eq!(checked.ast().sounds[0].name, "library/sounds.spk::TRACK");
    assert_eq!(checked.ast().sounds[0].handle, 1);
    assert_eq!(
        checked.ast().sounds[0].pcm,
        wav(&[1000, -1000, 32767, -32768])[44..]
    );
    assert_eq!(
        checked
            .ast()
            .constants
            .iter()
            .find(|constant| constant.name.ends_with("::TRACK"))
            .and_then(|constant| constant.value.as_ref()),
        Some(&ConstantValue::I32(1))
    );
    assert!(
        checked
            .sources()
            .dependencies()
            .any(|dependency| dependency.ends_with("assets/track.wav"))
    );

    let ir = speck::codegen::llvm::emit(&checked);
    assert!(ir.contains("@spk_sound_asset_1 = private unnamed_addr constant [8 x i8]"));
    assert!(ir.contains("store i32 4, ptr %sample_count"));
    assert!(ir.contains("call void @crumb_sound_play(i32 1, float"));
    assert!(ir.contains("call float @crumb_sound_position()"));
    fs::write(work.path().join("asset.ll"), ir).unwrap();
    assert_success(
        "asset LLVM verification",
        &support::verify_ir(&work.path().join("asset.ll"), &work.path().join("asset.bc")),
    );
}

#[test]
fn embedded_sound_runs_after_sources_and_working_directory_disappear() {
    let work = support::workspace();
    let source = work.path().join("game.spk");
    let asset = work.path().join("track.wav");
    fs::write(&asset, wav(&[1000, -1000, 2000, -2000])).unwrap();
    fs::write(
        &source,
        r#"game "Portable asset"
sound TRACK = "track.wav"
start {
    sound_play(TRACK, 1.0)
    sound_pause()
    sound_resume()
    sound_seek(0.0)
    sound_stop()
    print_i32(i32(sound_position()))
}
update(dt: f32) {}
draw {}
"#,
    )
    .unwrap();
    let executable = support::build_in(work.path(), &source);
    fs::remove_file(source).unwrap();
    fs::remove_file(asset).unwrap();
    let elsewhere = support::workspace();
    fs::create_dir(elsewhere.path().join("build")).unwrap();
    let output = support::run(Command::new(executable).current_dir(elsewhere.path()));
    assert_success("self-contained embedded sound executable", &output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "0\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn sound_diagnostics_are_local_and_keep_missing_or_oversized_dependencies() {
    let work = support::workspace();
    let source = work.path().join("game.spk");
    fs::write(
        &source,
        "game \"Bad\"\nsound TRACK = \"missing.wav\"\nstart {} update(dt: f32) {} draw {}",
    )
    .unwrap();
    let failure = speck::analyze_path(&source).unwrap_err();
    let rendered = failure.to_string();
    assert!(rendered.contains("game.spk:2:1"), "{rendered}");
    assert!(
        rendered.contains("could not read sound asset"),
        "{rendered}"
    );
    assert!(
        failure
            .sources
            .dependencies()
            .any(|path| path.ends_with("missing.wav"))
    );

    fs::write(work.path().join("invalid.wav"), b"not a wave").unwrap();
    fs::write(
        &source,
        "game \"Bad\"\nsound TRACK = \"invalid.wav\"\nstart {} update(dt: f32) {} draw {}",
    )
    .unwrap();
    let failure = speck::analyze_path(&source).unwrap_err().to_string();
    assert!(failure.contains("expected a RIFF/WAVE file"), "{failure}");

    let oversized = work.path().join("oversized.wav");
    let file = fs::File::create(&oversized).unwrap();
    file.set_len(32 * 1024 * 1024 + 1).unwrap();
    fs::write(
        &source,
        "game \"Bad\"\nsound TRACK = \"oversized.wav\"\nstart {} update(dt: f32) {} draw {}",
    )
    .unwrap();
    let failure = speck::analyze_path(&source).unwrap_err().to_string();
    assert!(failure.contains("32 MiB encoded size limit"), "{failure}");
}

#[test]
fn source_only_analysis_rejects_resource_declarations_without_general_strings() {
    let errors = speck::analyze(
        "game \"Memory\" sound TRACK = \"track.wav\" start {} update(dt: f32) {} draw {}",
    )
    .unwrap_err();
    assert_eq!(
        errors[0].message,
        "sound assets require file-based analysis so their paths can be resolved"
    );
    assert!(
        speck::analyze("game \"Memory\" start { print_i32(\"nope\") } update(dt: f32) {} draw {}")
            .is_err()
    );
}
