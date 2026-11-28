"""Judge `td::ForumTopicId` + `td::SavedMessagesTopicId` (pure parts).

    uv run python tools/topic_id_diff.py
    uv run python tools/topic_id_diff.py --selftest

Both are value wrappers: `ForumTopicId` is an int32 that round-trips through
`MessageId`'s server id, `SavedMessagesTopicId` is a `DialogId` with a
hidden-author special case (`DialogId(2666000)`). No `Td`, no actor.

Criteria:

  (a) narration line by line (validity, rendering, and the MessageId round trip);
  (b) FLOOR: a second oracle run prints the same lines;
  (c) one abort shape (`frommsg_force <non-server>`): `from_top_thread_message_id`
      calls `MessageId::get_server_message_id()`, whose CHECK kills both sides;
  (d) `--selftest`: hand-computed cells.

Facts (F221): `general()` is 1, `operator<<` is `topic N` for any id (even 0 or
negative), the top-message round trip keeps the 20-bit server layout, and the
saved-messages render has three shapes (`[no topic]` / `[Author Hidden topic]`
/ `[topic of <DialogId>]`).

Not covered (return conditions in F221): everything that takes `Td`
(`get_saved_messages_topic_type_object`, `get_input_peer`, ...), the
`MessageForwardInfo` constructor, and storer/parser.
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
DEFAULT_CPP = "C:/tmp/td-probe/topic_id_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/topic_id_probe.exe"
WORK = REPO / "work" / "topic_id"

SERVER = 1 << 20

CASES = [
    (
        "forum_bounds",
        [line for value in (0, 1, -1, 1000, 2147483647, -2147483648) for line in (f"forum {value}",)] +
        ["fids 1,2,0,-3", "fids -", "fmsgs 1,2,0,-3", "fmsgs -"],
        False,
    ),
    (
        "round_trip",
        [
            "frommsg 1048576",
            "frommsg 20971520",
            "frommsg 0",
            "frommsg 2251799813683200",
            "frommsg 1000",
            "frommsg 1048577",
            "frommsg_force 1048576",
        ],
        False,
    ),
    (
        "saved_forms",
        [
            "saved 42",
            "saved 2666000",
            "saved 0",
            "saved -5",
            "saved -1000000000005",
            "saved -2000000000005",
            "saved 2666001",
        ],
        False,
    ),
    (
        "saved_eq",
        ["savedeq 42 42", "savedeq 42 43", "savedeq 0 0", "savedeq 2666000 2666000"],
        False,
    ),
    (
        "frommsg_abort",
        ["frommsg_force 1000"],
        True,
    ),
]

SELFTEST_CELLS = [
    ("forum_bounds", 0, "forum raw=0 valid=0 print=topic 0"),
    ("forum_bounds", 1, "forum raw=1 valid=1 print=topic 1"),
    ("forum_bounds", 2, "forum raw=-1 valid=0 print=topic -1"),
    ("forum_bounds", 6, "fids n=4 [topic 1] [topic 2] [topic 0] [topic -3]"),
    ("forum_bounds", 8, "fmsgs n=4 1 2 0 -3"),
    ("round_trip", 0, "frommsg raw=1048576 top=server message 1 print=topic 1"),
    ("round_trip", 1, "frommsg raw=20971520 top=server message 20 print=topic 20"),
    ("round_trip", 4, "frommsg=n/a"),
    ("saved_forms", 0, "saved dialog=42 valid=1 unique=42 hidden=0 print=[topic of chat 42]"),
    ("saved_forms", 1, "saved dialog=2666000 valid=1 unique=2666000 hidden=1 print=[Author Hidden topic]"),
    ("saved_forms", 2, "saved dialog=0 valid=0 unique=0 hidden=0 print=[no topic]"),
    ("saved_eq", 1, "savedeq eq=0 ne=1"),
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
        cpp_rc, cpp_out, cpp_err = run(Path(args.cpp), script, env)
        rs_rc, rs_out, rs_err = run(Path(args.rs), script, env)
        cpp_lines[name] = cpp_out
        total_lines += len(cpp_out) + len(rs_out)
        if not compare_lines(f"{name} narration", cpp_out, rs_out):
            return 1
        if expect_abort:
            aborts += 1
            if cpp_rc == 0 or rs_rc == 0:
                print(f"DIFF {name}: expected both sides to die, oracle rc={cpp_rc} rust rc={rs_rc}")
                return 1
            print(f"  abort {name}: both died (oracle rc={cpp_rc} rust rc={rs_rc})")
        else:
            if cpp_rc != rs_rc:
                print(f"DIFF {name} exit code: oracle={cpp_rc} rust={rs_rc} {cpp_err!r} {rs_err!r}")
                return 1
            if cpp_rc != 0:
                print(f"FAIL {name}: both sides exited {cpp_rc}")
                return 1
            floor_rc, floor_out, _ = run(Path(args.cpp), script, env)
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