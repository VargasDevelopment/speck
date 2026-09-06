pub mod support;

use std::process::Command;
use std::time::Duration;

#[test]
fn excessive_nesting_reports_diagnostics_without_aborting() {
    const CHILD: &str = "SPECK_NESTING_TEST_CHILD";
    if std::env::var_os(CHILD).is_some() {
        for shape in ["parentheses", "unary", "binary"] {
            for levels in [80, 81] {
                let expression = match shape {
                    "parentheses" => {
                        format!("{}1{}", "(".repeat(levels - 1), ")".repeat(levels - 1))
                    }
                    "unary" => format!("{}1", "-".repeat(levels - 1)),
                    "binary" => format!("{}1", "1 + ".repeat(levels - 1)),
                    _ => unreachable!(),
                };
                let source = format!(
                    "game \"Boundary\"\nconst VALUE: i32 = {expression}\nstart {{}}\nupdate(dt: f32) {{}}\ndraw {{}}"
                );
                if levels == 80 {
                    speck::compile_to_llvm(&source)
                        .unwrap_or_else(|errors| panic!("{shape} at the limit: {errors:#?}"));
                } else {
                    rejects(shape, &source);
                }
            }
        }
        // Test harness threads have ordinary stacks, unlike a CLI main thread
        // whose larger stack can hide a library/compiler abort.
        let expressions = [
            (
                "parentheses",
                format!("{}1{}", "(".repeat(512), ")".repeat(512)),
            ),
            ("unclosed parentheses", format!("{}1", "(".repeat(512))),
            ("unary", format!("{}1", "-".repeat(2_000))),
            ("binary", format!("{}1", "1 + ".repeat(10_000))),
            ("postfix fields", format!("value{}", ".x".repeat(10_000))),
            ("postfix indices", format!("value{}", "[0]".repeat(2_000))),
            (
                "calls",
                format!("{}1{}", "i32(".repeat(512), ")".repeat(512)),
            ),
            (
                "array literals",
                format!("{}1{}", "[".repeat(512), "]".repeat(512)),
            ),
            (
                "struct literals",
                format!("{}1{}", "S { x: ".repeat(512), "}".repeat(512)),
            ),
        ];
        for (name, expression) in expressions {
            rejects(
                name,
                &format!(
                    "game \"Depth\"\nstruct S {{ x: i32 }}\nstart {{ let value: i32 = {expression} }}\nupdate(dt: f32) {{}}\ndraw {{}}"
                ),
            );
        }
        let ty = format!("{}i32{}", "[".repeat(512), "; 1]".repeat(512));
        rejects(
            "array types",
            &format!(
                "game \"Depth\"\nstruct S {{ x: {ty} }}\nstart {{}}\nupdate(dt: f32) {{}}\ndraw {{}}"
            ),
        );
        let blocks = "if true {".repeat(512) + "print_i32(1)" + &"}".repeat(512);
        rejects(
            "blocks",
            &format!("game \"Depth\"\nstart {{ {blocks} }}\nupdate(dt: f32) {{}}\ndraw {{}}"),
        );
        let mixed = "if true {".repeat(40)
            + "let x: i32 = "
            + &"(".repeat(64)
            + "1"
            + &")".repeat(64)
            + &"}".repeat(40);
        rejects(
            "mixed nesting",
            &format!("game \"Depth\"\nstart {{ {mixed} }}\nupdate(dt: f32) {{}}\ndraw {{}}"),
        );

        // Wide arrays and long declaration lists do not have deep syntax trees.
        let wide = vec!["1"; 2_000].join(",");
        speck::analyze(&format!("game \"Wide\"\nconst ITEMS: [i32; 2000] = [{wide}]\nstart {{}}\nupdate(dt: f32) {{}}\ndraw {{}}"))
            .expect("width is not nesting");
        return;
    }

    let output = support::run_with_timeout(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "excessive_nesting_reports_diagnostics_without_aborting",
                "--nocapture",
            ])
            .env(CHILD, "1"),
        Duration::from_secs(10),
    );
    support::assert_success("nesting regression subprocess", &output);
}

fn rejects(name: &str, source: &str) {
    let errors = speck::compile_to_llvm(source).expect_err(name);
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("source nesting exceeds limit")),
        "{name}: {errors:#?}"
    );
    for error in errors {
        assert!(
            error.span.start < error.span.end && error.span.end <= source.len(),
            "{name}: {error:#?}"
        );
        assert!(
            source.is_char_boundary(error.span.start) && source.is_char_boundary(error.span.end)
        );
    }
}
