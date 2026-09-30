# Oracle / Rust driver report format

Both `build/oracle` (C) and `target/release/cjson-driver` (Rust/`cjson-core`) print the same text for a fixture path argument. Sections may be selected with `--sections <name>[,<name>…]` where names are `parse`, `print`, and `tree`. Without `--sections`, all three are printed in that order.

Exact layout (no trailing spaces; final newline after the last section):

```
=== parse ===
status: ok
error_offset: -
=== print ===
compact:
<compact JSON bytes, or empty if parse failed>
end_compact
pretty:
<pretty JSON bytes, or empty if parse failed>
end_pretty
=== tree ===
array_size: <n or ->
compare_self: <same|diff|skipped>
minify:
<minified form of the file contents (always attempted)>
end_minify
```

Rules:

- `status` is `ok` or `fail`.
- `error_offset` is a decimal byte offset into the input on failure, or `-` on success. It matches `cJSON_GetErrorPtr() - input`.
- Compact uses `cJSON_PrintUnformatted`; pretty uses `cJSON_Print`.
- `array_size` is `cJSON_GetArraySize` on the root when parse succeeded, else `-`.
- `compare_self` is `cJSON_Compare(root, root, 1)` when parse succeeded, except when the estimated compare cost exceeds 1_000_000 steps, in which case both drivers print `skipped` (decision from the input tree shape only, never timing).
- `minify` copies the raw file bytes into a mutable buffer and runs `cJSON_Minify` (even when parse fails).
