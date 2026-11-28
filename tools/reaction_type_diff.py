"""Judge `td::ReactionType` (`td/telegram/ReactionType.{h,cpp}`): C++ vs Rust.

    uv run python tools/reaction_type_diff.py
    uv run python tools/reaction_type_diff.py --selftest

The oracle is `tools/fixtures/reaction_type_probe.cpp` over `libtdcore.a` (no
`Td`, no actor, no clock): the class is one string plus two decoders and three
encoders.

Criteria:

  (a) narration line by line: the four encodings, both decoders (including the
      `#`/`$` forgery guard and the td_api `check_utf8` guard), the three exits,
      `get_hash`, `get_reaction_types_hash`, the non-lexicographic `<` and
      `operator<<`;
  (b) one **abort pair set**: `#zz` fails the base64 CHECK and `#abc` fails the
      8-byte CHECK, so both must exit non-zero, while `ok_custom` must exit 0;
  (c) FLOOR: a second oracle run prints the same lines;
  (d) `--selftest`: hand-computed cells -- `hash("")` and `hash("ok")` are the
      first eight md5 bytes big-endian, a custom hash is the document id, and
      paid sorts before everything.

Not covered (return conditions in F288/Spec): `get_hash`/`get_reaction_types_hash`
on an actual emoji, because `remove_emoji_selectors` needs `is_emoji` with the
packed 2370-entry table, which has no lane of its own yet.
"""

from __future__ import annotations

import argparse
import ctypes
import hashlib
import os
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/reaction_type_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/reaction_type_probe.exe"
WORK = REPO / "work" / "reaction_type"

CASES = [
    (
        "empty",
        [
            "print -",
            "hash -",
            "input -",
            "obj -",
            "upd -",
            "cmp - -",
            "vec 0 -",
            "vechash -",
        ],
    ),
    (
        "paid",
        [
            "print $",
            "input $",
            "obj $",
            "upd $",
            "cmp $ $",
            "cmp $ -",
            "vec 1 -",
            "vec 0 $",
        ],
    ),
    (
        "emoji",
        [
            "print e:f09f918d",
            "input e:f09f918d",
            "obj e:f09f918d",
            "upd e:f09f918d",
            "print e:6f6b",
            "hash e:6f6b",
            "cmp e:f09f918d e:f09f918c",
        ],
    ),
    (
        "custom",
        [
            "print c:1",
            "print c:123456789012345",
            "print c:-1",
            "hash c:1",
            "hash c:123456789012345",
            "input c:1",
            "obj c:1",
            "upd c:1",
            "cmp c:1 c:2",
        ],
    ),
    (
        "decode_telegram",
        [
            "print tl:empty",
            "print tl:paid",
            "print tl:emoji:f09f918d",
            "print tl:emoji:23616263",
            "print tl:emoji:24",
            "print tl:custom:1",
            "print tl:custom:-5",
        ],
    ),
    (
        "decode_td",
        [
            "print td:null",
            "print td:paid",
            "print td:emoji:f09f918d",
            "print td:emoji:ff",
            "print td:emoji:c3",
            "print td:emoji:23616263",
            "print td:emoji:24",
            "print td:custom:1",
        ],
    ),
    (
        "vectors",
        [
            "vec 1 -",
            "vec 0 c:1",
            "vec 1 c:1 e:f09f918d",
            "vec 1 $",
            "vechash c:1",
            "vechash c:1 c:2 c:3",
            "vechash c:-1",
        ],
    ),
    (
        "order",
        [
            "cmp $ -",
            "cmp - $",
            "cmp - c:1",
            "cmp c:1 -",
            "cmp c:1 e:f09f918d",
            "cmp e:f09f918d c:1",
            "cmp - -",
            "cmp $ $",
        ],
    ),
    (
        "rejects",
        [
            "print",
            "print bogus",
            "hash",
            "cmp -",
            "vec 2 -",
            "vec 0 bogus",
            "vechash",
            "nope x",
        ],
    ),
]

# (case, expected exit is non-zero). `bad_b64` and `short_b64` hit the two CHECKs
# inside `get_custom_emoji_id`; `ok_custom` is the control that must stay at 0.
CHECK_CASES = [
    ("bad_b64", True),
    ("short_b64", True),
    ("ok_custom", False),
]


def script_text(lines) -> str:
    return "\n".join(lines) + "\n"


def disable_error_dialogs() -> None:
    if sys.platform != "win32":
        return
    ctypes.windll.kernel32.SetErrorMode(0x0001 | 0x0002 | 0x8000)


def run(exe: Path, script: Path, env: dict):
    proc = subprocess.run(
        [str(exe), str(script)],
        capture_output=True,
        encoding="utf-8",
        errors="replace",
        env=env,
    )
    return proc.returncode, proc.stdout.splitlines(), proc.stderr.strip()


def run_check(exe: Path, case: str, env: dict) -> int:
    proc = subprocess.run(
        [str(exe), "--check", case],
        capture_output=True,
        encoding="utf-8",
        errors="replace",
        env=env,
    )
    return proc.returncode


def compare_lines(name: str, cpp, rs) -> bool:
    for index, (a, b) in enumerate(zip(cpp, rs)):
        if a != b:
            print(f"DIFF {name} line {index + 1}: oracle={a!r} rust={b!r}")
            return False
    if len(cpp) != len(rs):
        print(f"DIFF {name}: oracle lines={len(cpp)} rust lines={len(rs)}")
        return False
    return True


# (case, index, expected line).
SELFTEST_CELLS = [
    ("empty", 0, "print - str=- empty=1 custom=0 paid=0 empty reaction"),
    ("empty", 1, "hash - 15284527576400310788"),
    ("empty", 3, "obj - null"),
    ("paid", 0, "print $ str=24 empty=0 custom=0 paid=1 paid reaction"),
    ("paid", 3, "upd $ paid"),
    ("emoji", 5, "hash e:6f6b 4921250468871504777"),
    ("custom", 4, "hash c:123456789012345 123456789012345"),
    ("custom", 5, "input c:1 id=-1992950669 custom doc=1"),
    ("vectors", 0, "vec 1 - [paid,null]"),
    ("vectors", 4, "vechash c:1 1"),
    ("order", 0, "cmp $ - lt=1 eq=0 ne=1"),
    ("order", 2, "cmp - c:1 lt=1 eq=0 ne=1"),
    ("order", 4, "cmp c:1 e:f09f918d lt=1 eq=0 ne=1"),
    ("decode_telegram", 3, "print tl:emoji:23616263 str=- empty=1 custom=0 paid=0 empty reaction"),
    ("decode_telegram", 4, "print tl:emoji:24 str=- empty=1 custom=0 paid=0 empty reaction"),
    ("decode_td", 3, "print td:emoji:ff str=- empty=1 custom=0 paid=0 empty reaction"),
    ("decode_td", 6, "print td:emoji:24 str=- empty=1 custom=0 paid=0 empty reaction"),
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


def check_pairs(args, env) -> bool:
    """The two abort cases must fail and the control must pass, on both sides."""
    for case, want_abort in CHECK_CASES:
        cpp_rc = run_check(Path(args.cpp), case, env)
        rs_rc = run_check(Path(args.rs), case, env)
        if (cpp_rc != 0) != want_abort:
            print(f"DIFF check {case}: oracle exit={cpp_rc}, expected abort={want_abort}")
            return False
        if (rs_rc != 0) != want_abort:
            print(f"DIFF check {case}: rust exit={rs_rc}, expected abort={want_abort}")
            return False
    return True


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

    if not check_pairs(args, env):
        return 1

    if args.selftest and not selftest(cpp_lines):
        return 1

    elapsed = time.time() - started
    print(f"PASS cases={len(CASES)} checks={len(CHECK_CASES)} lines={total_lines} elapsed={elapsed:.2f}s")
    return 0


if __name__ == "__main__":
    sys.exit(main())