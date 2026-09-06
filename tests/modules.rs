pub mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(files: &[(&str, &str)]) -> tempfile::TempDir {
    let work = support::workspace();
    for (path, source) in files {
        let path = work.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    work
}

fn root(imports: &str, start: &str) -> String {
    format!("game \"Modules\"\n{imports}\nstart {{ {start} }}\nupdate(dt: f32) {{}}\ndraw {{}}\n")
}

fn error(files: &[(&str, &str)]) -> speck::AnalysisError {
    let work = fixture(files);
    speck::analyze_path(&work.path().join("game.spk")).unwrap_err()
}

fn native(work: &Path, expected: &str) {
    let executable = support::build_in(work, &work.join("game.spk"));
    support::assert_success(
        "independent module IR verification",
        &support::verify_ir(&work.join("build/game.ll"), &work.join("verified.bc")),
    );
    let output = support::run(Command::new(executable).current_dir(work));
    support::assert_success("module executable", &output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn room_data_and_helpers_share_types_and_state_across_a_diamond() {
    let main = root(
        "import \"rooms.spk\" as rooms\nimport \"helpers.spk\" as helpers\nimport \"common.spk\" as common",
        "let value: common::Room = rooms::make()\nhelpers::show(value)\nrooms::tick()\nhelpers::tick()\nprint_i32(common::visits)\nprint_i32(rooms::DATA[1].x)\nprint_i32(rooms::make().x)",
    );
    let work = fixture(&[
        ("game.spk", &main),
        (
            "common.spk",
            "struct Room { x: i32 }\nconst COUNT: i32 = 2\nlet visits: i32 = 10\nfn tick() -> void { visits += 1 }",
        ),
        (
            "rooms.spk",
            "import \"./common.spk\" as model\nconst DATA: [model::Room; model::COUNT] = [model::Room { x: 7 }, model::Room { x: 9 }]\nfn make() -> model::Room { return DATA[0] }\nfn tick() -> void { model::tick() }",
        ),
        (
            "helpers.spk",
            "import \"common.spk\" as geometry\nfn show(value: geometry::Room) -> void { print_i32(value.x) }\nfn tick() -> void { geometry::tick() }",
        ),
    ]);
    let program = speck::analyze_path(&work.path().join("game.spk")).unwrap();
    assert_eq!(program.ast().structs.len(), 1);
    assert_eq!(program.ast().globals.len(), 1);
    assert_eq!(program.sources().files().count(), 4);
    native(work.path(), "7\n12\n9\n7\n");
}

#[test]
fn qualified_names_cover_empty_structs_conditions_and_lexical_shadowing() {
    let main = root(
        "import \"logic.spk\" as logic",
        "logic::exercise()\nlet value: logic::Empty = logic::Empty {}\nif logic::READY {}\nwhile logic::STOP {}\nlet logic: i32 = 9\nprint_i32(logic)",
    );
    let work = fixture(&[
        ("game.spk", &main),
        (
            "logic.spk",
            "struct Empty {}\nconst READY: bool = true\nconst STOP: bool = false\nlet counter: i32 = 3\nfn exercise() -> void {\nprint_i32(counter)\nlet counter: i32 = counter + 1\nif READY { let counter: i32 = counter + 1 print_i32(counter) }\nprint_i32(counter)\nfor counter in counter..counter + 1 { print_i32(counter) }\nprint_i32(counter)\n}\n",
        ),
    ]);
    native(work.path(), "3\n5\n4\n4\n4\n9\n");
}

#[test]
fn filenames_with_spaces_unicode_and_llvm_punctuation_compile() {
    let main = root(
        "import \"room data/étage-1.spk\" as rooms",
        "let r: rooms::Room = rooms::make() print_i32(r.x) print_i32(rooms::make().x) print_i32(rooms::DATA[0].x)",
    );
    let work = fixture(&[
        ("game.spk", &main),
        (
            "room data/étage-1.spk",
            "struct Room { x: i32 }\nconst DATA: [Room; 1] = [Room { x: 42 }]\nlet value: Room = Room { x: 12 }\nfn make() -> Room { return value }",
        ),
    ]);
    native(work.path(), "12\n12\n42\n");
}

#[test]
fn imports_resolve_relative_to_the_importer_and_alias_order_does_not_change_identity() {
    for imports in [
        "import \"nested/rooms.spk\" as a\nimport \"common.spk\" as b",
        "import \"common.spk\" as b\nimport \"nested/rooms.spk\" as a",
    ] {
        let main = root(imports, "let v: b::Room = a::make() print_i32(v.x)");
        let work = fixture(&[
            ("game.spk", &main),
            ("common.spk", "struct Room { x: i32 }"),
            (
                "nested/rooms.spk",
                "import \"../common.spk\" as same\nfn make() -> same::Room { return same::Room { x: 6 } }",
            ),
        ]);
        let checked = speck::analyze_path(&work.path().join("game.spk")).unwrap();
        assert_eq!(checked.ast().structs[0].name, "common.spk::Room");
        assert_eq!(checked.sources().files().count(), 3);
    }
}

#[test]
fn modules_are_isolated_and_do_not_reexport_imports() {
    let imported = "fn helper() -> i32 { return secret }";
    let main = root(
        "import \"helper.spk\" as helper\nconst secret: i32 = 42",
        "",
    );
    let failure = error(&[("game.spk", &main), ("helper.spk", imported)]);
    assert!(failure.to_string().contains("helper.spk:1:"));
    assert!(failure.to_string().contains("unknown variable `secret`"));
    for expression in [
        "print_i32(secret)",
        "print_i32(helper::missing)",
        "helper::nested::run()",
    ] {
        let main = root("import \"helper.spk\" as helper", expression);
        let failure = error(&[
            ("game.spk", &main),
            ("helper.spk", "const secret: i32 = 42"),
        ]);
        assert!(!failure.diagnostics.is_empty());
    }
    let main = root("import \"a.spk\" as a", "print_i32(a::nested)");
    let failure = error(&[
        ("game.spk", &main),
        ("a.spk", "import \"b.spk\" as nested"),
        ("b.spk", "const C: i32 = 1"),
    ]);
    assert!(failure.to_string().contains("has no declaration `nested`"));
}

#[test]
fn distinct_same_named_types_are_nominal_and_diagnostics_use_readable_names() {
    let main = root(
        "import \"a.spk\" as a\nimport \"b.spk\" as b",
        "let value: a::Room = b::Room { x: 1 }",
    );
    let failure = error(&[
        ("game.spk", &main),
        ("a.spk", "struct Room { x: i32 }"),
        ("b.spk", "struct Room { x: i32 }"),
    ]);
    let rendered = failure.to_string();
    assert!(rendered.contains("a.spk::Room"), "{rendered}");
    assert!(rendered.contains("b.spk::Room"), "{rendered}");
    assert!(!rendered.contains("spk_struct"));
}

#[test]
fn missing_imports_and_cycles_keep_sources_and_dependency_paths() {
    let main = root("import \"rooms.spk\" as rooms", "");
    let failure = error(&[("game.spk", &main)]);
    assert!(failure.to_string().contains("game.spk:2:1"));
    assert!(
        failure
            .sources
            .dependencies()
            .any(|path| path.ends_with("rooms.spk"))
    );
    assert_eq!(failure.sources.files().count(), 1);
    let failure = error(&[
        ("game.spk", &main),
        ("rooms.spk", "import \"./game.spk\" as root"),
    ]);
    assert!(failure.to_string().contains("import cycle:"));
    assert!(failure.to_string().contains("rooms.spk:1:1"));
}

#[test]
fn imports_and_imported_declarations_have_clear_boundaries() {
    let main = root("import \"module.spk\" as module", "");
    for source in [
        "game \"Other\"",
        "start {}",
        "update(dt: f32) {}",
        "draw {}",
    ] {
        let failure = error(&[("game.spk", &main), ("module.spk", source)]);
        assert!(
            failure
                .to_string()
                .contains("imported files cannot declare")
        );
    }
    for imports in [
        "import \"module.spk\" as module\nimport \"module.spk\" as module",
        "import \"module.spk\" as module\nconst module: i32 = 1",
        "import \"module.spk\" as KEY_A",
        "import \"module.spk\"",
    ] {
        let failure = error(&[("game.spk", &root(imports, "")), ("module.spk", "")]);
        assert!(!failure.diagnostics.is_empty());
    }
    let failure = error(&[
        ("game.spk", &main),
        ("module.spk", "fn print_i32() -> void {}"),
    ]);
    assert!(
        failure
            .to_string()
            .contains("conflicts with a predefined name")
    );
}

#[test]
fn errors_in_imported_sources_preserve_file_line_and_unicode_columns() {
    let main = root("import \"room data.spk\" as rooms", "");
    for source in [
        "// é\nconst X: i32 = true",
        "// é\nconst X: i32 = @",
        "// é\nfn broken( -> void {}",
    ] {
        let failure = error(&[("game.spk", &main), ("room data.spk", source)]);
        assert!(
            failure.to_string().contains("room data.spk:2:"),
            "{failure}"
        );
        let diagnostic = &failure.diagnostics[0];
        let (path, line, _) = failure.sources.location(diagnostic.span).unwrap();
        assert!(path.ends_with("room data.spk"));
        assert_eq!(line, 2);
    }
}

#[test]
fn check_loads_imports_with_no_native_tools_and_source_string_api_rejects_them() {
    let main = root("import \"rooms.spk\" as rooms", "print_i32(rooms::COUNT)");
    let failure = speck::analyze(&main).unwrap_err();
    assert!(failure[0].message.contains("imports require a file path"));
    let work = fixture(&[("game.spk", &main), ("rooms.spk", "const COUNT: i32 = 2")]);
    let output = support::run(
        Command::new(env!("CARGO_BIN_EXE_speck"))
            .current_dir(work.path())
            .env("PATH", "")
            .args(["check", "game.spk"]),
    );
    support::assert_success("module check without tools", &output);
    assert!(!work.path().join("build").exists());
    let program = speck::analyze_path(&work.path().join("game.spk")).unwrap();
    let paths = program
        .sources()
        .dependencies()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    assert!(paths.iter().any(|path| path.ends_with("rooms.spk")));
}

#[test]
fn symlink_imports_share_the_canonical_module_and_keep_both_dependency_paths() {
    let main = root(
        "import \"model.spk\" as first\nimport \"alias.spk\" as second",
        "let value: first::Room = second::Room { x: 4 }",
    );
    let work = fixture(&[("game.spk", &main), ("model.spk", "struct Room { x: i32 }")]);
    std::os::unix::fs::symlink("model.spk", work.path().join("alias.spk")).unwrap();
    let checked = speck::analyze_path(&work.path().join("game.spk")).unwrap();
    assert_eq!(checked.ast().structs.len(), 1);
    assert_eq!(checked.sources().files().count(), 2);
    for filename in ["model.spk", "alias.spk"] {
        assert!(
            checked
                .sources()
                .dependencies()
                .any(|path| path.ends_with(filename))
        );
    }
}

#[test]
fn import_depth_and_expression_depth_have_independent_stack_budgets() {
    const CHILD: &str = "SPECK_MODULE_DEPTH_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = support::run(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "import_depth_and_expression_depth_have_independent_stack_budgets",
                    "--nocapture",
                ])
                .env(CHILD, "1"),
        );
        support::assert_success("module depth on an ordinary test thread stack", &output);
        return;
    }
    let work = fixture(&[("game.spk", &root("import \"1.spk\" as next", ""))]);
    for index in 1..127 {
        fs::write(
            work.path().join(format!("{index}.spk")),
            format!("import \"{}.spk\" as next", index + 1),
        )
        .unwrap();
    }
    let last = work.path().join("127.spk");
    fs::write(
        &last,
        format!("const VALUE: i32 = {}1{}", "(".repeat(79), ")".repeat(79)),
    )
    .unwrap();
    let checked = speck::analyze_path(&work.path().join("game.spk")).unwrap();
    assert_eq!(checked.sources().files().count(), 128);
    speck::codegen::llvm::emit(&checked);
    fs::write(
        &last,
        format!(
            "struct S {{ x: i32 }}\nconst VALUE: S = {}1{}",
            "S { x: ".repeat(512),
            "}".repeat(512)
        ),
    )
    .unwrap();
    let failure = speck::analyze_path(&work.path().join("game.spk")).unwrap_err();
    assert!(failure.to_string().contains("source nesting exceeds limit"));
    fs::write(&last, "import \"128.spk\" as next").unwrap();
    fs::write(work.path().join("128.spk"), "").unwrap();
    let failure = speck::analyze_path(&work.path().join("game.spk")).unwrap_err();
    assert!(
        failure
            .to_string()
            .contains("import nesting exceeds the limit of 128 files")
    );
}

#[test]
fn identical_diagnostics_in_different_files_keep_distinct_source_locations() {
    let main = root("import \"a.spk\" as a\nimport \"b.spk\" as b", "");
    let same = "fn f() -> void { print_i32(true) }";
    let failure = error(&[("game.spk", &main), ("a.spk", same), ("b.spk", same)]);
    assert_eq!(failure.diagnostics.len(), 2, "{failure}");
    assert_eq!(
        failure.diagnostics[0].message,
        failure.diagnostics[1].message
    );
    assert_ne!(
        failure.diagnostics[0].span.source,
        failure.diagnostics[1].span.source
    );
    assert!(failure.to_string().contains("a.spk:1:"));
    assert!(failure.to_string().contains("b.spk:1:"));
}

#[test]
fn import_depth_limit_is_independent_of_cached_dependency_order() {
    for module_count in [127, 128, 129] {
        let work = fixture(&[]);
        for index in 1..=module_count {
            let source = if index == module_count {
                String::new()
            } else {
                format!("import \"{}.spk\" as next", index + 1)
            };
            fs::write(work.path().join(format!("{index}.spk")), source).unwrap();
        }
        for descending in [false, true] {
            let mut order = (1..=module_count).collect::<Vec<_>>();
            if descending {
                order.reverse();
            }
            let imports = order
                .into_iter()
                .map(|index| format!("import \"{index}.spk\" as module_{index}"))
                .collect::<Vec<_>>()
                .join("\n");
            let path = work.path().join("game.spk");
            fs::write(&path, root(&imports, "")).unwrap();
            let result = speck::analyze_path(&path);
            if module_count == 127 {
                let checked =
                    result.unwrap_or_else(|failure| panic!("descending={descending}: {failure}"));
                assert_eq!(checked.sources().files().count(), 128);
            } else {
                let failure = result.err().unwrap_or_else(|| {
                    panic!(
                        "{}-file path accepted, descending={descending}",
                        module_count + 1
                    )
                });
                assert!(
                    failure
                        .to_string()
                        .contains("import nesting exceeds the limit of 128 files"),
                    "{failure}"
                );
                assert_eq!(failure.diagnostics.len(), 1);
                let source = failure
                    .sources
                    .get(failure.diagnostics[0].span.source)
                    .unwrap();
                assert!(source.text()[failure.diagnostics[0].span.start..].starts_with("import "));
            }
        }
    }
}

#[test]
fn imported_array_functions_preserve_shared_types_context_and_value_copies() {
    let main = root(
        "import \"shape data/étage.spk\" as model\nimport \"operations.spk\" as operations",
        "let original: [model::Cell; model::COUNT] = model::make()\n\
         let copy: [model::Cell; model::COUNT] = operations::moved(original)\n\
         print_i32(original[0].value)\n\
         print_i32(copy[0].value)\n\
         print_i32(operations::moved([model::Cell { value: 7 }, model::Cell { value: 8 }])[1].value)\n\
         let grid: [[model::Cell; model::COUNT]; 1] = operations::nested()\n\
         print_i32(grid[0][1].value)\n\
         print_i32(operations::echo([-2147483648])[0])",
    );
    let work = fixture(&[
        ("game.spk", &main),
        (
            "shape data/étage.spk",
            "const COUNT: i32 = 2\nstruct Cell { value: i32 }\n\
         const CELLS: [Cell; COUNT] = [Cell { value: 1 }, Cell { value: 2 }]\n\
         fn make() -> [Cell; COUNT] { return CELLS }",
        ),
        (
            "operations.spk",
            "import \"shape data/étage.spk\" as cells\n\
         fn moved(values: [cells::Cell; cells::COUNT]) -> [cells::Cell; cells::COUNT] {\n\
             values[0].value += 10 return values\n\
         }\n\
         fn nested() -> [[cells::Cell; cells::COUNT]; 1] {\n\
             return [[cells::Cell { value: 30 }, cells::Cell { value: 40 }]]\n\
         }\n\
         fn echo(values: [i32; 1]) -> [i32; 1] { return values }",
        ),
    ]);
    native(work.path(), "1\n11\n8\n40\n-2147483648\n");
}

#[test]
fn nonconstant_references_in_imported_initializers_keep_their_use_span() {
    for expression in ["helper", "helper + 1", "i32(f32(helper))"] {
        for length_use in ["", "struct Sized { items: [i32; VALUE] }"] {
            let main = root("import \"lib.spk\" as library", "");
            let library = format!(
                "fn helper() -> i32 {{ return 1 }}\nconst VALUE: i32 = {expression}\n{length_use}"
            );
            let failure = error(&[("game.spk", &main), ("lib.spk", &library)]);
            assert!(!failure.diagnostics.is_empty());
            for diagnostic in &failure.diagnostics {
                let source = failure.sources.get(diagnostic.span.source).unwrap();
                assert!(source.path().ends_with("lib.spk"), "{failure}");
                assert_eq!(source.location(diagnostic.span.start).0, 2, "{failure}");
                assert_eq!(
                    &source.text()[diagnostic.span.start..diagnostic.span.end],
                    "helper",
                    "{failure}"
                );
            }
        }
    }
}
