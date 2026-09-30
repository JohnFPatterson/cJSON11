# FFI Unity runner (follow-up)

Public-API Unity suites should link `target/release/libcjson_ffi.a` instead of compiling `cJSON.c` through `tests/common.h`.

White-box suites that call `static` internals must remain C-only; their cases are ported to `cjson-core::internals` under the same test names.

Removed-from-FFI case list (exact names) will live in `MIGRATION.md` when the runner lands. Until then, use `make` / CMake `enable_testing` against the original C library (`make` in a CMake build directory with `-DENABLE_CJSON_TEST=ON`).
