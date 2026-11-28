"""Judge `td/telegram/TopDialogCategory.cpp` and `NotificationGroupType.cpp`:
C++ oracle vs Rust.

    uv run python tools/top_dialog_category_diff.py
    uv run python tools/top_dialog_category_diff.py --selftest

The oracle is `tools/fixtures/top_dialog_category_probe.cpp` over
`libtdcore.a` (no `Td`, no actor). Both faces are pure mapping: a td_api/wire
class to an enum, the enum back to a name or a wire class, and the four-value
notification group type with its two predicates.

Criteria:

  (a) narration line by line: 9 td_api classes + the null pointer, 10 wire
      classes, 10 names, 10 wire ids, 4 group-type rows and 4 round trips;
  (b) FLOOR: a second oracle run prints the same lines;
  (c) eight abort pairs: a null wire category (CHECK), `name(Size)`,
      `input(Size)`, a null group type (CHECK) and the out-of-range value 7 in
      the four group-type functions (UNREACHABLE);
  (d) `--selftest`: hand-computed cells pinning the asymmetry — td_api
      `topChatCategoryForwardChats` lands on `ForwardUsers`(6) while the wire
      `topPeerCategoryForwardChats` lands on `ForwardChats`(7) — plus `Size`
      = 10, the two phone-call ids and the group-type matrix.

Not covered (return conditions in F264): the `UNREACHABLE` arm of the two
`get_top_dialog_category` overloads (no unknown generated class id can be
built), and the printing/JSON of the returned objects, which belongs to the
object lane (F236).
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
DEFAULT_CPP = "C:/tmp/td-probe/top_dialog_category_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/top_dialog_category_probe.exe"
WORK = REPO / "work" / "top_dialog_category"

TD_API_NAMES = [
    "users",
    "bots",
    "botsinline",
    "groups",
    "channels",
    "calls",
    "forwardchats",
    "webappbots",
    "guestbots",
]

WIRE_NAMES = [
    "correspondents",
    "botspm",
    "botsinline",
    "groups",
    "channels",
    "phonecalls",
    "forwardusers",
    "forwardchats",
    "botsapp",
    "botsguest",
]

CASES = [
    ("td_api", [f"cat {name}" for name in TD_API_NAMES] + ["catnull"]),
    ("wire", [f"cattl {name}" for name in WIRE_NAMES]),
    ("names", [f"name {value}" for value in range(10)]),
    ("inputs", [f"input {value}" for value in range(10)]),
    ("group_types", [f"ngt {value}" for value in range(4)]),
    ("group_type_roundtrip", [f"ngtfrom {value}" for value in range(4)]),
]

CHECK_CASES = [
    "tl_null",
    "name_size",
    "input_size",
    "ngt_null",
    "ngt_db_oor",
    "ngt_partial_oor",
    "ngt_obj_oor",
    "ngt_print_oor",
]

# Hand-computed cells: (case, 0-based line index, expected text). The first two
# pin the asymmetry: the td_api side has no way to reach `ForwardChats`(7).
SELFTEST_CELLS = [
    ("td_api", 6, "cat=forwardchats id=1695922133 enum=6 name=forward_users"),
    ("td_api", 8, "cat=guestbots id=-1792882579 enum=9 name=bot_guest"),
    ("td_api", 9, "catnull=enum=10"),
    ("wire", 5, "cattl=phonecalls enum=5 name=call"),
    ("wire", 7, "cattl=forwardchats enum=7 name=forward_chats"),
    ("names", 6, "name=6 print=forward_users"),
    ("names", 7, "name=7 print=forward_chats"),
    ("inputs", 5, "input=5 id=511092620"),
    ("inputs", 6, "input=6 id=-1472172887"),
    ("inputs", 7, "input=7 id=-68239120"),
    ("group_types", 2, "ngt=2 db=1 partial=0 obj=1390759476 print=SecretChat"),
    ("group_types", 3, "ngt=3 db=0 partial=0 obj=1379123538 print=Calls"),
    ("group_type_roundtrip", 3, "ngtfrom=3 obj=1379123538 back=3"),
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
    print(f"PASS cases={len(CASES)} checks={len(CHECK_CASES)} lines={total_lines} elapsed={elapsed:.2f}s")
    return 0


if __name__ == "__main__":
    sys.exit(main())