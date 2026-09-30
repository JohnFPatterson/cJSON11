//! One `#[test]` per `PE-NNN` row in PARITY_EXCEPTIONS.md.
//! Currently empty: no Sonar SECURITY exceptions have been approved yet.

#[test]
fn exceptions_table_is_empty_until_sonar_approval() {
    // Placeholder so `cargo test -p cjson-core --test exceptions` succeeds
    // while PARITY_EXCEPTIONS.md has no rows.
    let rows = include_str!("../../PARITY_EXCEPTIONS.md");
    assert!(
        rows.contains("*(none yet)*"),
        "add a #[test] for each new PE-NNN row"
    );
}
