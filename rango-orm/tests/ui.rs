//! Compiler-diagnostic tests for the derive and the core traits.
//! Regenerate expectations with `TRYBUILD=overwrite cargo test -p rango-orm --test ui`.

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/pass_model.rs");
    t.compile_fail("tests/ui/unsupported_field_type.rs");
    t.compile_fail("tests/ui/unsupported_bytes.rs");
    t.compile_fail("tests/ui/missing_derive.rs");
}
