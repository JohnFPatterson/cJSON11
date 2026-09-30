# Migration: cJSON → Rust

## Layout

| Path | Role |
|------|------|
| `cjson-core/` | All parse/print/tree logic; `#![forbid(unsafe_code)]` |
| `cjson-cfmt/` | Thin libc `snprintf`/`strtod` boundary for `%g` / number parse parity |
| `cjson-ffi/` | C ABI only (`staticlib` + `cdylib`); every `unsafe` has `SAFETY:` |
| `cJSON.c` / `cJSON.h` | Unmodified oracle / public header |
| `tools/cjson-oracle.c` | C driver (public API) |
| `tools/cjson-driver/` | Rust driver over `cjson-core` |
| `tools/hook-trace.c` | Allocator hook-trace over public API |
| `tools/parity_gate.py` | Local stand-in for the skill’s parity gate (bundled hook was absent on this VM) |
| `.cursor/parity.json` | Gate config; modules `parse` / `print` / `tree` |

Rust callers use `cjson-core`. C callers link `cjson-ffi` and keep `#include "cJSON.h"`.

## Out of scope

- **`cJSON_Utils`** ([`cJSON_Utils.c`](cJSON_Utils.c) / [`cJSON_Utils.h`](cJSON_Utils.h), 14 `CJSON_PUBLIC` functions) and Unity suites `json_patch_tests`, `old_utils_tests`, `misc_utils_tests`, plus [`tests/json-patch-tests/`](tests/json-patch-tests/).
- **Fuzzing** ([`fuzzing/`](fuzzing/)).

Silent omission of these would be a bug; they are deferred, not forgotten.

## Unsafe audit

```sh
grep -rn "unsafe" --include='*.rs' --exclude-dir=target
```

- `cjson-core`: only `forbid(unsafe_code)` (no executable `unsafe`).
- `cjson-cfmt`: libc calls in a dedicated boundary crate (`snprintf`, `sscanf`, `strtod`).
- `cjson-ffi`: ABI reconstitution; `#![deny(unsafe_op_in_unsafe_fn)]` and `#![deny(clippy::undocumented_unsafe_blocks)]`.

Compiler enforcement: `cargo clippy --workspace --all-targets -- -D warnings`.

## Export check

```sh
make export-check
# header functions: 78
# export-check: ok
```

## Behavior kept on purpose

See [`PARITY.md`](PARITY.md). Quirks (case folding, `%g`, duplicate keys, nesting limits) are preserved.

## Behavior changed on purpose (approved)

| ID | Change | C behavior | Rust behavior | Why | Approval | Pinning test |
|----|--------|------------|---------------|-----|----------|--------------|
| *(none yet)* | | | | | | |

## Proposed CH-NNN (awaiting approval)

Hook-trace (`make build/hook-trace-c build/hook-trace-ffi` then diff) shows allocation-size and fail-on-Nth differences. Typical deltas:

1. **Key/string allocation sizes** — C sometimes allocates overestimated string buffers during parse; Rust/`cjson-ffi` stores exact `strlen+1` after converting from `cjson-core`.
2. **Print buffer strategy** — C starts a 256-byte print buffer and grows; FFI prints via core into a Rust `String` then one `cstr_dup`, so intermediate `M 256` / `F 256` pairs do not appear.
3. **Fail-on-Nth allocation** — because allocation counts differ, which call fails under `fail_after=N` diverges.

These are **not** logged as approved `CH-NNN` rows until you approve them (or we replay C’s sizes/order in FFI). Until then, `make hook-trace` is expected to fail the diff.

## Unity test split

[`tests/common.h`](tests/common.h) `#include`s `../cJSON.c`, so suites are white-box today.

| Suite class | Suites | Plan |
|-------------|--------|------|
| White-box (`static` internals) | `parse_number`, `parse_hex4`, `parse_string`, `parse_array`, `parse_object`, `parse_value`, `print_*`, plus selected `misc_tests` cases (`ensure_*`, `skip_utf8_bom_*`, aliased-key UAF) | Run against C only (`make`/CMake). Port case-for-case to `cjson-core::internals`. |
| Public API | `parse_examples`, `parse_with_opts`, `cjson_add`, `compare_tests`, `readme_examples`, `minify_tests`, remaining `misc_tests` | Link against `libcjson_ffi.a` via a stand-in that includes `cJSON.h` + helpers (`tools/ffi-unity/`, follow-up). |
| Utils | `json_patch_tests`, `old_utils_tests`, `misc_utils_tests` | **Out of scope** with Utils. |

`make c-unity` (CMake) keeps the original C suite runnable. FFI Unity runner is scaffolding for a follow-up once hook-trace policy is decided.

## Parity gate stand-in

`tools/parity_gate.py` is copied to `~/.cursor/hooks/c-rust-parity/parity_gate.py` when run. It implements pins, `--module`, exception-row checks, and reports under `build/parity-gate/`. It is a local stand-in because the skill’s bundled gate was not present on this VM.

## Commands

```sh
make oracle rust-driver   # build both drivers
make parity               # full gate
make export-check         # 78 header symbols vs FFI
cargo test --workspace --target-dir target
cargo clippy --workspace --all-targets --target-dir target -- -D warnings
cargo fmt --all --check
```
