#!/usr/bin/env python3
"""Local stand-in for ~/.cursor/hooks/c-rust-parity/parity_gate.py.

The bundled skill template was not present on this VM. This script follows the
documented contract: pin oracle sources and fixtures, compare C/Rust driver
stdout (and optionally stderr), honor PARITY_EXCEPTIONS.md fixture rows, and
gate per-module readiness via .cursor/parity.json.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path
from typing import Any


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def find_repo_root(workspace: Path) -> Path:
    cfg = workspace / ".cursor" / "parity.json"
    if cfg.is_file():
        return workspace
    raise SystemExit(f"no .cursor/parity.json under {workspace}")


def state_path(repo: Path) -> Path:
    digest = sha256_bytes(str(repo.resolve()).encode())[:16]
    base = Path.home() / ".cursor" / "hooks" / "c-rust-parity" / "state"
    base.mkdir(parents=True, exist_ok=True)
    return base / f"{digest}.json"


def parse_exceptions(repo: Path) -> dict[str, str]:
    """Map fixture path -> test function name from PARITY_EXCEPTIONS.md."""
    path = repo / "PARITY_EXCEPTIONS.md"
    if not path.is_file():
        return {}
    out: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.startswith("|"):
            continue
        cols = [c.strip() for c in line.strip("|").split("|")]
        if len(cols) < 9:
            continue
        if cols[0] in {"ID", "----"} or cols[0].startswith("*(") or cols[0].startswith("---"):
            continue
        fixture = cols[8]
        test = cols[7]
        if fixture and fixture != "-" and test and test != "-":
            out[fixture] = test
    return out


def expand_fixtures(repo: Path, patterns: list[str]) -> list[Path]:
    files: list[Path] = []
    for pat in patterns:
        if any(ch in pat for ch in "*?[]"):
            files.extend(sorted(repo.glob(pat)))
        else:
            p = repo / pat
            if p.is_file():
                files.append(p)
    # Drop .expected companions; fixtures are the input files.
    files = [f for f in files if not f.name.endswith(".expected") and f.is_file()]
    # Stable unique
    seen = set()
    unique: list[Path] = []
    for f in files:
        key = f.resolve()
        if key not in seen:
            seen.add(key)
            unique.append(f)
    return unique


def run_cmd(
    argv: list[str],
    cwd: Path,
    timeout: float,
    input_path: Path | None = None,
) -> tuple[int, bytes, bytes]:
    cmd = []
    for a in argv:
        if a == "{input}":
            if input_path is None:
                raise SystemExit("command uses {input} but no fixture given")
            cmd.append(str(input_path))
        else:
            cmd.append(a)
    try:
        proc = subprocess.run(
            cmd,
            cwd=str(cwd),
            capture_output=True,
            timeout=timeout,
            check=False,
        )
        return proc.returncode, proc.stdout, proc.stderr
    except subprocess.TimeoutExpired as exc:
        return 124, exc.stdout or b"", exc.stderr or b"timeout\n"
    except FileNotFoundError as exc:
        return 127, b"", str(exc).encode() + b"\n"


def pin_or_check(
    state: dict[str, Any],
    repo: Path,
    oracle_sources: list[str],
    fixtures: list[Path],
    modules_cfg: dict[str, Any],
) -> list[str]:
    errors: list[str] = []
    pins = state.setdefault("pins", {})
    oracle_pins = pins.setdefault("oracle", {})
    fixture_pins = pins.setdefault("fixtures", {})
    module_pins = pins.setdefault("modules", {})

    first_oracle = not oracle_pins
    first_fixtures = not fixture_pins

    for rel in oracle_sources:
        path = repo / rel
        if not path.is_file():
            errors.append(f"missing oracle source: {rel}")
            continue
        digest = sha256_file(path)
        if rel not in oracle_pins:
            oracle_pins[rel] = digest
        elif oracle_pins[rel] != digest:
            errors.append(f"pinned oracle source changed: {rel}")

    fixture_rels = [str(f.relative_to(repo)) for f in fixtures]
    for rel in fixture_rels:
        path = repo / rel
        digest = sha256_file(path)
        if rel not in fixture_pins:
            fixture_pins[rel] = digest
        elif fixture_pins[rel] != digest:
            errors.append(f"pinned fixture changed: {rel}")

    # Deleted pinned fixtures fail
    for rel in list(fixture_pins):
        if not (repo / rel).is_file():
            errors.append(f"pinned fixture deleted: {rel}")

    for name, mod in modules_cfg.items():
        if not mod.get("ready"):
            continue
        definition = {
            "driver_args": mod.get("driver_args", []),
            "fixtures": mod.get("fixtures"),
        }
        digest = sha256_bytes(json.dumps(definition, sort_keys=True).encode())
        if name not in module_pins:
            module_pins[name] = digest
        elif module_pins[name] != digest:
            errors.append(
                f"ready module definition changed: {name} "
                f"(restore it or re-baseline the state file)"
            )

    # Detect ready flipped back to false after pin
    for name in list(module_pins):
        if name not in modules_cfg:
            errors.append(f"pinned ready module removed: {name}")
        elif not modules_cfg[name].get("ready"):
            errors.append(
                f"ready module flipped back to false: {name} "
                f"(restore ready:true or re-baseline)"
            )

    state["_pin_notes"] = []
    if first_oracle:
        state["_pin_notes"].append(
            f"oracle files pinned on this run ({len(oracle_pins)})"
        )
    if first_fixtures:
        state["_pin_notes"].append(
            f"fixtures pinned on this run ({len(fixture_pins)})"
        )
    return errors


def compare_fixture(
    repo: Path,
    cfg: dict[str, Any],
    fixture: Path,
    driver_args: list[str],
    exceptions: dict[str, str],
) -> dict[str, Any]:
    timeout = float(cfg.get("per_run_timeout_s", 30))
    c_cmd = list(cfg["c_cmd"]) + driver_args
    r_cmd = list(cfg["rust_cmd"]) + driver_args
    c_rc, c_out, c_err = run_cmd(c_cmd, repo, timeout, fixture)
    r_rc, r_out, r_err = run_cmd(r_cmd, repo, timeout, fixture)
    rel = str(fixture.relative_to(repo))
    compare_stderr = bool(cfg.get("compare_stderr", False))
    same = c_rc == r_rc and c_out == r_out and (not compare_stderr or c_err == r_err)
    result: dict[str, Any] = {
        "fixture": rel,
        "identical": same,
        "c_exit": c_rc,
        "rust_exit": r_rc,
        "c_stdout_len": len(c_out),
        "rust_stdout_len": len(r_out),
    }
    if same:
        result["status"] = "identical"
        return result

    # Exception path
    test_name = exceptions.get(rel)
    if test_name:
        # Verify exception test exists / is listed
        list_cmd = list(cfg.get("exceptions_test", []))
        if list_cmd:
            rc, out, err = run_cmd(list_cmd, repo, timeout)
            listing = (out + err).decode("utf-8", "replace")
            if f"{test_name}" in listing:
                result["status"] = "exception"
                result["exception_test"] = test_name
                return result
        result["status"] = "exception_missing_test"
        result["exception_test"] = test_name
    else:
        result["status"] = "diverge"

    # First differing byte
    limit = min(len(c_out), len(r_out))
    off = 0
    while off < limit and c_out[off] == r_out[off]:
        off += 1
    result["byte_offset"] = off
    result["c_hex"] = c_out[off : off + 16].hex()
    result["rust_hex"] = r_out[off : off + 16].hex()
    return result


def write_report(
    repo: Path,
    cfg: dict[str, Any],
    report: dict[str, Any],
    module_mode: bool,
) -> None:
    report_dir = repo / cfg.get("report_dir", "build/parity-gate")
    report_dir.mkdir(parents=True, exist_ok=True)
    stem = "parity-report.modules" if module_mode else "parity-report"
    (report_dir / f"{stem}.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    lines = [
        f"# Parity report ({'modules' if module_mode else 'full'})",
        "",
        f"**Result:** {report['result_line']}",
        "",
        f"**Method:** {report['method']}",
        "",
        f"**Reproduce:** `{report['reproduce']}`",
        "",
        "## Pins",
        "",
    ]
    for note in report.get("pin_notes", []):
        lines.append(f"- {note}")
    if not report.get("pin_notes"):
        lines.append("- No new pins this run.")
    lines.extend(["", "## Results", "", "| Fixture | Module | Status |", "|---|---|---|"])
    for row in report.get("results", []):
        lines.append(
            f"| {row.get('fixture','')} | {row.get('module','')} | {row.get('status','')} |"
        )
    if report.get("errors"):
        lines.extend(["", "## Errors", ""])
        for e in report["errors"]:
            lines.append(f"- {e}")
    (report_dir / f"{stem}.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--force", action="store_true")
    ap.add_argument("--module", action="append", default=[])
    ap.add_argument("--event", default="stop")
    args = ap.parse_args()

    raw = sys.stdin.read()
    try:
        payload = json.loads(raw) if raw.strip() else {}
    except json.JSONDecodeError:
        payload = {}
    roots = payload.get("workspace_roots") or [os.getcwd()]
    repo = find_repo_root(Path(roots[0]))
    cfg = load_json(repo / ".cursor" / "parity.json")
    if not cfg.get("enabled", True):
        print(json.dumps({"status": "skipped", "reason": cfg.get("reason", "")}))
        return 0

    state_file = state_path(repo)
    state: dict[str, Any] = {}
    if state_file.is_file():
        state = load_json(state_file)

    modules_cfg = cfg.get("modules") or {}
    exceptions = parse_exceptions(repo)

    # Fixture set for pinning: union of top-level and per-module
    top_fixtures = expand_fixtures(repo, cfg.get("fixtures", []))
    all_fixtures = list(top_fixtures)
    for mod in modules_cfg.values():
        if mod.get("fixtures"):
            all_fixtures.extend(expand_fixtures(repo, mod["fixtures"]))
    # unique
    seen = set()
    uniq_fixtures: list[Path] = []
    for f in all_fixtures:
        k = f.resolve()
        if k not in seen:
            seen.add(k)
            uniq_fixtures.append(f)

    pin_errors = pin_or_check(
        state,
        repo,
        list(cfg.get("oracle_sources", [])),
        uniq_fixtures,
        modules_cfg,
    )
    state_file.write_text(json.dumps(state, indent=2) + "\n", encoding="utf-8")

    report: dict[str, Any] = {
        "method": "byte-compare C oracle vs Rust driver stdout (and exit codes)",
        "reproduce": (
            "printf '%s' '{\"status\":\"completed\",\"loop_count\":0,"
            f"\"workspace_roots\":[\"{repo}\"]}}' | "
            "python3 tools/parity_gate.py --force"
        ),
        "pin_notes": state.get("_pin_notes", []),
        "errors": list(pin_errors),
        "results": [],
    }

    # Module readiness for stop event
    module_mode = args.event == "subagentStop" or bool(args.module)
    if modules_cfg and args.event == "stop" and not args.module:
        not_ready = [n for n, m in modules_cfg.items() if not m.get("ready")]
        if not_ready:
            report["result_line"] = (
                f"FAIL: module(s) not ready: {', '.join(not_ready)}"
            )
            report["errors"].append(report["result_line"])
            write_report(repo, cfg, report, module_mode=False)
            print(report["result_line"])
            # Pins still taken; this is the expected first-run failure.
            return 2

    # Decide which modules to run
    if args.module:
        selected = args.module
        for n in selected:
            if n not in modules_cfg:
                report["errors"].append(f"unknown module: {n}")
                report["result_line"] = f"FAIL: unknown module {n}"
                write_report(repo, cfg, report, True)
                print(report["result_line"])
                return 2
    elif args.event == "subagentStop":
        selected = [n for n, m in modules_cfg.items() if m.get("ready")]
        if not selected:
            report["result_line"] = "PASS: no ready modules to check"
            write_report(repo, cfg, report, True)
            print(report["result_line"])
            return 0
    else:
        selected = []  # full compare

    # Build both sides
    build = cfg.get("build") or []
    if build:
        rc, out, err = run_cmd(list(build), repo, timeout=600)
        if rc != 0:
            report["errors"].append(
                "build failed:\n"
                + (out + err).decode("utf-8", "replace")[-4000:]
            )
            report["result_line"] = "FAIL: build failed"
            write_report(repo, cfg, report, module_mode)
            print(report["result_line"])
            return 2

    results: list[dict[str, Any]] = []
    if selected:
        for name in selected:
            mod = modules_cfg[name]
            driver_args = list(mod.get("driver_args") or [])
            fixtures = (
                expand_fixtures(repo, mod["fixtures"])
                if mod.get("fixtures")
                else top_fixtures
            )
            for fixture in fixtures:
                row = compare_fixture(repo, cfg, fixture, driver_args, exceptions)
                row["module"] = name
                results.append(row)
    else:
        for fixture in top_fixtures:
            row = compare_fixture(repo, cfg, fixture, [], exceptions)
            row["module"] = "*"
            results.append(row)

    report["results"] = results
    identical = sum(1 for r in results if r.get("status") == "identical")
    exceptions_n = sum(1 for r in results if r.get("status") == "exception")
    bad = [
        r
        for r in results
        if r.get("status") not in {"identical", "exception"}
    ]
    if pin_errors or bad:
        report["result_line"] = (
            f"FAIL: {identical} identical, {exceptions_n} exceptions, "
            f"{len(bad)} divergences, {len(results)} total"
        )
        for r in bad[:5]:
            report["errors"].append(
                f"{r.get('fixture')} [{r.get('module')}]: {r.get('status')} "
                f"@ {r.get('byte_offset')} c={r.get('c_hex')} rust={r.get('rust_hex')}"
            )
        write_report(repo, cfg, report, module_mode or bool(selected))
        print(report["result_line"])
        for e in report["errors"][:10]:
            print(e)
        return 1

    report["result_line"] = (
        f"PASS: {identical} identical, {exceptions_n} exceptions, "
        f"0 divergences, {len(results)} total"
    )
    write_report(repo, cfg, report, module_mode or bool(selected))
    print(report["result_line"])
    return 0


if __name__ == "__main__":
    # Also install/copy ourselves to the hook path for the documented invoke.
    hook = Path.home() / ".cursor" / "hooks" / "c-rust-parity" / "parity_gate.py"
    here = Path(__file__).resolve()
    if here != hook:
        hook.parent.mkdir(parents=True, exist_ok=True)
        if not hook.exists() or hook.read_bytes() != here.read_bytes():
            hook.write_bytes(here.read_bytes())
            hook.chmod(0o755)
    sys.exit(main())
