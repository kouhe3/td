"""Judge `td::FormattedDate` (`td/telegram/FormattedDate.{h,cpp,hpp}`): C++ vs Rust.

    uv run python tools/formatted_date_diff.py
    uv run python tools/formatted_date_diff.py --selftest

The oracle is `tools/fixtures/formatted_date_probe.cpp` over `libtdcore.a` (no
`Td`, no actor, no clock). The whole surface is flags arithmetic plus two
decoders, so one script feeds both sides.

Criteria:

  (a) narration line by line: both `get_formatted_date` overloads (error text
      included), the seven-bool constructor, `get_date_time_formatting_type_object`
      (its five branches), `get_input_message_entity_formatted_date`, and
      `get_input_text_date`;
  (b) FLOOR: a second oracle run prints the same lines;
  (c) `--selftest`: hand-computed cells pinning the two asymmetries --
      `get_date_flags("dD")` ORs both bits (24) while the constructor's
      `if short_date else if long_date` keeps only 8, and only the `td_api`
      overload rejects `date <= 0`;
  (d) no abort pairs: the only `UNREACHABLE` sits in
      `get_formatted_date(date, type_ptr)`'s `default:`, and
      `DateTimeFormattingType` has exactly two subclasses, so a third one cannot
      be constructed (F283).

Not covered (return conditions in F283/Spec): `store`/`parse` (two int32s, no
independent semantics) and that unreachable `UNREACHABLE`.
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
DEFAULT_CPP = "C:/tmp/td-probe/formatted_date_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/formatted_date_probe.exe"
WORK = REPO / "work" / "formatted_date"

DATE = "1700000000"

CASES = [
    (
        "formats",
        [
            f"fmt {DATE} r",
            f"fmt {DATE} R",
            f"fmt {DATE} rr",
            f"fmt {DATE} rt",
            f"fmt {DATE} rR",
            f"fmt {DATE} t",
            f"fmt {DATE} T",
            f"fmt {DATE} d",
            f"fmt {DATE} D",
            f"fmt {DATE} w",
            f"fmt {DATE} W",
            f"fmt {DATE} dD",
            f"fmt {DATE} Dd",
            f"fmt {DATE} wdt",
            f"fmt {DATE} TWD",
            f"fmt {DATE} -",
            f"fmt {DATE} x",
            f"fmt {DATE} 1",
            "fmt 0 r",
            "fmt -5 r",
            "fmt 2147483647 d",
            "fmt -2147483648 D",
            "fmt 2147483648 d",
        ],
    ),
    (
        "api_types",
        [
            f"api {DATE} none",
            f"api {DATE} rel",
            f"api {DATE} abs:n:n:0",
            f"api {DATE} abs:s:n:0",
            f"api {DATE} abs:l:n:0",
            f"api {DATE} abs:n:s:0",
            f"api {DATE} abs:n:l:0",
            f"api {DATE} abs:n:n:1",
            f"api {DATE} abs:s:l:1",
            f"api {DATE} abs:l:s:1",
            f"api {DATE} abs:s:s:0",
            "api 0 rel",
            "api 0 none",
            "api -5 abs:s:l:1",
            f"api {DATE} abs:x:n:0",
            f"api {DATE} rel:1",
        ],
    ),
    (
        "constructor",
        [
            f"ctr {DATE} 0 0 0 0 0 0",
            f"ctr {DATE} 1 0 0 0 0 0",
            f"ctr {DATE} 1 1 1 1 1 1",
            f"ctr {DATE} 0 1 0 0 0 0",
            f"ctr {DATE} 0 0 1 0 0 0",
            f"ctr {DATE} 0 1 1 0 0 0",
            f"ctr {DATE} 0 0 0 1 0 0",
            f"ctr {DATE} 0 0 0 0 1 0",
            f"ctr {DATE} 0 0 0 1 1 0",
            f"ctr {DATE} 0 0 0 0 0 1",
            f"ctr {DATE} 0 1 1 1 1 1",
            f"ctr {DATE} 0 1 0 1 0 1",
            "ctr 0 0 0 0 0 0 0",
            "ctr -1 0 1 0 0 0 0",
        ],
    ),
    (
        "type_objects",
        [
            f"typeobj {DATE} 0",
            f"typeobj {DATE} 1",
            f"typeobj {DATE} 2",
            f"typeobj {DATE} 4",
            f"typeobj {DATE} 8",
            f"typeobj {DATE} 16",
            f"typeobj {DATE} 32",
            f"typeobj {DATE} 6",
            f"typeobj {DATE} 24",
            f"typeobj {DATE} 33",
            f"typeobj {DATE} 63",
            "typeobj 0 0",
            "typeobj -7 0",
        ],
    ),
    (
        "telegram_api",
        [
            f"entity {DATE} 0 0 0",
            f"entity {DATE} 63 3 7",
            f"entity {DATE} 1 -1 0",
            f"entity {DATE} 40 2147483647 2147483647",
            f"textdate {DATE} 0",
            f"textdate {DATE} 21",
            f"textdate {DATE} 63",
            "textdate 0 1",
        ],
    ),
    (
        "compare",
        [
            f"eq {DATE} 2 {DATE} 2",
            f"eq {DATE} 2 {DATE} 3",
            f"eq {DATE} 0 {DATE} 1",
            "eq 0 2 1 2",
            "eq -1 2 1 2",
            f"print {DATE} 42",
            "print 0 1",
        ],
    ),
    (
        "rejects",
        [
            "fmt 1",
            "fmt a r",
            f"fmt {DATE}",
            "api 1",
            f"api {DATE}",
            "ctr 1 0",
            f"ctr {DATE} 0 0 0 0 0 x",
            "typeobj 1",
            "entity 1 2 3",
            "textdate 1",
            "eq 1 2 3",
            "print 1",
            "nope 1 2",
        ],
    ),
]


def script_text(lines) -> str:
    return "\n".join(lines) + "\n"


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


# (case, index, expected line). Indices are 0-based inside the case.
SELFTEST_CELLS = [
    # the two decoders disagree on "dD" vs (short_date, long_date)
    ("formats", 11, f"fmt {DATE} dD valid=1 date {DATE} with flags 24"),
    ("formats", 12, f"fmt {DATE} Dd valid=1 date {DATE} with flags 24"),
    ("formats", 14, f"fmt {DATE} TWD valid=1 date {DATE} with flags 52"),
    ("formats", 15, f"fmt {DATE} - valid=1 date {DATE} with flags 0"),
    ("formats", 3, f"fmt {DATE} rt err=Invalid date format used"),
    # only the td_api overload rejects a non-positive date
    ("formats", 18, "fmt 0 r valid=0 date 0 with flags 1"),
    ("api_types", 11, "api 0 rel err=Invalid date specified"),
    ("api_types", 12, "api 0 none err=Invalid date specified"),
    ("api_types", 0, f"api {DATE} none date {DATE} with flags 0"),
    ("api_types", 8, f"api {DATE} abs:s:l:1 date {DATE} with flags 50"),
    # the constructor: relative wins, short wins over long
    ("constructor", 2, f"ctr {DATE} 111111 valid=1 date {DATE} with flags 1"),
    ("constructor", 10, f"ctr {DATE} 011111 valid=1 date {DATE} with flags 42"),
    ("constructor", 8, f"ctr {DATE} 000110 valid=1 date {DATE} with flags 8"),
    ("constructor", 9, f"ctr {DATE} 000001 valid=1 date {DATE} with flags 32"),
    # the type object: 0 is null, bit 0 wins over everything else
    ("type_objects", 0, f"typeobj {DATE} 0 null"),
    ("type_objects", 10, f"typeobj {DATE} 63 rel"),
    ("type_objects", 8, f"typeobj {DATE} 24 abs:none:short:0"),
    # bit 0 (Relative) wins over every other bit, day_of_week included
    ("type_objects", 9, f"typeobj {DATE} 33 rel"),
    # telegram_api: six flag booleans in TL order, then the payload fields
    ("telegram_api", 1, f"entity {DATE} 63 3 7 rel=1 st=1 lt=1 sd=1 ld=1 dow=1 off=3 len=7 date={DATE}"),
    ("telegram_api", 5, f"textdate {DATE} 21 rel=1 st=0 lt=1 sd=0 ld=1 dow=0 text=0 date={DATE}"),
    # equality is (date, flags) pair equality
    ("compare", 0, f"eq {DATE} 2 {DATE} 2 eq=1 ne=0"),
    ("compare", 2, f"eq {DATE} 0 {DATE} 1 eq=0 ne=1"),
]


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