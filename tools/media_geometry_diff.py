"""Judge the media-geometry face (`td::Dimensions`, `td::MediaAreaCoordinates`):
C++ oracle vs Rust.

    uv run python tools/media_geometry_diff.py
    uv run python tools/media_geometry_diff.py --selftest

The oracle is `tools/fixtures/media_geometry_probe.cpp` over `libtdcore.a` (no
`Td`, no actor). Every double that reaches the object is echoed as its 64-bit
pattern, so the narration stays exact where the printed form rounds.

Criteria:

  (a) narration line by line, the printed form and the patterns byte for byte;
  (b) FLOOR: a second oracle run prints the same lines;
  (c) `--check obj_invalid` / `--check input_invalid`: both accessors CHECK
      `is_valid()`, so both sides must print the same prefix and die;
  (d) `--selftest`: hand-computed cells pinning the pair-zeroing, the clamp, the
      rotation normalisation, the recomputed flags and the binary `1e-6`
      equality boundary.

Not covered (return conditions in F239): `store`/`parse` (TL storer) and the
other geometry-adjacent value types (`PhotoSizeSource`, `PhotoSizeType`).
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
DEFAULT_CPP = "C:/tmp/td-probe/media_geometry_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/media_geometry_probe.exe"
WORK = REPO / "work" / "media_geometry"

CASES = [
    (
        "dimensions",
        [
            "dim 100 200",
            "dim 0 100",
            "dim 100 0",
            "dim -1 200",
            "dim 65535 65535",
            "dim 65536 10",
            "dim 2147483647 -2147483648",
            "dimcmp 1 2 1 2",
            "dimcmp 1 2 1 3",
            "dimcmp 0 5 7 0",
        ],
    ),
    (
        "areas",
        [
            "area 10 20 30 40 0 0",
            "area -5 120 -1 40 -90 400",
            "area nan inf -inf 50 45 5",
            "area 10 20 30 40 -360 0",
            "area 10 20 30 40 -90 5",
            "area 10 20 30 40 400 5",
            "area 10 20 30 40 360.0000001 5",
            "area 10 20 30 40 -360.0000001 5",
            "area 100 100 100 100 359.5 100",
            "area 0 0 0.5 0.5 0 0",
            "tarea 3 10 20 30 40 45 5",
            "tarea 0 10 20 30 40 45 0",
            "tarea 7 10 20 30 40 45 -1",
        ],
    ),
    (
        "equality",
        [
            "eq 10 20 30 40 45 5 10 20 30 40 45 5",
            "eq 10 20 30 40 45 5 10.000001 20 30 40 45 5",
            "eq 10 20 30 40 45 5 10.000002 20 30 40 45 5",
            "eq 10 20 30 40 45 5 10 20 30 40 45 5.000001",
            "eq nan 20 30 40 45 5 nan 20 30 40 45 5",
            "eq -1 20 30 40 45 5 0 20 30 40 45 5",
        ],
    ),
]

CHECK_CASES = ["obj_invalid", "input_invalid"]

# Hand-computed cells: (case, 0-based line index, expected text). They lock the
# rules the corpus is built on, so a broken harness cannot pass by comparing two
# equally broken outputs.
SELFTEST_CELLS = [
    ("dimensions", 0, "dim w=100 h=200 px=20000 print=(100, 200)"),
    ("dimensions", 2, "dim w=0 h=0 px=0 print=(0, 0)"),
    ("dimensions", 4, "dim w=65535 h=65535 px=4294836225 print=(65535, 65535)"),
    (
        "areas",
        1,
        '  json={"@type":"areaPosition","x_percentage":10.000000,"y_percentage":20.000000,'
        '"width_percentage":30.000000,"height_percentage":40.000000,"rotation_angle":0.000000,'
        '"corner_radius_percentage":0.000000}',
    ),
    (
        "areas",
        3,
        "area in=[c014000000000000 405e000000000000 bff0000000000000 4044000000000000 "
        "c056800000000000 4079000000000000] valid=0 print=AreaPosition[0.000000, 100.000000, "
        "0.000000, 40.000000, 270.000000, 100.000000]",
    ),
    (
        "areas",
        5,
        "area in=[4024000000000000 4034000000000000 403e000000000000 4044000000000000 "
        "c076800000000000 0000000000000000] valid=1 print=AreaPosition[10.000000, 20.000000, "
        "30.000000, 40.000000, 0.000000, 0.000000]",
    ),
    (
        "areas",
        11,
        "area in=[4024000000000000 4034000000000000 403e000000000000 4044000000000000 "
        "4079000000000000 4014000000000000] valid=1 print=AreaPosition[10.000000, 20.000000, "
        "30.000000, 40.000000, 360.000000, 5.000000]",
    ),
    (
        "areas",
        28,
        "  input flags=1 bits=[4024000000000000 4034000000000000 403e000000000000 "
        "4044000000000000 4046800000000000 4014000000000000]",
    ),
    ("equality", 1, "eq eq=1 ne=0"),
    ("equality", 2, "eq eq=0 ne=1"),
    ("equality", 4, "eq eq=1 ne=0"),
    ("equality", 5, "eq eq=1 ne=0"),
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