use speck::ast::{ArrayLength, ConstantValue, ValueType};
use speck::{Resolution, analyze, analyze_path, lexer, parser};

fn game(header: &str, declarations: &str) -> String {
    format!(
        "game \"Resolution\" {header}\n{declarations}\nstart {{}} update(dt: f32) {{}} draw {{}}"
    )
}

#[test]
fn header_defaults_bounds_and_contextual_identifier() {
    for (header, width, height) in [
        ("", 320, 180),
        (";", 320, 180),
        ("resolution(640, 360);", 640, 360),
        ("resolution(1, 4096)", 1, 4096),
        ("resolution(4096, 1)", 4096, 1),
        ("resolution(333, 197)", 333, 197),
    ] {
        let checked = analyze(&game(
            header,
            "fn resolution() -> i32 { let resolution: i32 = 7 return resolution }",
        ))
        .unwrap();
        assert_eq!(
            checked.ast().resolution,
            Resolution::new(width, height).unwrap()
        );
    }
}

#[test]
fn rejects_bad_dimensions_at_the_dimension_span() {
    for (dimensions, bad, name) in [
        ("0, 180", "0", "width"),
        ("320, 0", "0", "height"),
        ("4097, 180", "4097", "width"),
        ("320, 65536", "65536", "height"),
        ("-1, 180", "-", "width"),
        ("320, -2", "-", "height"),
        ("1.5, 180", "1.5", "width"),
        ("320, true", "true", "height"),
        ("WIDTH, 180", "WIDTH", "width"),
    ] {
        let source = game(&format!("resolution({dimensions})"), "");
        let errors = analyze(&source).unwrap_err();
        assert!(
            errors[0].message.contains(&format!("resolution {name}")),
            "{errors:?}"
        );
        assert_eq!(&source[errors[0].span.start..errors[0].span.end], bad);
    }
}

#[test]
fn rejects_expressions_malformed_and_misplaced_headers() {
    for header in [
        "resolution(320 + 1, 180)",
        "resolution(320, 180 * 2)",
        "resolution(320 180)",
        "resolution(320,)",
        "resolution(320, 180, 1)",
        "resolution(320, 180",
        "resolution 320, 180)",
        "resolution(320, 180) resolution(640, 360)",
        "; resolution(640, 360)",
    ] {
        assert!(analyze(&game(header, "")).is_err(), "accepted {header}");
    }
    let source = game("", "const VALUE: i32 = 1\nresolution(640, 360)");
    let errors = analyze(&source).unwrap_err();
    assert!(
        errors[0]
            .message
            .contains("immediately after the game title")
    );
}

#[test]
fn screen_constants_fold_for_values_and_array_lengths() {
    for (header, width, height) in [("", 320, 180), ("resolution(640, 360)", 640, 360)] {
        let source = game(
            header,
            "const AREA: i32 = FRAMEBUFFER_WIDTH * FRAMEBUFFER_HEIGHT\nlet edge: i32 = FRAMEBUFFER_HEIGHT - 1\nstruct Row { pixels: [i32; FRAMEBUFFER_WIDTH] }\nfn width() -> i32 { return FRAMEBUFFER_WIDTH }",
        );
        let checked = analyze(&source).unwrap();
        assert_eq!(
            checked.ast().constants[0].value,
            Some(ConstantValue::I32(width * height))
        );
        assert_eq!(
            checked.ast().globals[0].value,
            Some(ConstantValue::I32(height - 1))
        );
        assert_eq!(
            checked.ast().structs[0].fields[0].ty,
            ValueType::Array {
                element: Box::new(ValueType::I32),
                length: ArrayLength::Resolved(width as usize)
            }
        );
    }
}

#[test]
fn screen_constants_cannot_be_redeclared_shadowed_or_assigned() {
    for source in [
        game("", "const FRAMEBUFFER_WIDTH: i32 = 1"),
        game("", "let FRAMEBUFFER_HEIGHT: i32 = 1"),
        game("", "fn FRAMEBUFFER_WIDTH() -> i32 { return 1 }"),
        game("", "struct FRAMEBUFFER_HEIGHT {}"),
        game("", "fn f(FRAMEBUFFER_WIDTH: i32) -> void {}"),
        game("", "fn f() -> void { let FRAMEBUFFER_HEIGHT: i32 = 1 }"),
        game("", "fn f() -> void { for FRAMEBUFFER_WIDTH in 0..1 {} }"),
        game("", "fn f() -> void { FRAMEBUFFER_HEIGHT = 1 }"),
    ] {
        let errors = analyze(&source).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("predefined constant")
                    || error.message.contains("cannot assign to constant")),
            "{errors:?}"
        );
    }
}

#[test]
fn imported_modules_use_the_entry_resolution() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("layout.spk"), "const AREA: i32 = FRAMEBUFFER_WIDTH * FRAMEBUFFER_HEIGHT\nstruct Column { pixels: [i32; FRAMEBUFFER_HEIGHT] }\nlet right: i32 = FRAMEBUFFER_WIDTH - 1\nfn bottom() -> i32 { return FRAMEBUFFER_HEIGHT - 1 }").unwrap();
    let path = dir.path().join("game.spk");
    for (header, width, height) in [("", 320, 180), ("resolution(640, 360)", 640, 360)] {
        std::fs::write(
            &path,
            game(
                header,
                "import \"layout.spk\" as layout\nconst AREA: i32 = layout::AREA",
            ),
        )
        .unwrap();
        let checked = analyze_path(&path).unwrap();
        for constant in &checked.ast().constants {
            assert_eq!(constant.value, Some(ConstantValue::I32(width * height)));
        }
        assert_eq!(
            checked.ast().globals[0].value,
            Some(ConstantValue::I32(width - 1))
        );
        assert_eq!(
            checked.ast().structs[0].fields[0].ty,
            ValueType::Array {
                element: Box::new(ValueType::I32),
                length: ArrayLength::Resolved(height as usize)
            }
        );
    }
}

#[test]
fn imported_headers_and_screen_name_conflicts_are_diagnosed_in_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("game.spk");
    std::fs::write(
        &path,
        game("resolution(640, 360)", "import \"layout.spk\" as layout"),
    )
    .unwrap();
    for (library, message) in [
        (
            "resolution(640, 360)",
            "imported files cannot declare resolution",
        ),
        (
            "game \"Library\" resolution(640, 360)",
            "imported files cannot declare `game`",
        ),
        (
            "const FRAMEBUFFER_WIDTH: i32 = 1",
            "conflicts with a predefined name",
        ),
    ] {
        std::fs::write(dir.path().join("layout.spk"), library).unwrap();
        let error = analyze_path(&path).unwrap_err();
        assert!(error.diagnostics[0].message.contains(message), "{error:?}");
        assert_ne!(
            error.diagnostics[0].span.source,
            speck::source::SourceId::DEFAULT
        );
    }
}

#[test]
fn parser_retains_resolution_before_semantic_checking() {
    let program = parser::parse(lexer::lex(&game("resolution(800, 600)", "")).unwrap()).unwrap();
    assert_eq!(program.resolution, Resolution::new(800, 600).unwrap());
}
