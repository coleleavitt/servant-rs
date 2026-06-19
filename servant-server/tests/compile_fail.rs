#[test]
fn public_api_misuse_fails_to_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
