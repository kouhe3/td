"""Judge `td::get_outline_svg_path` (td/telegram/Outline.cpp): C++ oracle vs Rust.

    uv run python tools/outline_diff.py
    uv run python tools/outline_diff.py --selftest

The function is a pure string transform: the compact path encoding is expanded
into an SVG path, the commands `M/m/L/l/H/h/V/v/C/c/S/s/Z/z` are normalized
together with `int`-truncated zooming, and the result is a compact string like
`M 10 10 L 20 20 L 10 10z`.

Criteria:

  (a) output string byte for byte;
  (b) FLOOR: a second oracle run prints the same lines;
  (c) `--selftest`: hand-computed cells (line closure, `S` reflection, zoom
      truncation, invalid command -> empty).

Facts (F225): the function prepends `M` and appends `z` to the compact path, so
the script carries the coordinate stream only; a closing `z` moves back to the
subpath start only when the current point differs, and `int` casts truncate
(`1.5 * 10 = 15`, not 16).

Not covered (return conditions in F225): `get_outline_object` (it builds the
`td_api::outline` object, judged by the ABI lane at most) and inputs whose
compressed form uses the `>= 64` / `>= 128` shorthand (the probe always emits
table bytes).
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
DEFAULT_CPP = "C:/tmp/td-probe/outline_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/outline_probe.exe"
WORK = REPO / "work" / "outline"

CASES = [
    (
        "lines",
        [
            "path 1 10,10 L20,20",
            "path 1 10,10 l5,-5",
            "path 1 0,0 H10 V10",
            "path 1 0,0 h10 v10",
            "path 1 10,10 20,20 30,30",
            "path 1 1,1.5 L2.25,3.75",
        ],
    ),
    (
        "curves",
        [
            "path 1 0,0 C1,1,2,2,3,3",
            "path 1 0,0 c1,1,2,2,3,3",
            "path 1 0,0 S1,1,2,2",
            "path 1 0,0 C0,0,1,1,2,2 S3,3,4,4",
            "path 1 0,0 s1,1,2,2",
            "path 2 0,0 C1,1,2,2,3,3",
        ],
    ),
    (
        "implicit",
        [
            "path 1 0,0 L1,1,2,2",
            "path 1 0,0 l1,1,2,2",
            "path 1 0,0 h5,5",
            "path 1 0,0 v5,5",
        ],
    ),
    (
        "subpaths",
        [
            "path 1 0,0 L1,1 L2,2 L0,0",
            "path 1 0,0 M5,5 L6,6",
            "path 1 0,0 l3,3 l0,0",
        ],
    ),
    (
        "zoom",
        ["path 1 0,0 L10,10", "path 2 0,0 L10,10", "path 1.5 0,0 L10,10", "path 0.5 0,0 L3,7", "path 100 0,0 L3,7"],
    ),
    (
        "invalid",
        ["path 1 0,0 Q1,1", "path 1 0,0 L1,1", "path 1 0,0 T2,2"],
    ),
]

SELFTEST_CELLS = [
    ("lines", 0, "path zoom=1 out=M 10 10 L 20 20 L 10 10z"),
    ("lines", 1, "path zoom=1 out=M 10 10 L 15 5 L 10 10z"),
    ("lines", 2, "path zoom=1 out=M 0 0 L 10 0 L 10 10 L 0 0z"),
    ("curves", 0, "path zoom=1 out=M 0 0 C 1 1 2 2 3 3 L 0 0z"),
    ("curves", 2, "path zoom=1 out=M 0 0 C 0 0 1 1 2 2 L 0 0z"),
    ("curves", 3, "path zoom=1 out=M 0 0 C 0 0 1 1 2 2 C 3 3 3 3 4 4 L 0 0z"),
    ("zoom", 2, "path zoom=1.5 out=M 0 0 L 15 15 L 0 0z"),
    ("invalid", 0, "path zoom=1 out=-"),
    ("invalid", 1, "path zoom=1 out=M 0 0 L 1 1 L 0 0z"),
]


def script_text(lines) -> str:
    return "\n".join(lines) + "\n"


def disable_error_dialogs() -> None:
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
    print(f"PASS cases={len(CASES)} lines={total_lines} elapsed={elapsed:.2f}s")
    return 0


if __name__ == "__main__":
    sys.exit(main())