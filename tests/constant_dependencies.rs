pub mod support;

use std::fs;
use std::process::Command;

use speck::ast::ConstantValue;

const ENTRIES: &str = "start { print_i32(VALUE) } update(dt: f32) {} draw {}";

#[test]
fn constant_dependencies_fit_an_ordinary_worker_stack() {
    const CHILD: &str = "SPECK_CONSTANT_DEPENDENCY_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = support::run(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "constant_dependencies_fit_an_ordinary_worker_stack",
                    "--nocapture",
                ])
                .env(CHILD, "1"),
        );
        support::assert_success("constant dependencies on a 2 MiB worker stack", &output);
        return;
    }
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            for use_length in [false, true] {
                for (files, layers) in [(128, 1), (16, 20)] {
                    check_module_chain(files, layers, use_length);
                }
                check_forward_chain(512, 20, use_length);
            }
            check_aggregate_length_chain();
        })
        .unwrap()
        .join()
        .unwrap();
}

fn wrapped(name: &str, layers: usize) -> String {
    format!("{}{name}{}", "0 + (".repeat(layers), ")".repeat(layers))
}

fn length_use(enabled: bool) -> &'static str {
    if enabled {
        "struct Sized { items: [i32; VALUE] }"
    } else {
        ""
    }
}

fn check_module_chain(files: usize, layers: usize, use_length: bool) {
    let directory = support::workspace();
    for index in 0..files {
        let mut source = if index == 0 {
            "game \"Constant chain\"\n".to_owned()
        } else {
            String::new()
        };
        if index + 1 < files {
            source.push_str(&format!(
                "import \"{}.spk\" as next\nconst VALUE: i32 = {}\n",
                index + 1,
                wrapped("next::VALUE", layers)
            ));
        } else {
            source.push_str("const VALUE: i32 = 1\n");
        }
        if index == 0 {
            source.push_str(length_use(use_length));
            source.push_str(ENTRIES);
        }
        fs::write(directory.path().join(format!("{index}.spk")), source).unwrap();
    }
    let program = speck::analyze_path(&directory.path().join("0.spk")).unwrap_or_else(|failure| {
        panic!("files={files}, layers={layers}, length={use_length}: {failure}")
    });
    assert_eq!(program.sources().files().count(), files);
    check_value_and_emission(&program);
}

fn check_forward_chain(count: usize, layers: usize, use_length: bool) {
    let mut source = "game \"Forward constants\"\nconst VALUE: i32 = C0\n".to_owned();
    for index in 0..count {
        let expression = if index + 1 == count {
            "1".into()
        } else {
            wrapped(&format!("C{}", index + 1), layers)
        };
        source.push_str(&format!("const C{index}: i32 = {expression}\n"));
    }
    source.push_str(length_use(use_length));
    source.push_str(ENTRIES);
    let program = speck::analyze(&source)
        .unwrap_or_else(|failure| panic!("forward chain, length={use_length}: {failure:?}"));
    check_value_and_emission(&program);
}

fn check_aggregate_length_chain() {
    let mut source = "game \"Aggregate length dependencies\"\nconst VALUE: i32 = ROW0[0]\nstruct Sized { items: [i32; VALUE] }\n".to_owned();
    for index in 0..128 {
        let expression = if index == 127 {
            "1".into()
        } else {
            format!("ROW{}[0]", index + 1)
        };
        source.push_str(&format!(
            "const ROW{index}: [i32; WIDTH] = [{expression}]\n"
        ));
    }
    source.push_str("const WIDTH: i32 = 1\n");
    source.push_str(ENTRIES);
    let program = speck::analyze(&source).unwrap();
    check_value_and_emission(&program);
}

fn check_value_and_emission(program: &speck::CheckedProgram) {
    let constant = program
        .ast()
        .constants
        .iter()
        .find(|constant| constant.name == "VALUE")
        .unwrap();
    assert_eq!(constant.value, Some(ConstantValue::I32(1)));
    assert!(speck::codegen::llvm::emit(program).contains("call void @crumb_print_i32(i32 1)"));
}

#[test]
fn only_demanded_dependencies_participate_in_constant_cycles() {
    for (operator, flag) in [("||", "true"), ("&&", "false")] {
        for use_length in [false, true] {
            let source = format!(
                r#"game "Lazy dependency cycle"
struct Config {{ width: i32 enabled: bool }}
const VALUE: i32 = CONFIG.width
const CONFIG: Config = Config {{ width: 1, enabled: A }}
const A: bool = FLAG {operator} B
const B: bool = A
const FLAG: bool = {flag}
{}
{ENTRIES}
"#,
                length_use(use_length)
            );
            let program = speck::analyze(&source)
                .unwrap_or_else(|failure| panic!("{operator}, length={use_length}: {failure:?}"));
            check_value_and_emission(&program);
        }
    }
}

#[test]
fn wide_forward_constant_arrays_build_verify_and_execute() {
    let count = 2048;
    let entries = (0..count)
        .map(|index| format!("C{index} + 1"))
        .collect::<Vec<_>>()
        .join(", ");
    let definitions = (0..count)
        .map(|index| format!("const C{index}: i32 = {}", index % 17))
        .collect::<Vec<_>>()
        .join("\n");
    let source = format!(
        r#"game "Wide forward dependencies"
const VALUES: [i32; {count}] = [{entries}]
{definitions}
start {{
    let total: i32 = 0
    for index in 0..{count} {{ total += VALUES[index] }}
    print_i32(total)
}}
update(dt: f32) {{}}
draw {{}}
"#
    );
    let directory = support::workspace();
    let path = directory.path().join("wide.spk");
    fs::write(&path, source).unwrap();
    let executable = support::build_in(directory.path(), &path);
    support::assert_success(
        "wide constant IR verification",
        &support::verify_ir(
            &directory.path().join("build/wide.ll"),
            &directory.path().join("verified.bc"),
        ),
    );
    let output = support::run(Command::new(executable).current_dir(directory.path()));
    support::assert_success("wide constant executable", &output);
    let expected: i32 = (0..count).map(|index| index % 17 + 1).sum();
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{expected}\n")
    );
}
