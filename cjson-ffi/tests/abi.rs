//! Pointer-level ABI smoke tests for `cjson-ffi`.
//! These are documentation-level checks; full link tests need the C header.

#[test]
fn version_string_is_1_7_19() {
    assert_eq!(cjson_core::version(), "1.7.19");
}
