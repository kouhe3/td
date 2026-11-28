"""Judge `td/telegram/NotificationSound.cpp`: C++ oracle vs Rust.

    uv run python tools/notification_sound_diff.py
    uv run python tools/notification_sound_diff.py --selftest

The oracle is `tools/fixtures/notification_sound_probe.cpp` over `libtdcore.a`
(no `Td`, no actor). Only the pure value semantics are reachable; the log-event
storer and parser `LOG_CHECK` the actor context through `G()`, so `store` and
`parse` are judged as **abort pairs** instead (facts F249).

Criteria:

  (a) narration line by line, including the raw bytes of a local sound's title
      and data (the corpus feeds a NUL and an invalid UTF-8 byte);
  (b) FLOOR: a second oracle run prints the same lines;
  (c) `--check store` / `--check parse`: both sides print the same prefix and die;
  (d) `--selftest`: hand-computed cells for the surprising rules — a Local sound
      counts as the default, `nullptr` and Local share the ringtone ID `-1`, a
      ringtone ID of 0 or -1 becomes NoSound, and the legacy string table.

Not covered (return conditions in F249/F250): the `store`/`parse` byte format.
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
DEFAULT_CPP = "C:/tmp/td-probe/notification_sound_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/notification_sound_probe.exe"
WORK = REPO / "work" / "notification_sound"

CASES = [
    (
        "printing",
        [
            "print -",
            "print none",
            "print local:abc|def",
            "print ring:7",
            "print ring:-5",
            "print local:hex-ff|hex-4100",
            "print local:-|-",
        ],
    ),
    (
        "defaults",
        [
            "def - -",
            "def - none",
            "def - local:a|b",
            "def - ring:1",
            "def none local:a|b",
            "def local:a|b ring:1",
            "def none none",
            "def ring:1 ring:1",
        ],
    ),
    (
        "equivalence",
        [
            "eq - -",
            "eq - none",
            "eq - local:a|b",
            "eq none none",
            "eq none ring:0",
            "eq ring:7 ring:7",
            "eq ring:7 ring:8",
            "eq local:a|b local:c|d",
            "eq local:a|b none",
        ],
    ),
    (
        "different",
        [
            "diff - -",
            "diff - local:a|b",
            "diff local:a|b -",
            "diff local:a|b local:a|b",
            "diff local:a|b local:a|c",
            "diff local:a|b ring:1",
            "diff none none",
            "diff - none",
        ],
    ),
    (
        "ids",
        ["rid -", "rid none", "rid local:a|b", "rid ring:7", "rid ring:-1", "rid ring:0"],
    ),
    (
        "factories",
        [
            "legacy default",
            "legacy -",
            "legacy abc",
            "legacy hex-ff",
            "boolsound 1 5",
            "boolsound 1 -1",
            "boolsound 0 -1",
            "boolsound 0 0",
            "boolsound 0 5",
        ],
    ),
    (
        "tl_map",
        [
            "tl null",
            "tl default",
            "tl none",
            "tl local abc def",
            "tl local - -",
            "tl local hex-ff hex-41",
            "tl ring 0",
            "tl ring -1",
            "tl ring 9",
        ],
    ),
    (
        "input_map",
        [
            "input - 0",
            "input - 1",
            "input none 0",
            "input none 1",
            "input local:a|b 0",
            "input local:a|b 1",
            "input ring:7 0",
        ],
    ),
    (
        "dup",
        ["dup -", "dup none", "dup local:a|b", "dup ring:7"],
    ),
]

CHECK_CASES = ["store", "parse"]

# Hand-computed cells: (case, 0-based line index, expected text).
SELFTEST_CELLS = [
    ("printing", 0, "print type=null text=DefaultSound"),
    ("printing", 2, "print type=local text=LocalSound[abc|def]"),
    ("printing", 3, "print type=ringtone text=Ringtone[7]"),
    ("defaults", 0, "def lhs=1 rhs=1"),
    ("defaults", 1, "def lhs=1 rhs=0"),
    ("defaults", 4, "def lhs=0 rhs=1"),
    ("equivalence", 2, "eq=1"),
    ("equivalence", 3, "eq=1"),
    ("equivalence", 7, "eq=1"),
    ("equivalence", 8, "eq=0"),
    ("different", 1, "diff=1"),
    ("different", 2, "diff=1"),
    ("different", 4, "diff=1"),
    ("different", 6, "diff=0"),
    ("ids", 0, "rid=-1"),
    ("ids", 1, "rid=0"),
    ("ids", 2, "rid=-1"),
    ("ids", 3, "rid=7"),
    ("factories", 0, "legacy type=null text=DefaultSound"),
    ("factories", 1, "legacy type=none text=NoSound"),
    ("factories", 2, "legacy type=local text=LocalSound[abc|abc]"),
    ("factories", 6, "boolsound type=null text=DefaultSound"),
    ("factories", 7, "boolsound type=none text=NoSound"),
    ("tl_map", 1, "tl type=null text=DefaultSound"),
    ("tl_map", 6, "tl type=none text=NoSound"),
    ("tl_map", 7, "tl type=none text=NoSound"),
    ("tl_map", 8, "tl type=ringtone text=Ringtone[9]"),
    ("input_map", 0, "input=null"),
    ("input_map", 1, "input=default"),
    ("input_map", 2, "input=none"),
    ("input_map", 4, "input=local title=61 data=62"),
    ("dup", 2, "dup type=local text=LocalSound[a|b]"),
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
    ok = True
    for case in CHECK_CASES:
        cpp_rc, cpp_out, _ = run_check(Path(args.cpp), case, env)
        rs_rc, rs_out, _ = run_check(Path(args.rs), case, env)
        if cpp_rc == 0 or rs_rc == 0:
            print(f"DIFF check {case}: oracle rc={cpp_rc} rust rc={rs_rc} (one side survived)")
            ok = False
            continue
        if cpp_out != rs_out:
            print(f"DIFF check {case} stdout: oracle={cpp_out!r} rust={rs_out!r}")
            ok = False
            continue
        print(f"  check {case}: both aborted (oracle rc={cpp_rc} rust rc={rs_rc})")
    return ok


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
    print(
        f"PASS cases={len(CASES)} checks={len(CHECK_CASES)} lines={total_lines} elapsed={elapsed:.2f}s"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())