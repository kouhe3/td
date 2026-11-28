"""Judge `td::MessageId` (td/telegram/MessageId.{h,cpp}): C++ oracle vs Rust port.

    uv run python tools/message_id_diff.py
    uv run python tools/message_id_diff.py --selftest

`MessageId` is the 64-bit tag in front of every message: an ordinary layout
`|31 server|17 local|1 zero|2 type|` and a scheduled layout
`|30 date-2^30|18 server|1 one|2 type|`. It is a pure value type -- no `Td`, no
actor -- so the probe builds the values and drives the real code from
`libtdcore.a`.

Four criteria, all run even if one fails:

  (a) narration line by line (the `operator<<` rendering plus every predicate
      and the guarded accessors after each construction);
  (b) FLOOR: a second oracle run prints the same lines;
  (c) abort pair, one process per case: `date_ordinary` (CHECK is_valid_scheduled),
      `ssrv_ordinary` (CHECK is_server), `cmp_mixed` (operator< CHECKs a shared
      layout) and `next_none` (UNREACHABLE) must die on both sides;
  (d) `--selftest`: hand-computed cells (renderings and the arithmetic).

What the corpus pins down (docs/facts.md F208):
  * an ordinary id is valid only when the low 20 bits are zero (server) or hold
    type 1/2; everything else is `invalid message <id>`;
  * `operator<<` prints local/yet-unsent as `<server>.<low 20 bits>`, so the
    decimal tail is the *raw* low bits, not the local id;
  * the scheduled date is `(id >> 21) + 2^30` and the scheduled server id is
    `(id >> 3) & (2^18-1)`;
  * `get_next_message_id` carries into the next server id for ordinary ids but
    reuses the low field for scheduled ones (`id - current_type + type`);
  * a scheduled id with an unknown type renders as `bugged message`.

Not covered (return conditions in F208): `get_message_id(telegram_api::Message*)`
and the two order predicates (they need TL message objects), the storer/parser
templates, and `Td`-dependent callers.
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
DEFAULT_CPP = "C:/tmp/td-probe/message_id_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/message_id_probe.exe"
WORK = REPO / "work" / "message_id"

MAX_ID = (2**31 - 1) << 20

ORDINARY = [
    0,
    -1,
    1,
    2,
    3,
    4,
    5,
    7,
    1000,
    (1 << 20) - 1,
    1 << 20,
    (1 << 20) + 1,
    (1 << 20) + 2,
    (1 << 20) + 4,
    (1 << 20) + 7,
    2 << 20,
    MAX_ID - 1,
    MAX_ID,
    MAX_ID + 1,
]

CASES = [
    (
        "ordinary",
        [line for value in ORDINARY for line in (f"raw {value}", "dump", "prev", "nextsrv", "next 1", "next 2", "next 3", "srvget")],
    ),
    (
        "sponsored",
        [
            line
            for value in ((1 << 51) - 8, (1 << 51) - 4, (1 << 51) - 2, 1 << 51, (1 << 51) + 2, (1 << 51) + 8)
            for line in (f"raw {value}", "dump", "prev", "nextsrv")
        ],
    ),
    (
        "scheduled_construct",
        [
            line
            for args in (
                ("5", "1073741824", ""),
                ("5", "1073741825", ""),
                ("5", "1073741825", "1"),
                ("0", "1073741825", ""),
                ("0", "1073741825", "1"),
                ("262143", "2000000000", ""),
                ("262144", "2000000000", ""),
                ("262144", "2000000000", "1"),
                ("-1", "2000000000", ""),
                ("-1", "2000000000", "1"),
            )
            for line in (f"sched {args[0]} {args[1]} {args[2]}".strip(), "dump", "date", "ssrv")
        ],
    ),
    (
        "scheduled_ops",
        [
            "sched 5 1073741834",
            "dump",
            "date",
            "ssrv",
            "prev",
            "nextsrv",
            "next 1",
            "next 2",
            "next 3",
            "sched 5 2000000000",
            "dump",
            "next 1",
            "next 3",
            "date",
        ],
    ),
    (
        "type_switch",
        [
            line
            for value in (1 << 20, (1 << 20) | 1, (1 << 20) | 2, (1 << 20) | 3, (1 << 20) | 4, (1 << 20) | 5,
                          (1 << 20) | 6, (1 << 20) | 7)
            for line in (f"raw {value}", "dump")
        ],
    ),
    (
        "vectors",
        [
            "ids 0,1,1048576,1048577,1048578,2097152",
            "ids -",
            "srvids 1048576,2097152,3145728",
            "srvids -",
            "ssrvids 68,20971564",
            "ssrvids -",
        ],
    ),
    (
        "cmp_ordinary",
        ["cmp 0 1", "cmp 1048576 1048577", "cmp 1048578 1048578", "cmp 2097152 1048576", "cmp -1 0"],
    ),
    (
        "cmp_scheduled",
        [
            "cmp 20971564 20971566",
            "cmp 20971566 20971564",
            "cmp 2147483648 2147483650",
        ],
    ),
]

CHECK_CASES = ["date_ordinary", "ssrv_ordinary", "cmp_mixed", "next_none"]

# Hand-computed cells: (case, 0-based line index, expected text).
SELFTEST_CELLS = [
    ("ordinary", 0, "dump raw=0 print=invalid message 0 valid=0 vsched=0 vspon=0 type=None sched=0 server=0 ssrv=n/a local=n/a yet=n/a any=0"),
    ("ordinary", 14, "dump raw=1 print=yet unsent message 0.1 valid=1 vsched=0 vspon=0 type=YetUnsent sched=0 server=0 ssrv=n/a local=0 yet=1 any=0"),
    ("ordinary", 21, "dump raw=2 print=local message 0.2 valid=1 vsched=0 vspon=0 type=Local sched=0 server=0 ssrv=n/a local=1 yet=0 any=0"),
    ("ordinary", 70, "dump raw=1048576 print=server message 1 valid=1 vsched=0 vspon=0 type=Server sched=0 server=1 ssrv=n/a local=0 yet=0 any=1"),
    ("ordinary", 77, "dump raw=1048577 print=yet unsent message 1.1 valid=1 vsched=0 vspon=0 type=YetUnsent sched=0 server=0 ssrv=n/a local=0 yet=1 any=0"),
    ("ordinary", 84, "dump raw=1048578 print=local message 1.2 valid=1 vsched=0 vspon=0 type=Local sched=0 server=0 ssrv=n/a local=1 yet=0 any=0"),
    ("scheduled_ops", 1, "date=1073741834"),
    ("scheduled_ops", 2, "ssrv=5"),
    ("scheduled_ops", 5, "next=scheduled server message 6"),
    ("vectors", 0, "ids n=6 [invalid message 0] [yet unsent message 0.1] [server message 1] [yet unsent message 1.1] [local message 1.2] [server message 2]"),
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
    print(
        f"PASS cases={len(CASES)} checks={len(CHECK_CASES)} lines={total_lines} elapsed={elapsed:.2f}s"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())