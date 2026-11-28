"""Judge `td::SqliteConnectionSafe` + `td::SqliteKeyValueSafe` (C++ oracle vs Rust).

    uv run python tools/sqlite_safe_diff.py
    uv run python tools/sqlite_safe_diff.py --selftest

Both classes use `SchedulerLocalStorage`, so the probe runs the whole script
inside an actor on `ConcurrentScheduler(0)` (facts F216): constructing them on a
plain thread null-derefs, and `SqliteConnectionSafe` opens with
`allow_creation=false`, so the probe creates the database first. The Rust side
models the contract (lazy open, close keeps the file, destroy removes it, the
state counter in the fatal message).

Criteria:

  (a) narration line by line, plus the `exists=` state after every step;
  (b) FLOOR: a second oracle run prints the same lines;
  (c) two abort shapes (`get_missing`, `get_after_destroy`): both sides must die
      and their fatal message must carry the same `state <n>` token -- that is
      how the private `close_state_` counter is observed (0 vs 65537);
  (d) `--selftest`: hand-computed cells.

What the corpus pins down (facts F217): construction is lazy (no open until
`get`), `close` clears the values but keeps the file and bumps the state,
`close_and_destroy` adds 65536 and deletes the file, and a later `get` on a
missing file is a fatal error whose message names the state.

Not covered (return conditions in F217): per-thread cached connections (a
one-scheduler probe only ever uses slot 0), the `SqliteKeyValueAsync` layer
(needs the serialized driver from F216), and the real sqlite byte layout.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/sqlite_safe_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/sqlite_safe_probe.exe"
WORK = REPO / "work" / "sqlite_safe"

CASES = [
    (
        "lifecycle",
        [
            "prepare",
            "conn",
            "exists",
            "get",
            "kv",
            "set alpha 1",
            "sget alpha",
            "close",
            "exists",
            "get",
            "sget alpha",
            "destroy",
            "exists",
            "drop",
            "exists",
        ],
        False,
    ),
    (
        "lazy_conn",
        ["prepare", "conn", "exists", "drop", "exists"],
        False,
    ),
    (
        "reopen_after_close",
        ["prepare", "conn", "get", "close", "get", "close", "get", "exists"],
        False,
    ),
    (
        "kv_after_close",
        ["prepare", "conn", "get", "kv", "set beta 2", "close", "sget beta", "set gamma 3", "sget gamma"],
        False,
    ),
    (
        "destroy_then_drop",
        ["prepare", "conn", "destroy", "drop", "exists"],
        False,
    ),
    (
        "prepare_after_kv",
        ["prepare", "conn", "get", "kv", "set k v", "sget k", "prepare", "exists"],
        False,
    ),
    (
        "get_missing",
        ["prepare_empty", "conn", "get"],
        True,
    ),
    (
        "get_after_destroy",
        ["prepare", "conn", "destroy", "get"],
        True,
    ),
]

SELFTEST_CELLS = [
    ("lifecycle", 0, "prepare exists=1"),
    ("lifecycle", 2, "exists exists=1"),
    ("lifecycle", 3, "get exists=1"),
    ("lifecycle", 5, "set alpha 1"),
    ("lifecycle", 6, "sget alpha=1"),
    ("lifecycle", 7, "close exists=1"),
    ("lifecycle", 10, "sget alpha=1"),
    ("lifecycle", 11, "destroy exists=0"),
    ("lazy_conn", 3, "drop exists=1"),
    ("prepare_after_kv", 7, "exists exists=1"),
]

STATE_RE = re.compile(r"in state (\d+)")


def script_text(lines) -> str:
    return "\n".join(lines) + "\n"


def disable_error_dialogs() -> None:
    """Abort cases must die at once: a WerFault dialog keeps the exe locked."""
    if sys.platform != "win32":
        return
    import ctypes

    ctypes.windll.kernel32.SetErrorMode(0x0001 | 0x0002 | 0x8000)


def run(exe: Path, script: Path, workdir: Path, env: dict):
    if workdir.exists():
        for child in workdir.iterdir():
            if child.is_file():
                child.unlink()
    workdir.mkdir(parents=True, exist_ok=True)
    proc = subprocess.run(
        [str(exe), str(script), str(workdir)],
        capture_output=True,
        text=True,
        errors="replace",
        env=env,
    )
    state = STATE_RE.search(proc.stderr)
    return proc.returncode, proc.stdout.splitlines(), (state.group(1) if state else None)


def compare_lines(name: str, cpp, rs) -> bool:
    for index, (a, b) in enumerate(zip(cpp, rs)):
        if a != b:
            print(f"DIFF {name} line {index + 1}: oracle={a!r} rust={b!r}")
            return False
    if len(cpp) != len(rs):
        print(f"DIFF {name}: oracle lines={len(cpp)} rust lines={len(rs)}")
        return False
    return True


def selftest(cpp_lines: dict[str, list[str]]) -> bool:
    ok = True
    for name, index, expected in SELFTEST_CELLS:
        lines = cpp_lines.get(name)
        if lines is None or index >= len(lines):
            print(f"CHECK FAIL {name}[{index}]: missing line")
            ok = False
            continue
        if lines[index] != expected:
            print(f"CHECK FAIL {name}[{index}]: oracle={lines[index]!r} expected={expected!r}")
            ok = False
    if ok:
        print(f"  selftest {len(SELFTEST_CELLS)} cells OK")
    return ok


def main() -> int:
    disable_error_dialogs()
    parser = argparse.ArgumentParser()
    parser.add_argument("--cpp", default=DEFAULT_CPP)
    parser.add_argument("--rs", default=str(DEFAULT_RS))
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()

    for path in (args.cpp, args.rs):
        if not Path(path).exists():
            print(f"MISSING {path}")
            return 2

    text = script_text([f"# {name}" for name, _, _ in CASES] + [line for _, body, _ in CASES for line in body])
    digest = hashlib.md5(text.encode()).hexdigest()
    print(f"corpus md5={digest} lines={len(text.splitlines())}")

    scripts_dir = WORK / "scripts"
    if scripts_dir.exists():
        for child in scripts_dir.iterdir():
            child.unlink()
    scripts_dir.mkdir(parents=True, exist_ok=True)

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    started = time.time()
    total_lines = 0
    aborts = 0
    cpp_lines: dict[str, list[str]] = {}

    for name, body, expect_abort in CASES:
        script = scripts_dir / f"{name}.txt"
        script.write_text(script_text(body))
        cpp_rc, cpp_out, cpp_state = run(Path(args.cpp), script, WORK / f"cpp_{name}", env)
        rs_rc, rs_out, rs_state = run(Path(args.rs), script, WORK / f"rs_{name}", env)
        cpp_lines[name] = cpp_out
        total_lines += len(cpp_out) + len(rs_out)
        if not compare_lines(f"{name} narration", cpp_out, rs_out):
            return 1
        if expect_abort:
            aborts += 1
            if cpp_rc == 0 or rs_rc == 0:
                print(f"DIFF {name}: expected both sides to die, oracle rc={cpp_rc} rust rc={rs_rc}")
                return 1
            if cpp_state != rs_state:
                print(f"DIFF {name}: fatal state oracle={cpp_state} rust={rs_state}")
                return 1
            print(f"  abort {name}: both died, state={cpp_state}")
        else:
            if cpp_rc != rs_rc:
                print(f"DIFF {name} exit code: oracle={cpp_rc} rust={rs_rc}")
                return 1
            if cpp_rc != 0:
                print(f"FAIL {name}: both sides exited {cpp_rc}")
                return 1
            floor_rc, floor_out, _ = run(Path(args.cpp), script, WORK / f"cpp_{name}", env)
            if floor_rc != cpp_rc or floor_out != cpp_out:
                print(f"FLOOR FAIL: the oracle is not deterministic on {name}")
                return 1

    if args.selftest and not selftest(cpp_lines):
        return 1

    elapsed = time.time() - started
    print(f"PASS cases={len(CASES)} aborts={aborts} lines={total_lines} elapsed={elapsed:.2f}s")
    return 0


if __name__ == "__main__":
    sys.exit(main())