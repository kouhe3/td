"""Judge `td/telegram/MessageSearchFilter.cpp`: C++ oracle vs Rust.

    uv run python tools/message_search_filter_diff.py
    uv run python tools/message_search_filter_diff.py --selftest

The oracle is `tools/fixtures/message_search_filter_probe.cpp` over
`libtdcore.a` (no `Td`, no actor). The whole face is pure mapping: the enum, the
bit mask, the two phone-call filters, and the two TL/td_api directions.

Criteria:

  (a) narration line by line: the 20 names, the 20 indexes, the 21 masks (Empty
      included), the two call indexes, the 17 TL input filters and the 19 td_api
      classes plus the null pointer;
  (b) FLOOR: a second oracle run prints the same lines;
  (c) seven abort pairs: `operator<<(Size)`, `index(Empty)`,
      `call_message_search_filter_index(Photo)` and the four filters with no TL
      form (`UnreadMention`, `FailedToSend`, `UnreadReaction`,
      `UnreadPollVote`);
  (d) `--selftest`: hand-computed cells pinning `Size == 21`, the `1 << 19` top
      mask, the `Empty` mask of 0, and the two phone-call entries.

Not covered (return conditions in F260): the `UNREACHABLE` arm of
`get_message_search_filter` (no unknown `td_api` class can be built) and the
always-zero `flags_` of `inputMessagesFilterPhoneCalls`.
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
DEFAULT_CPP = "C:/tmp/td-probe/message_search_filter_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/message_search_filter_probe.exe"
WORK = REPO / "work" / "message_search_filter"

TD_API_NAMES = [
    "empty",
    "animation",
    "audio",
    "document",
    "photo",
    "poll",
    "video",
    "voicenote",
    "photoandvideo",
    "url",
    "chatphoto",
    "videonote",
    "voiceandvideonote",
    "mention",
    "unreadmention",
    "unreadreaction",
    "unreadpollvote",
    "failedtosend",
    "pinned",
]

CASES = [
    ("printing", ["count"] + [f"print {value}" for value in range(1, 21)]),
    ("indexes", [f"index {value}" for value in range(1, 21)]),
    ("masks", [f"mask {value}" for value in range(0, 21)]),
    ("call_indexes", ["callindex 10", "callindex 11"]),
    # 0..14 is every filter that has a TL form, plus Pinned (17) and Poll (19).
    ("tl_filters", [f"input {value}" for value in list(range(0, 15)) + [17, 19]]),
    ("td_api", [f"tdapi {name}" for name in TD_API_NAMES] + ["tdapinull"]),
]

CHECK_CASES = [
    "print_size",
    "index_empty",
    "callindex_none",
    "input_unread_mention",
    "input_failed_to_send",
    "input_unread_reaction",
    "input_unread_poll_vote",
]

# Hand-computed cells: (case, 0-based line index, expected text). `Size` is 21,
# so the last index is 19 and the last mask is `1 << 19`.
SELFTEST_CELLS = [
    ("printing", 0, "count=20"),
    ("printing", 1, "print=Animation"),
    ("printing", 20, "print=UnreadPollVote"),
    ("indexes", 0, "index=0"),
    ("indexes", 19, "index=19"),
    ("masks", 0, "mask=0"),
    ("masks", 1, "mask=1"),
    ("masks", 20, "mask=524288"),
    ("call_indexes", 0, "callindex=0"),
    ("call_indexes", 1, "callindex=1"),
    ("tl_filters", 0, "input=id=1474492012"),
    ("tl_filters", 10, "input=calls silent=0"),
    ("tl_filters", 11, "input=calls silent=1"),
    ("tl_filters", 14, "input=id=-1040652646"),
    ("td_api", 0, "tdapi=Empty"),
    ("td_api", 19, "tdapinull=Empty"),
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