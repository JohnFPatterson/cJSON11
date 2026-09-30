# Parity report: cJSON C vs Rust

## Result

**11 fixtures, 11 identical, 0 exceptions, 0 divergences** (full gate, all modules ready).

## Method

Compared stdout and exit status of:

- C oracle: `./build/oracle` (`tools/cjson-oracle.c` + unmodified `cJSON.c`)
- Rust driver: `./target/release/cjson-driver` (`cjson-core`)

Report format: [`tools/DRIVER_FORMAT.md`](tools/DRIVER_FORMAT.md).

Reproduce:

```sh
make parity
# or
printf '%s' '{"status":"completed","loop_count":0,"workspace_roots":["'"$PWD"'"]}' \
  | python3 tools/parity_gate.py --force
```

## Gate report (paste)

From `build/parity-gate/parity-report.md` after a successful full run:

```
PASS: 11 identical, 0 exceptions, 0 divergences, 11 total
```

Per-fixture: `test1`–`test11` under `tests/inputs/` all `identical` for modules `parse`, `print`, `tree`, and the full compare.

## Behavior kept on purpose (quirks)

| Quirk | C source | Notes |
|-------|----------|-------|
| Case-insensitive object lookup | [`cJSON.c:133`](cJSON.c) `case_insensitive_strcmp` | First matching key wins; duplicates kept |
| `valueint` saturation | [`cJSON.c:389-399`](cJSON.c) | Clamp to `INT_MAX` / `INT_MIN` |
| Non-finite numbers print as `null` | [`cJSON.c:611-614`](cJSON.c) | NaN / Inf |
| Number print `%d` / `%1.15g` / `%1.17g` | [`cJSON.c:616-629`](cJSON.c) | Matched via `cjson-cfmt` → glibc `snprintf` |
| Nesting limit 1000 | [`cJSON.h:136-137`](cJSON.h) | Parse/print reject deeper trees |
| Circular duplicate limit 10000 | [`cJSON.h:140-143`](cJSON.h) / [`cJSON.c:2836`](cJSON.c) | |
| Global parse error cursor | [`cJSON.c:92-97`](cJSON.c) | Offset into caller buffer |
| Pretty print tabs | [`cJSON.c:1814-1825`](cJSON.c) | `\t` indentation, `:\t` after keys |

## Behavior changed on purpose

None recorded yet. Hook-trace allocation differences are listed under **Proposed CH-NNN (awaiting approval)** in [`MIGRATION.md`](MIGRATION.md) and are **not** applied as approved rows.

## Divergences

None.

## Known gaps

- `cJSON_Compare` cost bound: both drivers print `compare_self: skipped` when `estimate_compare_cost` exceeds 1_000_000 (input-derived, not timed).
- `cJSON_Utils` and fuzzers are out of scope (named in `MIGRATION.md`).
- SonarQube MCP was unavailable (`needsAuth` / auth timeout); `PARITY_EXCEPTIONS.md` has no `PE-NNN` rows yet.

## Oracle defect found

None on this revision. ASan over all 11 fixtures with `gcc -fsanitize=address,undefined` printed nothing:

```sh
gcc -g -fsanitize=address,undefined -fno-omit-frame-pointer -I. \
  -o build/asan/oracle tools/cjson-oracle.c cJSON.c -lm
for f in tests/inputs/test{1..11}; do
  ASAN_OPTIONS=detect_leaks=0 ./build/asan/oracle "$f" >/dev/null || echo "ASAN: $f"
done
# (no ASAN: lines)
```

`cJSON_strdup` in this tree copies `strlen + 1` bytes ([`cJSON.c:197-204`](cJSON.c)).

## AddressSanitizer

Passed for all listed fixtures (see above). Hook-trace / Unity ASan runs are recommended once hook-trace parity is approved.
