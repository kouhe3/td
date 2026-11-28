"""Judge the FileManager-free half of `td/telegram/DialogPhoto.cpp`: C++ oracle vs Rust.

    uv run python tools/dialog_photo_diff.py
    uv run python tools/dialog_photo_diff.py --selftest

The oracle is `tools/fixtures/dialog_photo_probe.cpp` over `libtdcore.a` (no
`Td`, no actor), plus `need_update_dialog_photo_minithumbnail` from
`td/telegram/PhotoSize.cpp`. The other half of the header takes a live
`FileManager *` and is `blocked` (points 1-2 of F270).

Criteria:

  (a) narration line by line: profile photo ids, `as_fake_dialog_photo` (the
      empty-collapse included), file ids, both `need_update_*`,
      `need_update_dialog_photo_minithumbnail`, both `operator<<`, both
      `operator==`/`!=`, and `convert_photo_to_profile_photo`;
  (b) FLOOR: a second oracle run prints the same lines;
  (c) no abort pairs: this half has no `CHECK`/`UNREACHABLE` path. The blocked
      half's return condition is a `static_assert(std::is_final_v<FileManager>)`
      in the fixture — if upstream drops `final`, the fixture stops compiling and
      F270 has to be re-judged;
  (d) `--selftest`: hand-computed cells pinning the three facts — `need_update`
      ignores `minithumbnail` while `==` compares it, `as_fake_dialog_photo`
      collapses to an empty struct when either side is missing, and the
      minithumbnail comparison is on decoded sizes with the 8x8 rule.

Not covered (return conditions in F270/Spec): the eight `FileManager *` functions.
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
DEFAULT_CPP = "C:/tmp/td-probe/dialog_photo_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/dialog_photo_probe.exe"
WORK = REPO / "work" / "dialog_photo"

CASES = [
    ("profile_id", ["pid none", "pid empty", "pid photo=777", "pid photo=0"]),
    (
        "fake",
        [
            "fake id=5,m=01ff10,sz=a3;c4,an=0 42 0",
            "fake id=5,m=01ff10,sz=a3;c4,an=1 42 1",
            "fake id=5,m=01ff10,sz=a3,an=1 42 1",
            "fake id=5,m=01ff10,sz=c4,an=1 42 1",
            "fake id=-2,m=01ff10,sz=a3;c4,an=1 42 1",
            "fake id=5,m=,sz=a3;c4;z9,an=0 7 1",
        ],
    ),
    ("file_ids", ["ids id=5,sz=a3;c4 42 0", "ids id=5,sz=a3 42 0", "ids id=-2,sz=a3;c4 42 0"]),
    (
        "need_update",
        [
            "need s=1,b=2,m=aa,a=0,p=0 s=1,b=2,m=bb,a=0,p=0",
            "need s=1,b=2,m=aa,a=0,p=0 s=1,b=3,m=aa,a=0,p=0",
            "need s=1,b=2,m=aa,a=1,p=0 s=1,b=2,m=aa,a=0,p=0",
            "need s=1,b=2,m=aa,a=0,p=0 s=1,b=2,m=aa,a=0,p=1",
            "need s=1,b=2,m=aa,a=0,p=0 s=1,b=2,m=aa,a=0,p=0",
        ],
    ),
    (
        "need_update_profile",
        [
            "needp id=9,s=1,b=2 s=1,b=2",
            "needp s=1,b=2 s=1,b=2",
            "needp id=9,s=1,b=2,m=aa id=9,s=1,b=2,m=bb",
        ],
    ),
    (
        "minithumbnail",
        [
            "mneed 01ff10 01ff20",
            "mneed 01ff10 01ff30",
            "mneed 01ff10 -",
            "mneed - 01ff10",
            "mneed 01ff10 01ff10",
            "mneed 010508 01ff05",
            "mneed 010808 010909",
            "mneed 02ff10 01ff20",
            "mneed 01 01",
        ],
    ),
    (
        "printing",
        [
            "print s=1,b=2,m=aa,a=1,p=1",
            "print s=0,b=0,m=,a=0,p=0",
            "printp id=7,s=3,b=4,m=,a=0,p=1",
            "printp id=0,s=0,b=0,m=,a=1,p=0",
        ],
    ),
    (
        "equality",
        [
            "eq s=1,b=2,m=aa s=1,b=2,m=bb",
            "eq s=1,b=2,m=aa s=1,b=2,m=aa",
            "eq s=1,b=2,m=aa,a=1 s=1,b=2,m=aa,a=0",
            "eqp id=1,s=1,b=2 id=2,s=1,b=2",
            "eqp id=1,s=1,b=2,m=aa id=1,s=1,b=2,m=bb",
        ],
    ),
    (
        "convert",
        [
            "conv id=9,dc=2,sz=a;c,v=0 0",
            "conv id=9,dc=2,sz=a;c,v=1 1",
            "conv id=9,dc=2,sz=a;d,v=0 0",
            "conv id=9,dc=2,sz=c,v=0 0",
            "conv id=9,dc=2,sz=d;d,v=0 0",
        ],
    ),
]

# This half has no CHECK/UNREACHABLE path, so there is no abort pair to run.
CHECK_CASES: list[str] = []

# Hand-computed cells: (case, 0-based line index, expected text).
SELFTEST_CELLS = [
    ("profile_id", 0, "pid=0"),
    ("profile_id", 2, "pid=777"),
    ("fake", 0, "fake=s=3 b=4 mini=01ff10 an=0 p=0"),
    # only 'a' present: the whole struct collapses to empty
    ("fake", 2, "fake=s=0 b=0 mini=- an=0 p=0"),
    # an empty Photo is not a collapse: it is simply left empty
    ("fake", 4, "fake=s=0 b=0 mini=- an=0 p=0"),
    ("file_ids", 0, "ids=2:3:4"),
    ("file_ids", 1, "ids=0"),
    # different minithumbnail, same ids: need_update false
    ("need_update", 0, "need=0"),
    ("need_update", 1, "need=1"),
    ("need_update_profile", 0, "needp=1"),
    ("minithumbnail", 0, "mneed=1"),
    ("minithumbnail", 2, "mneed=0"),
    ("minithumbnail", 3, "mneed=0"),
    ("printing", 0, "print=<small_file_id = 1(0), big_file_id = 2(0), has_animation = true, is_personal = true>"),
    # different minithumbnail, same ids: == is false while need_update is false
    ("equality", 0, "eq=0 ne=1"),
    ("equality", 1, "eq=1 ne=0"),
    ("convert", 0, "conv=flags=0 video=0 personal=0 photo=9 stripped=0 dc=2"),
    ("convert", 2, "conv=null"),
]


def script_text(lines) -> str:
    return "\n".join(lines) + "\n"


def disable_error_dialogs() -> None:
    if sys.platform != "win32":
        return
    import ctypes

    ctypes.windll.kernel32.SetErrorMode(0x0001 | 0x0002 | 0x8000)


def run(exe: Path, script: Path, env: dict):
    proc = subprocess.run([str(exe), str(script)], capture_output=True, text=True, errors="replace", env=env)
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

    if args.selftest and not selftest(cpp_lines):
        return 1

    elapsed = time.time() - started
    print(f"PASS cases={len(CASES)} checks={len(CHECK_CASES)} lines={total_lines} elapsed={elapsed:.2f}s")
    return 0


if __name__ == "__main__":
    sys.exit(main())