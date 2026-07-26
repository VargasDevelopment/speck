use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const WIDTH: usize = 320;
const CHANNELS: usize = 3;
const PPM_HEADER: &[u8] = b"P6\n320 180\n255\n";

#[test]
fn delta_rectangle_executes_with_short_circuiting_and_expected_pixels() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = root.join("target/language_delta_e2e");
    fs::create_dir_all(&work).expect("test working directory should be created");
    let executable = build_in(&work, &root.join("examples/delta_rectangle.spk"));

    let run = Command::new(executable)
        .current_dir(&work)
        .output()
        .expect("delta rectangle should start");
    assert_success("delta rectangle", &run);
    assert_eq!(
        run.stdout, b"",
        "short-circuited calls must not print their sentinel"
    );

    let ppm = fs::read(work.join("build/frame.ppm")).expect("frame.ppm should be written");
    assert_eq!(&ppm[..PPM_HEADER.len()], PPM_HEADER);
    assert_eq!(pixel(&ppm, 10, 90), [240, 150, 40]);
    assert_eq!(pixel(&ppm, 9, 90), [255, 255, 255]);

    let ir = fs::read_to_string(work.join("build/delta_rectangle.ll"))
        .expect("generated LLVM IR should remain inspectable");
    assert!(ir.contains("define void @spk_fn_draw_box"));
    assert!(ir.contains("phi i1"));
    assert!(ir.contains("fptosi float"));
    assert!(!ir.contains("@spk_global_SCREEN_WIDTH"));
}

#[test]
fn safe_float_to_integer_conversion_executes_at_boundaries() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = root.join("target/conversion_e2e");
    fs::create_dir_all(&work).expect("test working directory should be created");
    let source = work.join("conversion_boundaries.spk");
    fs::write(
        &source,
        r#"game "Conversion Boundaries"
let zero: f32 = 0.0
let calls: i32 = 0
let value: i32 = 1
fn once() -> i32 {
    calls += 1
    return 2
}
start {
    print_i32(i32(3.9))
    print_i32(i32(-3.9))
    print_i32(i32(2147483648.0))
    print_i32(i32(-2147483648.0))
    print_i32(i32(zero / zero))
    value += once()
    print_i32(calls)
    print_i32(value)
}
update(dt: f32) {}
draw {}
"#,
    )
    .expect("test source should be written");
    let executable = build_in(&work, &source);

    let run = Command::new(executable)
        .current_dir(&work)
        .output()
        .expect("conversion executable should start");
    assert_success("conversion executable", &run);
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "3\n-3\n2147483647\n-2147483648\n0\n1\n3\n"
    );
}

fn build_in(work: &Path, source: &Path) -> PathBuf {
    let build = Command::new(env!("CARGO_BIN_EXE_speck"))
        .current_dir(work)
        .args(["build"])
        .arg(source)
        .output()
        .expect("Speck compiler should start");
    assert_success("language feature build", &build);
    let stem = source
        .file_stem()
        .expect("test source should have a file stem");
    work.join("build").join(stem)
}

fn pixel(ppm: &[u8], x: usize, y: usize) -> [u8; CHANNELS] {
    let offset = PPM_HEADER.len() + (y * WIDTH + x) * CHANNELS;
    ppm[offset..offset + CHANNELS]
        .try_into()
        .expect("pixel should contain three channels")
}

fn assert_success(description: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{description} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
