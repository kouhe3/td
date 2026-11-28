"""Judge `tdutils/td/utils/emoji.h` (`is_emoji`, `find_emojis`, four modifier
helpers): C++ oracle vs the Rust port.

    uv run python tools/emoji_diff.py
    uv run python tools/emoji_diff.py --selftest

The oracle is `tools/fixtures/emoji_probe.cpp` over `libtdutils.a` (no `Td`, no
actor, no clock). Three criteria shapes sit next to each other here:

  (a) **judgement** -- `is_emoji` is a boolean; the corpus walks all 2370 table
      entries plus the border cases (empty, stacked selectors, ZWJ, truncated
      UTF-8, a 32-byte non-emoji);
  (b) **structure** -- `find_emojis` returns slices, printed as a hex list;
  (c) **byte-exact** -- `remove_fitzpatrick_modifier`, `remove_emoji_modifiers`,
      `remove_emoji_selectors` and `get_fitzpatrick_modifier`'s value.

Plus FLOOR (a second oracle run) and hand-computed cells.

The corpus is derived from `rust/src/emoji_table.rs`, so the table itself is part
of the judgement: if the generated table drifts, every `is` line changes.

Not covered (return conditions in F293/Spec): the `CHECK(is_emoji(str))` at the
end of `remove_emoji_selectors` -- every selector removal seen so far lands on
another table entry (`#` + VS16 + keycap -> `#` + keycap).
"""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/emoji_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/emoji_probe.exe"
WORK = REPO / "work" / "emoji_lane"

TABLE = REPO / "rust/src/emoji_table.rs"
TABLE_COUNT = 2370

THUMBS = "f09f918d"  # 👍
VS16 = "efb88f"
FITZ_12 = "f09f8fbb"  # 🏻
FITZ_6 = "f09f8fbf"  # 🏿
ZWJ = "e2808d"
FAMILY = THUMBS + ZWJ + THUMBS + ZWJ + THUMBS  # not the real family, but ZWJ-shaped

BORDERS = [
    "-",
    THUMBS,
    THUMBS + VS16,
    THUMBS + VS16 + VS16,
    THUMBS + FITZ_12,
    "f09f91a8" + ZWJ + "f09f91a9" + ZWJ + "f09f91a7",  # 👨‍👩‍👧
    "f09f91a8" + ZWJ + "f09f91a9" + ZWJ + "f09f91a7" + ZWJ + "f09f91a6",  # + 👦
    "f09f91a8" + ZWJ,
    ZWJ,
    "e2",
    "e280",
    THUMBS + THUMBS,
    THUMBS + "61",
    "61" * 32,
    "23e283a3",  # # + keycap
    "23" + VS16 + "e283a3",  # # + VS16 + keycap
    "f09f91",  # truncated 👍
    "ff",
    "e2" + "80" + "8d" + "61",  # ZWJ + 'a'
    "f09f918d" + "e2808d" + "61",  # 👍 + ZWJ + 'a'
]

FIND = [
    "61" + THUMBS + "62",
    THUMBS + THUMBS,
    "f09f91a8" + ZWJ + "f09f91a9" + ZWJ + "f09f91a7",
    THUMBS + VS16,
    "23" + VS16 + "e283a3",
    THUMBS + VS16 + THUMBS,
    "61" * 8,
    "-",
    "f09f91a8" + ZWJ + "f09f91a9" + ZWJ + "61",
]

MODIFIERS = [
    ("fp", THUMBS),
    ("fp", THUMBS + FITZ_12),
    ("fp", THUMBS + FITZ_6),
    ("fp", THUMBS + FITZ_12 + FITZ_12),
    ("fp", "f09f918d" + "f09f8fba"),
    ("rfp", THUMBS + FITZ_12),
    ("rfp", THUMBS + FITZ_12 + FITZ_12),
    ("rfp", THUMBS),
    ("rem", THUMBS + VS16 + " 0"),
    ("rem", THUMBS + VS16 + " 1"),
    ("rem", THUMBS + FITZ_12 + " 0"),
    ("rem", THUMBS + FITZ_12 + " 1"),
    ("rem", "f09f91a8" + ZWJ + "e29980" + " 0"),
    ("rem", "f09f91a8" + ZWJ + "e29980" + " 1"),
    ("rem", "61" + VS16 + " 1"),
    ("rs", THUMBS + VS16),
    ("rs", THUMBS + FITZ_12),
    ("rs", "23" + VS16 + "e283a3"),
    ("rs", "61"),
    ("rs", "-"),
]

REJECTS = ["is", "is xy", "is ffz", "find", "fp", "rfp", "rs", "rem 61", "rem 61 2", "nope 61"]


def to_hex(text: str) -> str:
    data = text.encode("utf-8")
    return "-" if not data else data.hex()


def table_entries() -> list[str]:
    """The 2370 UTF-8 entries of the generated table, in file order."""
    entries = []
    for line in TABLE.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if stripped.startswith('"') and stripped.endswith('",'):
            entries.append(stripped[1:-2])
    return entries


def build_cases() -> list[tuple[str, list[str]]]:
    entries = table_entries()
    assert len(entries) == TABLE_COUNT, f"expected {TABLE_COUNT} table entries, got {len(entries)}"
    cases = [
        ("table", [f"is {to_hex(entry)}" for entry in entries]),
        ("borders", [f"is {case}" for case in BORDERS]),
        ("find", [f"find {case}" for case in FIND]),
        ("modifiers", [f"{command} {argument}" for command, argument in MODIFIERS]),
        ("rejects", REJECTS),
    ]
    return cases


def script_text(lines) -> str:
    return "\n".join(lines) + "\n"


def run(exe: Path, script: Path, env: dict):
    proc = subprocess.run([str(exe), str(script)], capture_output=True, encoding="utf-8", errors="replace", env=env)
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


# (case, index, expected line). `table` indices follow file order; the three
# first entries are "#⃣", "#️⃣", "*⃣".
SELFTEST_CELLS = [
    ("table", 0, "is 23e283a3 1"),
    ("table", 1, "is 23efb88fe283a3 1"),
    ("borders", 0, "is - 0"),
    ("borders", 1, "is f09f918d 1"),
    ("borders", 2, "is f09f918defb88f 1"),
    ("borders", 3, "is f09f918defb88fefb88f 0"),
    ("borders", 5, "is f09f91a8e2808df09f91a9e2808df09f91a7 1"),
    ("borders", 11, "is f09f918df09f918d 0"),
    ("borders", 13, "is " + "61" * 32 + " 0"),
    ("find", 1, "find f09f918df09f918d [f09f918d;f09f918d]"),
    ("find", 3, "find f09f918defb88f [f09f918defb88f]"),
    ("modifiers", 1, "fp f09f918df09f8fbb 2"),
    ("modifiers", 2, "fp f09f918df09f8fbf 6"),
    ("modifiers", 6, "rfp f09f918df09f8fbbf09f8fbb f09f918d"),
    ("modifiers", 8, "rem f09f918defb88f 0 " + THUMBS + VS16),
    ("modifiers", 9, "rem f09f918defb88f 1 " + THUMBS),
    ("modifiers", 15, "rs f09f918defb88f " + THUMBS),
    ("modifiers", 16, "rs f09f918df09f8fbb " + THUMBS + FITZ_12),
    ("modifiers", 18, "rs 61 61"),
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

    cases = build_cases()
    text = script_text([f"# {name}" for name, _ in cases] + [line for _, body in cases for line in body])
    digest = hashlib.md5(text.encode()).hexdigest()
    print(f"corpus md5={digest} lines={len(text.splitlines())}")

    scripts_dir = WORK / "scripts"
    scripts_dir.mkdir(parents=True, exist_ok=True)

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    started = time.time()
    total_lines = 0
    cpp_lines: dict[str, list[str]] = {}

    for name, body in cases:
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
    print(f"PASS cases={len(cases)} lines={total_lines} elapsed={elapsed:.2f}s")
    return 0


if __name__ == "__main__":
    sys.exit(main())