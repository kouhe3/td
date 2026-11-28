"""Judge `td::DialogId` (td/telegram/DialogId.{h,cpp}): C++ oracle vs Rust port.

    uv run python tools/dialog_id_diff.py
    uv run python tools/dialog_id_diff.py --selftest

`DialogId` packs a dialog of any type into one int64: users are positive, basic
groups are small negatives, channels are `-10^12 - channel_id`, secret chats are
`-2*10^12 + secret_chat_id`, and a monoforum channel range sits on top. Pure
value type -- no `Td`, no actor -- so the probe drives the real code from
`libtdcore.a`.

Four criteria, all run even if one fails:

  (a) narration line by line (rendering, validity, type, and the guarded
      getters after each construction);
  (b) FLOOR: a second oracle run prints the same lines;
  (c) abort pair, one process per case: the four getters on a wrong type
      (`uid_on_chat`, `cid_on_channel`, `chid_on_user`, `sid_on_chat`) must die
      on both sides;
  (d) `--selftest`: hand-computed cells for the range edges.

What the corpus pins down (docs/facts.md F212):
  * the ranges are contiguous and half-open: `[-999999999999, 0)` basic groups,
    `[-1997852516352, -10^12)` channels, `[-2002147483648, -2*10^12)` secret
    chats, and `[-4*10^12, ...)` the rest of the channels (monoforum);
  * `-2*10^12` itself is *not* a secret chat (the range excludes the zero id)
    and falls through to the monoforum channel branch;
  * constructors never CHECK: an invalid `UserId`/`ChatId`/`ChannelId`/
    `SecretChatId` produces the null dialog;
  * `operator<<` always renders `chat <raw id>`, whatever the type.

Not covered (return conditions in F212): `get_message_dialog_id` and the
`telegram_api::Peer`/`DialogPeer` constructors (they need TL objects), the
storer/parser templates, and `Td`-dependent callers.
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
DEFAULT_CPP = "C:/tmp/td-probe/dialog_id_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/dialog_id_probe.exe"
WORK = REPO / "work" / "dialog_id"

MAX_USER = (1 << 40) - 1
MAX_CHAT = 999999999999
MAX_CHANNEL = 1000000000000 - (1 << 31)
MIN_MONOFORUM = 1000000000000 + (1 << 31) + 1
MAX_MONOFORUM = 3000000000000

RAW_VALUES = [
    0,
    1,
    -1,
    42,
    MAX_USER,
    MAX_USER + 1,
    -MAX_CHAT,
    -MAX_CHAT - 1,
    -1000000000001,
    -(1000000000000 + MAX_CHANNEL),
    -(1000000000000 + MAX_CHANNEL) + 1,
    -2000000000001,
    -2000000000005,
    -(2000000000000 + 2147483648),
    -(2000000000000 + 2147483648) - 1,
    -4000000000000,
    -4000000000001,
    -3000000000000,
    -9223372036854775808,
]

CASES = [
    (
        "raw_ranges",
        [line for value in RAW_VALUES for line in (f"raw {value}", "dump", "uid", "cid", "chid", "sid")],
    ),
    (
        "construct_user",
        [line for value in (0, 1, MAX_USER, MAX_USER + 1, -1) for line in (f"user {value}", "dump", "uid")],
    ),
    (
        "construct_chat",
        [line for value in (0, 1, MAX_CHAT, MAX_CHAT + 1, -1) for line in (f"chat {value}", "dump", "cid")],
    ),
    (
        "construct_channel",
        [
            line
            for value in (0, 1, MAX_CHANNEL - 1, MAX_CHANNEL, MIN_MONOFORUM, MAX_MONOFORUM - 1, MAX_MONOFORUM, -1)
            for line in (f"channel {value}", "dump", "chid")
        ],
    ),
    (
        "construct_secret",
        [line for value in (0, 1, -1, 2147483647) for line in (f"secret {value}", "dump", "sid")],
    ),
    (
        "vectors",
        [
            "ids 42,-5,-1000000000005,-2000000000005,0",
            "ids -",
            "usrs 1,1099511627775,1099511627776,0",
            "chats 1,999999999999,1000000000000,0",
            "chans 1,997852516351,997852516352,1002147483649,3000000000000,0",
            "secs 1,2,0",
            "rmsecret 42,-5,-1000000000005,-2000000000005,0",
            "rmsecret -",
        ],
    ),
    (
        "cmp",
        ["cmp 42 42", "cmp 42 -5", "cmp 0 0", "cmp -2000000000005 -2000000000005"],
    ),
]

CHECK_CASES = ["uid_on_chat", "cid_on_channel", "chid_on_user", "sid_on_chat"]

SELFTEST_CELLS = [
    ("raw_ranges", 0, "dump raw=0 print=chat 0 valid=0 type=None"),
    ("raw_ranges", 5, "dump raw=1 print=chat 1 valid=1 type=User"),
    ("raw_ranges", 10, "dump raw=-1 print=chat -1 valid=1 type=Chat"),
    ("raw_ranges", 20, "dump raw=1099511627775 print=chat 1099511627775 valid=1 type=User"),
    ("raw_ranges", 25, "dump raw=1099511627776 print=chat 1099511627776 valid=0 type=None"),
    ("construct_user", 0, "dump raw=0 print=chat 0 valid=0 type=None"),
    ("construct_user", 2, "dump raw=1 print=chat 1 valid=1 type=User"),
    ("construct_chat", 2, "dump raw=-1 print=chat -1 valid=1 type=Chat"),
    ("construct_channel", 2, "dump raw=-1000000000001 print=chat -1000000000001 valid=1 type=Channel"),
    ("construct_secret", 2, "dump raw=-1999999999999 print=chat -1999999999999 valid=1 type=SecretChat"),
    ("cmp", 1, "cmp eq=0 ne=1"),
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
        cpp_rc, cpp_out, _ = run_check(Path(args.cpp), case, env)
        rs_rc, rs_out, _ = run_check(Path(args.rs), case, env)
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