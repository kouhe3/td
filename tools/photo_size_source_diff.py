"""Judge `td/telegram/PhotoSizeSource.cpp`: C++ oracle vs Rust.

    uv run python tools/photo_size_source_diff.py
    uv run python tools/photo_size_source_diff.py --selftest

The oracle is `tools/fixtures/photo_size_source_probe.cpp` over `libtdcore.a`
(no `Td`, no actor). Ten variant offsets exist but only eight can be built; a
default-constructed source carries offset `-1` and aborts on any type query.

Criteria:

  (a) narration line by line: printing, `get_file_type`, `get_unique` (the
      compare-type folding and the 12/13-byte TL layout), `unique_less` /
      `unique_equal`, `get_unique_name`, `operator==`, `get_input_peer` and
      `get_input_sticker_set`;
  (b) FLOOR: a second oracle run prints the same lines;
  (c) four abort pairs: FullLegacy file type, StickerSetThumbnailLegacy unique
      name, a thumbnail type above 127, and the default-constructed source;
  (d) `--selftest`: hand-computed cells for `'a'`/`'c'` folding, the `+5` shift,
      the leading `0x02`, the decimal thumbnail type in the unique name, the
      dropped chat access hash, and "a default source is not equal to itself".

Not covered (return conditions in F254/F255): `store`/`parse` (declared but
never defined), the two unconstructible variants, and `DialogPhotoLegacy`'s
`get_input_peer` (only the small/big non-legacy branches are reachable through
`dialog_photo()`).
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
DEFAULT_CPP = "C:/tmp/td-probe/photo_size_source_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/photo_size_source_probe.exe"
WORK = REPO / "work" / "photo_size_source"

CASES = [
    (
        "printing",
        [
            "print t:0:97",
            "print t:5:98",
            "print d:u12345:777:0",
            "print d:n999:888:1",
            "print f:111:222:333",
            "print dl:c42:5:0:111:222",
            "print dl:c42:5:1:111:222",
            "print sl:900:901:111:222",
            "print sv:900:901:7",
        ],
    ),
    (
        "file_types",
        [
            "filetype t:0:97",
            "filetype t:1:97",
            "filetype t:5:97",
            "filetype d:u1:0:0",
            "filetype d:u1:0:1",
            "filetype dl:u1:0:0:1:2",
            "filetype dl:u1:0:1:1:2",
            "filetype sl:1:2:3:4",
            "filetype sv:1:2:3",
        ],
    ),
    (
        "unique",
        [
            "unique t:0:97",
            "unique t:0:99",
            "unique t:0:98",
            "unique t:5:120",
            "unique d:u1:0:0",
            "unique d:u1:0:1",
            "unique sv:900:901:7",
            "unique sl:1:2:3:4",
            "unique f:111:222:333",
            "unique dl:u1:0:0:5:6",
        ],
    ),
    (
        "ordering",
        [
            "less t:0:97 d:u1:0:0",
            "less d:u1:0:0 d:u1:0:1",
            "less sv:900:901:7 sv:1:2:8",
            "less sv:900:901:7 sv:900:901:8",
            "ueq sv:900:901:7 sv:900:999:7",
            "ueq sv:900:901:7 sv:901:901:7",
            "ueq t:0:97 t:5:97",
            "ueq f:1:2:3 f:1:2:4",
        ],
    ),
    (
        "names",
        [
            "name 1000 t:0:97",
            "name 1000 t:0:98",
            "name 1000 d:u1:0:0",
            "name 1000 d:u1:0:1",
            "name 1000 sv:900:901:7",
        ],
    ),
    (
        "equality",
        [
            "eq t:0:97 t:0:97",
            "eq t:0:97 t:5:97",
            "eq d:u1:0:0 d:u1:0:1",
            "eq f:1:2:3 f:1:2:3",
            "eq f:1:2:3 f:1:2:4",
            "eq sl:1:2:3:4 sl:1:2:3:4",
            "eq sv:1:2:3 sv:1:2:3",
            "eq l: l:",
        ],
    ),
    (
        "peers",
        [
            "peer d:u1:77:0",
            "peer d:c3:77:0",
            "peer d:n4:77:0",
            "peer d:s5:77:0",
            "peer d:z:77:0",
            "peer dl:u9:66:1:1:2",
        ],
    ),
    (
        "sticker_set",
        [
            "sset sv:900:901:7",
            "sset sl:5:6:7:8",
        ],
    ),
]

CHECK_CASES = ["filetype_full_legacy", "name_ss_legacy", "unique_thumb_wide", "type_default"]

# Hand-computed cells: (case, 0-based line index, expected text).
SELFTEST_CELLS = [
    ("printing", 0, "print type=1 text=PhotoSizeSourceThumbnail[Thumbnail, type = a]"),
    ("printing", 4, "print type=5 text=PhotoSizeSourceFullLegacy[]"),
    ("printing", 8, "print type=9 text=PhotoSizeSourceStickerSetThumbnailVersion[900_7]"),
    ("file_types", 0, "filetype=0"),
    ("file_types", 3, "filetype=1"),
    ("file_types", 7, "filetype=0"),
    ("unique", 0, "unique len=1 hex=00"),
    ("unique", 1, "unique len=1 hex=01"),
    ("unique", 2, "unique len=1 hex=67"),
    ("unique", 6, "unique len=13 hex=02840300000000000007000000"),
    ("unique", 8, "unique len=12 hex=6f00000000000000de000000"),
    ("ordering", 1, "less=1"),
    ("ordering", 3, "less=1"),
    ("ordering", 5, "ueq=0"),
    ("names", 0, "name=1000_97"),
    ("names", 3, "name=1000_1"),
    ("equality", 7, "eq eq=0 ne=1"),
    ("peers", 1, "peer=chat id=3"),
    ("peers", 3, "peer=null"),
    ("sticker_set", 0, "sset id=900 hash=901"),
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