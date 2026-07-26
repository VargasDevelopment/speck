#[test]
fn crumb_bum_ir_matches_golden_file() {
    let source = include_str!("../examples/crumb_bum.spk");
    let actual = speck::compile_to_llvm(source).expect("example should compile to LLVM IR");
    let expected = include_str!("golden/crumb_bum.ll");
    assert_eq!(actual, expected);
}

#[test]
fn target_specific_ir_names_the_selected_host() {
    let source = include_str!("../examples/framebuffer_rect.spk");
    let actual = speck::compile_to_llvm_for_target(source, "arm64-apple-darwin")
        .expect("example should compile to target-specific LLVM IR");
    assert!(actual.contains("target triple = \"arm64-apple-darwin\""));
    assert!(!actual.contains("target datalayout"));
}
