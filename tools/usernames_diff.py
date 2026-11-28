"""Judge `td::Usernames` (td/telegram/Usernames.{h,cpp}): C++ oracle vs Rust port.

    uv run python tools/usernames_diff.py
    uv run python tools/usernames_diff.py --selftest

`Usernames` is the pure value type behind `td_api::usernames`: it takes a
`first_username` plus a list of `telegram_api::username` records and keeps two
ordered arrays (`active`, `disabled`), the editable username's position and
three side tables (other editable / deletable / expired). No `Td`, no actor, no
scheduler is involved, so the probe drives the real implementation directly.

Four criteria, all run even if one fails:

  (a) narration line by line (the `operator<<` rendering plus accessors and the
      five predicates after every mutation);
  (b) FLOOR: a second oracle run prints the same lines;
  (c) abort pair: `--check reorder_missing`, one process per side -- passing an
      order that drops the editable username hits
      `CHECK(result.has_editable_username())`, so both sides must die;
  (d) `--selftest`: hand-computed cells for the corpus (vector rendering, the
      deletable/editable collision, the two-editable split, the invalidation
      reset).

What the corpus pins down (docs/facts.md F205):
  * `deletable` clears that record's `editable` before anything else, and the
    name still lands in `active`;
  * only the first `editable` is kept; later ones are demoted into
    `other_editable` and stay wherever `active`/`disabled` put them;
  * `disabled` entries take `editable_username_pos_` indexed into
    `disabled_usernames_`, and `is_editable_username_disabled_` picks the array;
  * duplicate names across `active` and `disabled` (reachable through
    `reorder_to`) make `check_validness()` reset the *whole* object, and every
    other invalid state does the same;
  * `add_secondary` prepends to `active` and shifts the editable position,
    `delete_secondary` may re-activate the editable username through
    `toggle(true, ...)`.

`toggle()` of a name that is in neither array is **not** an abort path: the
guard `if (!can_toggle(...)) return result;` runs first, so the trailing
`UNREACHABLE()` is reachable only through a `can_toggle` that disagrees with the
arrays. The corpus feeds that case through the script and both sides print the
unchanged object.

Not covered (return conditions in F205): `store`/`parse` (serialization needs
the TL storer and is judged elsewhere), `get_usernames_object()` (it is the
generated `td_api::usernames` object, not this value type), and `Td`-dependent
callers.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/usernames_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/usernames_probe.exe"
WORK = REPO / "work" / "usernames"

CASES = [
    ("first_only", ["new abc -", "print", "acc", "canadd x", "cantoggle 0 abc"]),
    (
        "first_plus_items",
        ["new abc a:1:0:0:0", "print", "acc", "canreorder -"],
    ),
    (
        "dedupe",
        ["new - a:1:1:0:0,a:0:1:0:0,b:1:0:0:0,b:0:0:0:0,c:0:0:1:0", "print", "acc"],
    ),
    (
        "deletable_editable",
        ["new - a:1:1:1:0,b:0:1:1:0", "print", "acc", "canadd c", "candel a", "candel b"],
    ),
    (
        "two_editables",
        ["new - a:1:1:0:0,b:0:1:0:0,c:1:1:0:0", "print", "acc"],
    ),
    (
        "editable_disabled",
        ["new - a:0:1:0:0,b:1:1:0:0,c:1:0:0:0", "print", "acc", "cantoggle 0 a", "cantoggle 1 a"],
    ),
    (
        "expired",
        ["new - a:1:0:0:1,b:1:0:0:0,c:0:0:0:1", "print", "acc"],
    ),
    (
        "empty_name",
        ["new - :1:0:0:0,b:1:0:0:0", "print"],
    ),
    (
        "change_ops",
        [
            "new - a:1:1:0:0,b:1:0:0:0",
            "change z",
            "print",
            "acc",
            "change -",
            "print",
            "acc",
            "change q",
            "print",
            "acc",
        ],
    ),
    (
        "change_disabled",
        ["new - a:0:1:0:0,b:1:0:0:0", "print", "change z", "print", "acc"],
    ),
    (
        "toggle_active",
        [
            "new - a:1:1:0:0,b:1:0:0:0,c:1:0:0:0",
            "toggle 0 b 0",
            "print",
            "toggle 0 b 1",
            "print",
            "toggle 0 a 0",
            "print",
            "toggle 1 a 0",
            "print",
            "acc",
            "toggle 1 a 1",
            "print",
        ],
    ),
    (
        "toggle_disabled",
        [
            "new - a:0:1:0:0,b:0:0:0:0",
            "print",
            "toggle 0 b 1",
            "print",
            "toggle 1 a 1",
            "print",
            "acc",
            "toggle 0 b 0",
            "print",
        ],
    ),
    (
        "toggle_unknown",
        ["new - a:1:1:0:0,b:0:0:0:0", "toggle 0 zz 0", "toggle 1 zz 1", "print", "acc"],
    ),
    (
        "secondary",
        [
            "new - a:1:1:0:0,b:1:0:0:0",
            "addsec c",
            "print",
            "acc",
            "canadd d",
            "candel c",
            "candel a",
            "delsec c",
            "print",
            "acc",
            "addsec e",
            "print",
        ],
    ),
    (
        "secondary_last_active",
        [
            "new - a:1:1:0:0",
            "addsec b",
            "print",
            "delsec b",
            "print",
            "acc",
        ],
    ),
    (
        "deact_active",
        ["new - a:1:1:0:0,b:1:0:0:0,c:0:0:0:0", "deact", "print", "acc", "deact", "print"],
    ),
    (
        "deact_disabled",
        ["new - a:0:1:0:0,b:1:0:0:0,c:0:0:0:0", "deact", "print", "acc"],
    ),
    (
        "reorder_ok",
        [
            "new - a:1:1:0:0,b:1:0:0:0,c:1:0:0:0",
            "canreorder c,a,b",
            "reorder c,a,b",
            "print",
            "acc",
            "canreorder c,a",
            "reorder b,a,c",
            "print",
        ],
    ),
    (
        "reorder_invalidates",
        [
            "new - a:0:1:0:0,b:0:0:0:0,c:1:0:0:0",
            "reorder b",
            "print",
            "acc",
        ],
    ),
    (
        "invalid_utf8",
        [
            "hexmode 1",
            "new - hex-61ff:1:1:0:0,hex-62:1:0:0:0",
            "printhex",
            "acc",
            "hexmode 0",
            "print",
        ],
    ),
    (
        "invalid_utf8_change",
        [
            "new - a:1:1:0:0",
            "hexmode 1",
            "change hex-fffe",
            "printhex",
            "acc",
            "hexmode 0",
            "print",
        ],
    ),
]

CHECK_CASES = ["reorder_missing"]

# Hand-computed cells: (case, 0-based line index, expected text). They lock the
# rendering format and the classification rules the corpus is built on, so a
# broken harness cannot pass by comparing two equally broken outputs.
SELFTEST_CELLS = [
    ("first_only", 0, "state=Usernames[abc, active {abc}]"),
    ("first_only", 2, "acc empty=0 hasfirst=1 first=616263 haseditable=1 editable=616263"),
    ("first_plus_items", 0, "state=Usernames[]"),
    ("deletable_editable", 0, "state=Usernames[, active {a}, disabled {b}, deletable {a, b}]"),
    ("two_editables", 0, "state=Usernames[a, active {a, c}, disabled {b}, other editable {b, c}]"),
    ("empty_name", 0, "state=Usernames[, active {b}]"),
    ("reorder_invalidates", 1, "state=Usernames[]"),
    ("invalid_utf8", 0, "statehex=557365726e616d65735b5d"),
]


def script_text(lines) -> str:
    return "\n".join(lines) + "\n"


def disable_error_dialogs() -> None:
    """Abort cases must die at once: a WerFault dialog keeps the exe locked."""
    if sys.platform != "win32":
        return
    import ctypes

    ctypes.windll.kernel32.SetErrorMode(0x0001 | 0x0002 | 0x8000)


def run(exe: Path, script: Path, env: dict):
    proc = subprocess.run(
        [str(exe), str(script)],
        capture_output=True,
        text=True,
        errors="replace",
        env=env,
    )
    return proc.returncode, proc.stdout.splitlines(), proc.stderr.strip()


def run_check(exe: Path, case: str, env: dict):
    proc = subprocess.run([str(exe), "--check", case], capture_output=True, text=True, env=env)
    return proc.returncode, proc.stdout.splitlines(), proc.stderr.strip()


def compare_lines(name: str, cpp, rs) -> bool:
    for index, (a, b) in enumerate(zip(cpp, rs)):
        if a != b:
            print(f"DIFF {name} line {index + 1}: oracle={a!r} rust={b!r}")
            return False
    if len(cpp) != len(rs):
        print(f"DIFF {name}: oracle lines={len(cpp)} rust lines={len(rs)}")
        return False
    return True


def check_abort_pair(args, env) -> bool:
    for case in CHECK_CASES:
        cpp_rc, cpp_out, cpp_err = run_check(Path(args.cpp), case, env)
        rs_rc, rs_out, rs_err = run_check(Path(args.rs), case, env)
        if cpp_rc == 0 or rs_rc == 0:
            print(f"DIFF check {case}: oracle rc={cpp_rc} rust rc={rs_rc} (one side survived)")
            return False
        if cpp_out != rs_out:
            print(f"DIFF check {case} stdout: oracle={cpp_out!r} rust={rs_out!r}")
            return False
        print(f"  check {case}: both aborted (oracle rc={cpp_rc} rust rc={rs_rc})")
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

    text = script_text([f"# {name}" for name, _ in CASES] + [line for _, body in CASES for line in body])
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
    cpp_lines: dict[str, list[str]] = {}

    for name, body in CASES:
        script = scripts_dir / f"{name}.txt"
        script.write_text(script_text(body))
        cpp_rc, cpp_out, cpp_err = run(Path(args.cpp), script, env)
        rs_rc, rs_out, rs_err = run(Path(args.rs), script, env)
        if cpp_rc != rs_rc:
            print(f"DIFF {name} exit code: oracle={cpp_rc} rust={rs_rc} {cpp_err!r} {rs_err!r}")
            return 1
        if cpp_rc != 0:
            print(f"FAIL {name}: both sides exited {cpp_rc}: {cpp_err!r} {rs_err!r}")
            return 1
        cpp_lines[name] = cpp_out
        total_lines += len(cpp_out) + len(rs_out)
        if not compare_lines(f"{name} narration", cpp_out, rs_out):
            return 1

        floor_rc, floor_out, _ = run(Path(args.cpp), script, env)
        if floor_rc != cpp_rc or floor_out != cpp_out:
            print(f"FLOOR FAIL: the oracle is not deterministic on {name}")
            return 1

    if not check_abort_pair(args, env):
        return 1

    if args.selftest and not selftest(cpp_lines):
        return 1

    elapsed = time.time() - started
    print(f"PASS cases={len(CASES)} checks={len(CHECK_CASES)} lines={total_lines} elapsed={elapsed:.2f}s")
    return 0


if __name__ == "__main__":
    sys.exit(main())