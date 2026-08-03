//! Compile-fail tests for the guarantees `obj` makes at compile time.
//!
//! Diagnostics drift between compiler versions, so these are skipped on the MSRV and Miri jobs.

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
