// SPDX-License-Identifier: EUPL-1.2

//! compile-fail coverage for the derive's error paths.

#![cfg(feature = "derive")]

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
