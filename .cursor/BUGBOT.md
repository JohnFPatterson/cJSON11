# Bugbot rules for the cJSON C-to-Rust port

## Unsafe code

- `unsafe` is allowed only in `cjson-ffi` (and its tests).
- `cjson-core` must keep `#![forbid(unsafe_code)]`.
- Every `unsafe` block in `cjson-ffi` needs an adjacent `SAFETY:` comment stating the invariant.

## Parity exceptions

- A `PE-NNN` row in `PARITY_EXCEPTIONS.md` requires a matching `#[test]` in `cjson-core/tests/exceptions.rs`.
- A `CH-NNN` row in `MIGRATION.md` requires a pinning test named on the row.
- Do not change observable C behavior without an approved exception ID.

## FFI surface

- `cjson-ffi` must export every `CJSON_PUBLIC` function in `cJSON.h`, except functions named out of scope in `MIGRATION.md`.
- Do not change C ABI signatures to be more idiomatic.

## Parity gate

- Do not loosen `.cursor/parity.json` fixtures, oracle commands, or a ready module's `driver_args` / `fixtures`.
- Do not edit pinned C sources or fixtures to make the gate pass.
- Do not flip a ready module back to `"ready": false`.

## Out of scope

- `cJSON_Utils` and the fuzzer harnesses are out of scope for this port; do not silently omit or partially implement them without updating `MIGRATION.md`.
