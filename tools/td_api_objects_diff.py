"""Judge the `td_api` object face: the generated JSON printer plus the three
assembly functions (`Usernames::get_usernames_object`,
`get_outline_object`, `MessageId::get_message_id`). C++ oracle vs Rust.

    uv run python tools/td_api_objects_diff.py
    uv run python tools/td_api_objects_diff.py --selftest

The oracle (`tools/fixtures/td_api_objects_probe.cpp`) links the generated
`td_api_json_*.cpp.obj` of the `tdjson_private` object library, because
`td_api::to_json` is in no static archive (facts F234).

Criteria:

  (a) narration line by line, the JSON payload byte for byte;
  (b) FLOOR: a second oracle run prints the same lines;
  (c) `--check badutf`: an invalid UTF-8 string makes JsonBuilder CHECK, so both
      sides must print the same prefix and die;
  (d) `--selftest`: hand-computed cells pinning the printer rules
      (`int64` as a quoted string, `bytes` as base64, six-decimal doubles, a
      null object field omitted, `paths=[]` for a subpath without commands).

Not covered (return conditions in `docs/SPEC-1b9efe84d-tdjson-abi.md` §4): the
TL `store` form, and the other ~1100 generated printers beyond this family.
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
DEFAULT_CPP = "C:/tmp/td-probe/td_api_objects_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/td_api_objects_probe.exe"
WORK = REPO / "work" / "td_api_objects"

CASES = [
    (
        "printer",
        ["rules"],
    ),
    (
        "printer_strings",
        [
            'obj hex-612262 - hex-5c2c2f - - -',
            'obj hex-6108620a0d09 - hex-c3a920 - hex-f09f9880 - -',
            'obj hex-7f - hex-00 - - -',
        ],
    ),
    (
        "usernames_object",
        [
            "ulist - a:1:1:0:0,b:0:0:0:0",
            "ulist alice -",
            "ulist - -",
            "ulist - a:1:0:1:0",
            "ulist - a:1:1:0:0,b:1:0:0:0",
            "ulist - a:1:0:0:1,b:0:0:0:0",
            "ulist - a:1:0:0:0,a:1:0:0:0",
            "ulist - hex-61ff:1:0:0:0",
            "ulist - hex-61ff:0:0:0:0",
        ],
    ),
    (
        "outline_object",
        [
            "outline 1",
            "outline 1 M0,0 z",
            "outline 1 M0,0 L10,10",
            "outline 2 M0,0 L10,10",
            "outline 1.5 M0,0 H5 V5",
            "outline 1 M0,0 C1,1,2,2,3,3",
            "outline 1 M0,0 S1,1,2,2",
            "outline 1 M0,0 C0,0,1,1,2,2 S3,3,4,4",
            "outline 1 M0,0 L1,1 M5,5 L6,6",
            "outline 1 M0,0 Q1,1",
            "outline 1 M0,0 L1,1",
        ],
    ),
    (
        "message_ids",
        [
            "msgid message 5 0 0",
            "msgid service 5 2000000000 0",
            "msgid service 5 2000000000 1",
            "msgid service 5 1000 1",
            "msgid empty 789 0 0",
            "msgid empty 789 0 1",
            "msgid message 0 0 0",
            "msgid message -5 0 0",
            "msgid message 2147483647 0 0",
            "msgid service 262144 2000000000 1",
        ],
    ),
]

CHECK_CASES = ["badutf"]

# Hand-computed cells: (case, 0-based line index, expected text). They lock the
# printing rules the corpus is built on, so a broken harness cannot pass by
# comparing two equally broken outputs.
SELFTEST_CELLS = [
    ("printer", 0, 'ok={"@type":"ok"}'),
    ("printer", 1, 'error={"@type":"error","code":0,"message":""}'),
    ("printer", 4, 'data={"@type":"data","data":"AAH/fw=="}'),
    ("printer", 5, 'dataempty={"@type":"data","data":""}'),
    ("printer", 6, 'point={"@type":"point","x":1.500000,"y":-0.000000}'),
    (
        "printer",
        11,
        'posmax={"@type":"chatPosition","list":{"@type":"chatListMain"},'
        '"order":"9223372036854775807","is_pinned":true}',
    ),
    ("printer", 13, 'posnull={"@type":"chatPosition","order":"0","is_pinned":false}'),
    ("printer", 14, 'cpath_empty={"@type":"closedVectorPath","commands":[]}'),
    (
        "usernames_object",
        0,
        'ulist={"@type":"usernames","active_usernames":["a"],"disabled_usernames":["b"],'
        '"editable_username":"a","collectible_usernames":["b"],"secondary_username":""}',
    ),
    ("outline_object", 0, "outline=null"),
    ("outline_object", 1, 'outline={"@type":"outline","paths":[]}'),
    ("message_ids", 0, "msgid raw=5242880 valid=1 sched=0 type=1 text=server message 5"),
    ("message_ids", 3, "msgid raw=0 valid=0 sched=0 type=0 text=invalid message 0"),
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
        cpp_rc, cpp_out, cpp_err = run_check(Path(args.cpp), case, env)
        rs_rc, rs_out, rs_err = run_check(Path(args.rs), case, env)
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