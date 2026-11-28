"""Judge `td::TQueue` (tddb/td/db/TQueue.h): C++ oracle vs the Rust port.

    uv run python tools/tqueue_diff.py
    uv run python tools/tqueue_diff.py --selftest

Both sides run the same script against `TQueue::create()` (the real
`TQueueImpl`) with a `TQueueMemoryStorage` as storage callback -- no actor, no
binlog. Five criteria, all run even after one fails:

  (a) every command's narration line by line;
  (b) FLOOR: a second oracle run prints the same lines;
  (c) `--check`, one process per case: `do_push` with an empty `EventId` aborts
      on `CHECK(raw_event.event_id.is_valid())`, a valid one does not;
  (d) the no-hint path only asserts an invariant (the tail lands in
      `[2000001, 1000000000)`), because `Random::fast` is not injectable;
  (e) the replay path: `TQueueMemoryStorage::replay` rebuilds a *fresh* queue
      from the stored events, so a wrong `pop`/rewrite shows up as a different
      head/tail/size even when the first queue looks right.

What the corpus pins down (measured; docs/facts.md F199/F200):
  * `get_size` drops the **last** event when its data is empty, so a rewritten
    slot does not count; `clear(q, 0)` therefore returns `size - 1` events and
    keeps the last one's slot and `log_event_id`;
  * `get` returns `get_size`, not the number of events handed back, and it pops
    expired/empty events from the front **even when `max_events` is 0**;
  * `pop` on the last event rewrites (data cleared, id kept) instead of removing;
  * `push` honours `hint_new_id` only while the tail is empty;
  * `from_id > tail + 10` is "in the future", `from_id < head - 100000` is "in
    the past";
  * the error order is data-empty, data-too-big, queue-0, queue-full,
    queue-too-big, already-expired;
  * `EventId::from_int32(0)` succeeds (valid but empty); `MAX_ID` does not.

Not covered (return conditions in F199): `Random::fast`'s value, `run_gc`'s
50 ms budget and its `& 128` slice (a small corpus never reaches either),
`Time::now()` in `clear`'s warning log, `TQueueBinlog` frames 2314/2315, and
`close(Promise)`.
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
DEFAULT_CPP = "C:/tmp/td-probe/tqueue_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/tqueue_probe.exe"
WORK = REPO / "work" / "tqueue"

MAX_EVENT_LENGTH = 65536 * 8
BIG = "ab" * (MAX_EVENT_LENGTH + 1)  # one byte over the limit

CASES = [
    (
        "basic",
        [
            "push 1 6161 1000 7 100",
            "push 1 6262 2000 0 -",
            "push 1 6363 3000 9 -",
            "head 1",
            "tail 1",
            "size 1",
            "get 1 100 0 500 8",
            "forget 1 101",
            "get 1 100 0 500 8",
            "size 1",
        ],
    ),
    (
        "hint",
        [
            "push 1 61 1000 0 100",
            "push 1 62 1000 0 500",
            "head 1",
            "tail 1",
            "push 2 63 1000 0 700",
            "head 2",
            "tail 2",
        ],
    ),
    (
        "errors",
        [
            "push 1 - 1000 0 100",
            "push 0 61 1000 0 1",
            "push 1 61 0 0 1",
            "push 1 61 -5 0 1",
            f"push 1 {BIG} 1000 0 1",
            f"push 1 {BIG} 0 0 1",
        ],
    ),
    (
        "expiry",
        [
            "push 1 61 500 0 100",
            "push 1 62 1000 0 -",
            "push 1 63 1500 0 -",
            "get 1 100 0 600 8",
            "size 1",
            "head 1",
        ],
    ),
    (
        "max_zero",
        [
            "push 1 61 500 0 100",
            "push 1 62 1000 0 -",
            "get 1 100 0 600 0",
            "size 1",
            "head 1",
        ],
    ),
    (
        "forget",
        [
            "push 1 61 1000 0 100",
            "push 1 62 1000 0 -",
            "push 1 63 1000 0 -",
            "forget 1 101",
            "get 1 100 0 500 8",
            "forget 1 102",
            "size 1",
            "head 1",
            "tail 1",
            "replay 1",
        ],
    ),
    (
        "clear0",
        [
            "push 1 61 1000 0 100",
            "push 1 62 2000 0 -",
            "push 1 63 3000 0 -",
            "clear 1 0",
            "size 1",
            "head 1",
            "get 1 102 0 500 8",
            "replay 1",
        ],
    ),
    (
        "clear_keep",
        [
            "push 1 61 1000 0 100",
            "push 1 62 2000 0 -",
            "push 1 63 3000 0 -",
            "push 1 64 4000 0 -",
            "clear 1 2",
            "size 1",
            "head 1",
            "clear 1 5",
            "clear 9 0",
        ],
    ),
    (
        "gc",
        [
            "push 1 61 1000 0 100",
            "push 1 62 500 0 -",
            "gc 400",
            "size 1",
            "gc 700",
            "size 1",
            "head 1",
            "gc 5000",
            "size 1",
            "head 1",
        ],
    ),
    (
        "ids",
        [
            "id 0",
            "id 1",
            "id -1",
            "id 1999999999",
            "id 2000000000",
            "id 2147483647",
            "idnext 0",
            "idnext 1999999998",
            "idnext 1999999999",
            "idadv 100 5",
            "idadv 1999999990 20",
            "idadv 1 0",
            "idprint 0",
            "idprint 1",
            "idprint 1999999999",
        ],
    ),
    (
        "sanity",
        [
            "push 1 61 1000 0 100",
            "get 1 112 0 500 8",
            "get 1 110 0 500 8",
            "push 2 62 1000 0 200000",
            "get 2 99999 0 500 8",
            "get 2 100000 0 500 8",
            "get 9 100 0 500 8",
        ],
    ),
    (
        "replay",
        [
            "push 1 6161 1000 0 100",
            "push 1 6262 2000 0 -",
            "push 2 6363 3000 0 500",
            "replay 1 2 3",
        ],
    ),
]

CHECK_CASES = ["invalid_event_id", "valid_event_id"]


def narration(lines, command: str):
    """Lines whose first token is exactly `command`."""
    return [line for line in lines if line.split(" ", 1)[0] == command]


def compare_lines(name: str, cpp, rs) -> bool:
    if len(cpp) != len(rs):
        print(f"DIFF {name}: line count oracle={len(cpp)} rust={len(rs)}")
        for index in range(min(len(cpp), len(rs))):
            if cpp[index] != rs[index]:
                print(f"  first mismatch at line {index + 1}: oracle={cpp[index]!r} rust={rs[index]!r}")
                break
        return False
    for index, (left, right) in enumerate(zip(cpp, rs)):
        if left != right:
            print(f"DIFF {name} line {index + 1}: oracle={left!r} rust={right!r}")
            return False
    return True


def script_text(lines) -> str:
    return "\n".join(lines) + "\n"


def run(exe: Path, script: Path, env: dict):
    proc = subprocess.run([str(exe), str(script)], capture_output=True, env=env)
    return (
        proc.returncode,
        proc.stdout.decode("utf-8", "replace").splitlines(),
        proc.stderr.decode("utf-8", "replace").strip(),
    )


def run_check(exe: Path, case: str, env: dict):
    proc = subprocess.run([str(exe), "--check", case], capture_output=True, env=env)
    return (
        proc.returncode,
        proc.stdout.decode("utf-8", "replace").splitlines(),
        proc.stderr.decode("utf-8", "replace").strip(),
    )


def check_abort_pair(args, env) -> bool:
    expect = {"invalid_event_id": True, "valid_event_id": False}
    ok = True
    for case in CHECK_CASES:
        cpp_rc, cpp_out, _ = run_check(Path(args.cpp), case, env)
        rs_rc, rs_out, rs_err = run_check(Path(args.rs), case, env)
        cpp_abort = cpp_rc != 0
        rs_abort = rs_rc != 0
        if cpp_abort != expect[case]:
            print(
                f"FAIL check {case}: oracle rc={cpp_rc}, expected "
                f"{'an abort' if expect[case] else 'a clean exit'}"
            )
            ok = False
        if rs_abort != cpp_abort:
            print(f"DIFF check {case}: oracle rc={cpp_rc} rust rc={rs_rc} stderr={rs_err!r}")
            ok = False
        if not cpp_abort and cpp_out != rs_out:
            print(f"DIFF check {case} stdout: oracle={cpp_out!r} rust={rs_out!r}")
            ok = False
        print(f"  check {case}: abort={'yes' if cpp_abort else 'no'} cpp_rc={cpp_rc} rs_rc={rs_rc}")
    return ok


def selftest(cpp_lines: dict[str, list[str]]) -> bool:
    def want(condition: bool, message: str) -> bool:
        if not condition:
            print(f"SELFTEST FAIL: {message}")
        return condition

    ok = True

    # 1. the tail starts at the hint and advances; get returns the size.
    ok &= want(
        narration(cpp_lines["basic"], "head") == ["head q=1 id=100"]
        and narration(cpp_lines["basic"], "tail") == ["tail q=1 id=103"],
        "the first push takes the hint and the tail is one past the last id",
    )
    ok &= want(
        narration(cpp_lines["basic"], "get")[0]
        == "get q=1 ret=3 n=3 id=100 exp=1000 extra=7 data=6161 vlen=2 "
        "id=101 exp=2000 extra=0 data=6262 vlen=2 id=102 exp=3000 extra=9 data=6363 vlen=2",
        "get hands back every live event in id order",
    )
    ok &= want(
        narration(cpp_lines["basic"], "get")[1].startswith("get q=1 ret=2 n=2 id=100"),
        "forgetting the middle event leaves two",
    )

    # 2. a hint is only honoured while the tail is empty.
    ok &= want(len(narration(cpp_lines["hint"], "push")) == 3, "all three pushes succeeded")
    ok &= want(
        narration(cpp_lines["hint"], "tail") == ["tail q=1 id=102", "tail q=2 id=701"],
        "the second push to queue 1 ignores hint 500, queue 2 takes hint 700",
    )

    # 3. the six error messages, in the order the checks run.
    ok &= want(
        narration(cpp_lines["errors"], "push")
        == [
            "push err=Data is empty",
            "push err=Queue identifier is invalid",
            "push err=Failed to add already expired event",
            "push err=Failed to add already expired event",
            "push err=Data is too big",
            "push err=Data is too big",
        ],
        "empty data, queue 0, expired, then too-big -- and too-big still wins over expired",
    )

    # 4. an expired event is skipped and popped, but the returned number is the size.
    ok &= want(
        narration(cpp_lines["expiry"], "get")[0].startswith("get q=1 ret=2 n=2 id=101"),
        "the expired event is dropped and get returns the remaining size",
    )
    ok &= want(
        narration(cpp_lines["expiry"], "head") == ["head q=1 id=101"],
        "the dropped event is gone from the head",
    )

    # 5. max_events = 0 still sweeps the expired events from the front.
    ok &= want(
        narration(cpp_lines["max_zero"], "get") == ["get q=1 ret=1 n=0"],
        "max_events=0 returns nothing but still pops the expired event",
    )
    ok &= want(
        narration(cpp_lines["max_zero"], "head") == ["head q=1 id=101"],
        "the expired slot is gone even though nothing was returned",
    )

    # 6. forgetting the last event rewrites instead of removing.
    ok &= want(
        len(narration(cpp_lines["forget"], "get")) == 1
        and narration(cpp_lines["forget"], "get")[0].startswith("get q=1 ret=2 n=2 id=100"),
        "after forgetting the middle event the other two are still live",
    )
    ok &= want(
        narration(cpp_lines["forget"], "size") == ["size q=1 n=1"],
        "forget(101) drops one; forget(102) is the last event and only clears its data",
    )
    ok &= want(
        narration(cpp_lines["forget"], "head") == ["head q=1 id=100"],
        "the emptied last slot keeps its id but 100 is still the head",
    )
    ok &= want(
        narration(cpp_lines["forget"], "replay") == ["replay q=1 head=100 tail=103 size=1"],
        "replaying the storage keeps the emptied slot without counting it",
    )

    # 7. clear(0) keeps exactly one slot and returns size - 1.
    ok &= want(
        narration(cpp_lines["clear0"], "clear")
        == ["clear q=1 keep=0 returned=2 del=100,101 head=102 tail=103 size=0"],
        "clear(q, 0) returns two of the three events and keeps the last slot",
    )
    ok &= want(
        narration(cpp_lines["clear0"], "get") == ["get q=1 ret=0 n=0"],
        "the kept slot has no data, so get returns nothing",
    )
    ok &= want(
        narration(cpp_lines["clear0"], "replay") == ["replay q=1 head=102 tail=103 size=0"],
        "replay agrees the slot survived",
    )

    # 8. clear(keep) removes the front and is a no-op when there is little left.
    ok &= want(
        narration(cpp_lines["clear_keep"], "clear")
        == ["clear q=1 keep=2 returned=2 del=100,101 head=102 tail=104 size=2",
            "clear q=1 keep=5 returned=0 del=- head=102 tail=104 size=2",
            "clear q=9 keep=0 returned=0 del=- head=0 tail=0 size=0"],
        "keeping two deletes the two oldest; keeping five does nothing; an unknown queue is empty",
    )

    # 9. gc only runs when the scheduled time is strictly in the past.
    ok &= want(
        narration(cpp_lines["gc"], "gc")
        == ["gc now=400 deleted=0 completed=1", "gc now=700 deleted=0 completed=1",
            "gc now=5000 deleted=2 completed=1"],
        "gc at 400/700 does nothing because the queue is scheduled at 1000",
    )
    ok &= want(
        narration(cpp_lines["gc"], "size") == ["size q=1 n=2", "size q=1 n=2", "size q=1 n=0"],
        "the sweep only changes the size once it actually runs",
    )
    ok &= want(
        narration(cpp_lines["gc"], "head") == ["head q=1 id=100", "head q=1 id=101"],
        "after the sweep only the rewritten last slot remains",
    )

    # 10. EventId boundaries.
    ok &= want(
        narration(cpp_lines["ids"], "id")
        == [
            "id n=0 ok=1 value=0 valid=0 empty=1",
            "id n=1 ok=1 value=1 valid=1 empty=0",
            "id n=-1 ok=0 err=Invalid ID",
            "id n=1999999999 ok=1 value=1999999999 valid=1 empty=0",
            "id n=2000000000 ok=0 err=Invalid ID",
            "id n=2147483647 ok=0 err=Invalid ID",
        ],
        "0 is valid-but-empty; MAX_ID and above are rejected",
    )
    ok &= want(
        narration(cpp_lines["ids"], "idnext") == [
            "idnext n=0 ok=1 value=1",
            "idnext n=1999999998 ok=1 value=1999999999",
            "idnext n=1999999999 ok=0 err=Invalid ID",
        ],
        "next() crosses into MAX_ID and fails there",
    )
    ok &= want(
        narration(cpp_lines["ids"], "idadv")
        == ["idadv n=100 off=5 ok=1 value=105", "idadv n=1999999990 off=20 ok=0 err=Invalid ID",
            "idadv n=1 off=0 ok=1 value=1"],
        "advance adds then validates",
    )
    ok &= want(
        narration(cpp_lines["ids"], "idprint")
        == ["idprint n=0 text=EventId{0}", "idprint n=1 text=EventId{1}",
            "idprint n=1999999999 text=EventId{1999999999}"],
        "operator<< prints EventId{value}",
    )

    # 11. the two sanity windows.
    ok &= want(
        narration(cpp_lines["sanity"], "get")
        == [
            "get err=Specified from_id is in the future",
            "get q=1 ret=1 n=0",
            "get err=Specified from_id is in the past",
            "get q=2 ret=1 n=1 id=200000 exp=1000 extra=0 data=62 vlen=1",
            "get q=9 ret=0 n=0",
        ],
        "tail+10 is the future bound, head-100000 the past bound, unknown queues are empty",
    )

    # 12. replay rebuilds both queues from the storage.
    ok &= want(
        narration(cpp_lines["replay"], "replay")
        == ["replay q=1 head=100 tail=102 size=2", "replay q=2 head=500 tail=501 size=1",
            "replay q=3 head=0 tail=0 size=0"],
        "replay restores each queue's head/tail/size",
    )
    return ok


def disable_error_dialogs() -> None:
    """Abort cases must die at once: a WerFault dialog leaves a zombie holding the exe."""
    if sys.platform != "win32":
        return
    import ctypes

    ctypes.windll.kernel32.SetErrorMode(0x0001 | 0x0002 | 0x8000)


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

    text = script_text([f"# {name}" for name, _ in CASES] + sum((body for _, body in CASES), []))
    digest = hashlib.md5(text.encode()).hexdigest()
    print(f"corpus md5={digest} lines={len(text.splitlines())}")

    for sub in ("scripts",):
        shutil_rm = WORK / sub
        if shutil_rm.exists():
            for child in shutil_rm.iterdir():
                child.unlink()
        shutil_rm.mkdir(parents=True, exist_ok=True)

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    started = time.time()
    total_lines = 0
    cpp_lines: dict[str, list[str]] = {}

    for name, body in CASES:
        script = WORK / "scripts" / f"{name}.txt"
        script.write_text(script_text(body))
        cpp_rc, cpp_out, cpp_err = run(Path(args.cpp), script, env)
        rs_rc, rs_out, rs_err = run(Path(args.rs), script, env)
        if cpp_rc != rs_rc:
            print(f"DIFF {name} exit code: oracle={cpp_rc} rust={rs_rc} stderr={cpp_err!r} {rs_err!r}")
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