#[test]
fn crumb_bum_ir_matches_golden_file() {
    let source = include_str!("../examples/crumb_bum.spk");
    let actual = speck::compile_to_llvm(source).expect("example should compile to LLVM IR");
    let expected = include_str!("golden/crumb_bum.ll");
    assert_eq!(actual, expected);
}
