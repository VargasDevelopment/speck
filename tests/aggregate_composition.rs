use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[test]
fn nested_aggregates_lower_through_one_composable_path() {
    let source = r#"game "Composition"
struct Platform { x: i32, width: i32 }
struct Level { platforms: [Platform; 2], positions: [i32; 2] }
const BASE: [Platform; 2] = [
    Platform { width: 5, x: 10 },
    Platform { x: 20, width: 6 }
]
let levels: [Level; 1] = [
    Level {
        positions: [30, 40],
        platforms: [
            Platform { x: 1, width: 2 },
            Platform { width: 4, x: 3 }
        ]
    }
]
fn show(platform: Platform) -> void { print_i32(platform.x) }
start {
    let copy: Platform = levels[0].platforms[0]
    copy.x = 99
    levels[0].platforms[1].width += 8
    levels[0].positions[0] = 50
    show(levels[0].platforms[1])
    print_i32(copy.x)
    print_i32(levels[0].platforms[0].x)
    print_i32(levels[0].platforms[1].width)
    print_i32(levels[0].positions[0])
    print_i32(BASE[1].width)
}
update(dt: f32) {}
draw {}
"#;
    let ir = speck::compile_to_llvm(source).expect("nested aggregates should compile");
    assert!(ir.contains("%spk_struct_Level = type { [2 x %spk_struct_Platform], [2 x i32] }"));
    assert!(ir.contains("@spk_const_BASE = internal constant [2 x %spk_struct_Platform]"));
    assert!(ir.matches("getelementptr inbounds").count() >= 10);

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = root.join("target/aggregate_composition_e2e");
    fs::create_dir_all(&work).expect("aggregate test directory should exist");
    let path = work.join("aggregate_values.spk");
    fs::write(&path, source).expect("aggregate source should be written");
    let executable = build_in(&work, &path);
    let run = Command::new(executable)
        .current_dir(&work)
        .output()
        .expect("aggregate example should start");
    assert_success("aggregate value example", &run);
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "3\n99\n1\n12\n50\n6\n"
    );
}

#[test]
fn nested_const_paths_are_read_only() {
    assert_error(
        r#"game "Bad"
struct Platform { x: i32 }
struct Level { platforms: [Platform; 1] }
const LEVEL: Level = Level { platforms: [Platform { x: 1 }] }
start { LEVEL.platforms[0].x += 1 }
update(dt: f32) {}
draw {}
"#,
        "cannot use compound assignment through constant `LEVEL`",
    );
}

#[test]
fn indexed_aggregate_rvalues_are_materialized_safely() {
    let source = r#"game "Rvalue Index"
struct Level { positions: [i32; 2] }
fn make_level() -> Level {
    return Level { positions: [7, 9] }
}
start {
    let index: i32 = 1
    print_i32(make_level().positions[index])
}
update(dt: f32) {}
draw {}
"#;
    let ir = speck::compile_to_llvm(source).expect("aggregate rvalue indexing should compile");
    assert!(ir.contains("alloca [2 x i32]"));
    assert!(ir.contains("getelementptr inbounds [2 x i32]"));

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = root.join("target/aggregate_rvalue_index");
    fs::create_dir_all(&work).expect("rvalue index directory should exist");
    let path = work.join("rvalue_index.spk");
    fs::write(&path, source).expect("rvalue index source should be written");
    let executable = build_in(&work, &path);
    let run = Command::new(executable)
        .current_dir(&work)
        .output()
        .expect("rvalue index example should start");
    assert_success("aggregate rvalue indexing", &run);
    assert_eq!(String::from_utf8_lossy(&run.stdout), "9\n");
}

#[test]
fn malformed_nested_initializers_are_diagnosed() {
    assert_error(
        r#"game "Bad"
struct Platform { x: i32 }
struct Level { platforms: [Platform; 2] }
let level: Level = Level { platforms: [Platform { x: 1 }] }
start {}
update(dt: f32) {}
draw {}
"#,
        "expected array length 2, found 1 elements",
    );
    assert_error(
        r#"game "Bad"
struct Platform { x: i32, width: i32 }
let values: [Platform; 1] = [Platform { x: 1 }]
start {}
update(dt: f32) {}
draw {}
"#,
        "missing initializer for field `width`",
    );
}

#[test]
fn struct_array_bounds_checks_fail_at_runtime() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = root.join("target/struct_array_bounds");
    fs::create_dir_all(&work).expect("bounds directory should exist");
    let path = work.join("struct_bounds.spk");
    fs::write(
        &path,
        r#"game "Bounds"
struct Point { x: i32 }
let index: i32 = 2
let points: [Point; 2] = [Point { x: 1 }, Point { x: 2 }]
start { print_i32(points[index].x) }
update(dt: f32) {}
draw {}
"#,
    )
    .expect("bounds source should be written");
    let executable = build_in(&work, &path);
    let run = Command::new(executable)
        .current_dir(&work)
        .output()
        .expect("bounds example should start");
    assert!(!run.status.success(), "out-of-bounds access must fail");
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        "Speck array index 2 is out of bounds for length 2\n"
    );
}

#[test]
fn platform_array_example_builds_verifies_and_renders() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = root.join("target/platform_array_e2e");
    fs::create_dir_all(&work).expect("platform array directory should exist");
    let executable = build_in(&work, &root.join("examples/platform_array.spk"));
    let run = Command::new(executable)
        .current_dir(&work)
        .output()
        .expect("platform array should start");
    assert_success("platform array", &run);

    let ppm = fs::read(work.join("build/frame.ppm")).expect("frame should be written");
    let pixel = |x: usize, y: usize| {
        const HEADER: usize = 15;
        let offset = HEADER + (y * 320 + x) * 3;
        &ppm[offset..offset + 3]
    };
    assert_eq!(pixel(40, 140), [255, 255, 255]);
    assert_eq!(pixel(150, 105), [255, 255, 255]);
    assert_eq!(pixel(240, 70), [255, 255, 255]);
    assert_eq!(pixel(39, 140), [32, 18, 32]);

    let verify = Command::new("llvm-as")
        .arg(work.join("build/platform_array.ll"))
        .arg("-o")
        .arg(work.join("verified.bc"))
        .output()
        .expect("llvm-as should start");
    assert_success("aggregate LLVM verification", &verify);
}

fn assert_error(source: &str, expected: &str) {
    let errors = speck::analyze(source).expect_err("source should be rejected");
    assert!(
        errors.iter().any(|error| error.message.contains(expected)),
        "expected diagnostic containing {expected:?}, found: {errors:#?}"
    );
}

fn build_in(work: &Path, source: &Path) -> PathBuf {
    let build = Command::new(env!("CARGO_BIN_EXE_speck"))
        .current_dir(work)
        .args(["build"])
        .arg(source)
        .output()
        .expect("Speck compiler should start");
    assert_success("aggregate build", &build);
    let stem = source.file_stem().expect("source should have a file stem");
    work.join("build").join(stem)
}

fn assert_success(description: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{description} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
