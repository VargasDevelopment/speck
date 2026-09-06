pub mod support;

use std::fs;
use std::process::Command;

const LIMIT_ERROR: &str = "resolved constant aggregate nesting exceeds limit of 128";
const ENTRIES: &str = "start {} update(dt: f32) {} draw {}";

#[derive(Clone, Copy, Debug)]
enum Context {
    Constant,
    Global,
    ArrayLength,
}

#[test]
fn resolved_aggregate_budget_bounds_clone_drop_and_emission_on_small_stacks() {
    const CHILD: &str = "SPECK_AGGREGATE_DEPTH_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = support::run(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "resolved_aggregate_budget_bounds_clone_drop_and_emission_on_small_stacks",
                    "--nocapture",
                ])
                .env(CHILD, "1"),
        );
        support::assert_success("constant aggregate depth on a 2 MiB worker", &output);
        return;
    }
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            for imported in [false, true] {
                for array_parent in [false, true] {
                    for context in [Context::Constant, Context::Global, Context::ArrayLength] {
                        for depth in [128, 129] {
                            check_boundary(imported, array_parent, context, depth);
                        }
                    }
                }
            }
            // Every file and initializer stays shallow, but their resolved values
            // would otherwise form 2,048 nested records before cloning/formatting.
            let source = imported_chain(&vec![1; 128 * 16], "");
            let failure = speck::analyze_path(&source.path().join("0.spk")).unwrap_err();
            assert!(failure.to_string().contains(LIMIT_ERROR), "{failure}");
        })
        .unwrap()
        .join()
        .unwrap();
}

fn check_boundary(imported: bool, array_parent: bool, context: Context, depth: usize) {
    let mut remaining = depth - 1;
    let mut levels = Vec::new();
    while remaining > 0 {
        let level = if array_parent && remaining >= 2 { 2 } else { 1 };
        levels.push(level);
        remaining -= level;
    }
    let value = if imported { "VALUE" } else { "V0" };
    let declaration = if matches!(context, Context::Global) {
        "let"
    } else {
        "const"
    };
    let output = if array_parent {
        format!("{declaration} OUTPUT: [S0; 1] = [{value}]\n")
    } else {
        format!(
            "struct Wrapper {{ tag: i32 item: S0 }}\n{declaration} OUTPUT: Wrapper = Wrapper {{ tag: 1, item: {value} }}\n"
        )
    };
    let output = if matches!(context, Context::ArrayLength) {
        let selected = if array_parent {
            "OUTPUT[0].tag"
        } else {
            "OUTPUT.tag"
        };
        format!(
            "{output}const SIZE: i32 = {selected}\nstruct UsesLength {{ items: [i32; SIZE] }}\n"
        )
    } else {
        output
    };
    let result = if imported {
        let source = imported_chain(&levels, &output);
        speck::analyze_path(&source.path().join("0.spk")).map_err(|failure| {
            for diagnostic in &failure.diagnostics {
                if diagnostic.message.contains(LIMIT_ERROR) {
                    let file = failure.sources.get(diagnostic.span.source).unwrap();
                    assert!(diagnostic.span.end > diagnostic.span.start);
                    assert!(
                        file.text()[diagnostic.span.start..diagnostic.span.end].contains(value),
                        "{failure}"
                    );
                }
            }
            failure.to_string()
        })
    } else {
        let source = same_file_chain(&levels, &output);
        speck::analyze(&source).map_err(|diagnostics| {
            speck::render_diagnostics(std::path::Path::new("same.spk"), &source, &diagnostics)
        })
    };
    if depth == 128 {
        let program = result
            .unwrap_or_else(|failure| panic!("{imported}, {array_parent}, {context:?}: {failure}"));
        // Accepted values must remain safe through emission and ordinary drop,
        // not just survive the evaluation cache.
        assert!(speck::codegen::llvm::emit(&program).contains("@spk_start"));
    } else {
        let failure = result.err().unwrap_or_else(|| {
            panic!("accepted depth {depth}: {imported}, {array_parent}, {context:?}")
        });
        assert!(failure.contains(LIMIT_ERROR), "{failure}");
    }
}

fn field_type(child: &str, depth: usize) -> String {
    if depth == 2 {
        format!("[{child}; 1]")
    } else {
        child.to_owned()
    }
}

fn field_value(child: &str, depth: usize) -> String {
    if depth == 2 {
        format!("[{child}]")
    } else {
        child.to_owned()
    }
}

fn same_file_chain(levels: &[usize], output: &str) -> String {
    let mut source = "game \"Same-file aggregate depth\"\n".to_owned();
    for (index, depth) in levels.iter().enumerate() {
        let (child_type, child_value) = if index + 1 == levels.len() {
            ("i32".into(), "1".into())
        } else {
            (format!("S{}", index + 1), format!("V{}", index + 1))
        };
        source.push_str(&format!("struct S{index} {{ tag: i32 item: {} }}\nconst V{index}: S{index} = S{index} {{ tag: 1, item: {} }}\n",
            field_type(&child_type, *depth), field_value(&child_value, *depth)));
    }
    source.push_str(output);
    source.push_str(ENTRIES);
    source
}

fn imported_chain(levels: &[usize], output: &str) -> tempfile::TempDir {
    let directory = support::workspace();
    let chunks = levels.chunks(16).collect::<Vec<_>>();
    for (index, chunk) in chunks.iter().enumerate() {
        let mut source = if index == 0 {
            "game \"Imported aggregate depth\"\n".to_owned()
        } else {
            String::new()
        };
        let has_next = index + 1 < chunks.len();
        if has_next {
            source.push_str(&format!("import \"{}.spk\" as next\n", index + 1));
        }
        for (local, depth) in chunk.iter().enumerate() {
            let child = if local + 1 < chunk.len() {
                format!("S{}", local + 1)
            } else if has_next {
                "next::S0".into()
            } else {
                "i32".into()
            };
            source.push_str(&format!(
                "struct S{local} {{ tag: i32 item: {} }}\n",
                field_type(&child, *depth)
            ));
        }
        let mut value = if has_next {
            "next::VALUE".to_owned()
        } else {
            "1".to_owned()
        };
        for (local, depth) in chunk.iter().enumerate().rev() {
            value = format!(
                "S{local} {{ tag: 1, item: {} }}",
                field_value(&value, *depth)
            );
        }
        source.push_str(&format!("const VALUE: S0 = {value}\n"));
        if index == 0 {
            source.push_str(output);
            source.push_str(ENTRIES);
        }
        fs::write(directory.path().join(format!("{index}.spk")), source).unwrap();
    }
    directory
}
