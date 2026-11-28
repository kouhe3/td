"""Compare tdutils StringBuilder with TDLib, byte for byte.

    uv run python tools/string_builder_diff.py
    uv run python tools/string_builder_diff.py --selftest

Criterion: one deterministic ASCII script, run through the C++ oracle
(tools/fixtures/string_builder_probe.cpp) and the Rust probe, stdout compared
byte for byte. The script never names a real address or a locale, so `%p` is fed
constant integers and `FixedDouble` is fed raw bit patterns.

What the corpus pins down:

* the buffer model -- `StringBuilder()` starts with 100 usable bytes out of 130
  allocated; `StringBuilder(MutableSlice(n))` ignores `n <= 30` (allocating 130
  again) and otherwise exposes `n - 30`, never growing; growth doubles
  `usable + 1` with a 100 floor and a `need = current + size` lower bound;
* the reserved-tails rules -- `operator<<(Slice)`, `append_char(count)` and
  `FixedDouble` truncate at `end + 30 - 1` when growth fails (setting the error
  flag only if they actually cut), but only while `current <= end`; once
  `current > end` they append nothing. `operator<<(int)` and friends reserve a
  single byte and then write up to 20 digits, so on a non-growable builder they
  pass `end` with no error at all -- that is what the 30 reserved bytes are for;
* `pop_back()` CHECKs `current > begin`, so the abort cases run one process each
  with SEM_NOGPFAULTERRORBOX, and a control case that must return is included;
* formatting: `print_int`'s INT_MIN path (print `x + 1`, bump the last digit),
  `print_uint`'s below-100 fast path, `std::pair`/`std::vector` separators,
  fixed `std::ostream` doubles (`inf`/`-inf`, sign-less `nan`, negative
  precision == the omitted printf precision == 6), and `%p` as 16 lowercase hex
  digits without `0x`.

Coverage boundary: growth arithmetic itself is only observable through the
absence of errors (a growable builder always succeeds, so sizes never depend on
it); the `FixedDouble` truncation branch is unreachable because its
`reserve(308 + precision + 4)` always leaves room for the longest expansion;
`as_cslice`'s abort (writing past `end + 30`) cannot be reached because every
path stops at `end + 29`. Locale-dependent output beyond the classic locale,
`wchar_t *`, real heap addresses, and the `char (&)[N]` overload outside the
three fixed literals are out of scope by design.

Invalidation: any change to StringBuilder.h/.cpp in the judged surface must
re-derive the corpus; the instrument refuses a doctored oracle via the FLOOR
check and the independent Python recomputation in `--selftest`.
"""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import io
import math
import os
import re
import struct
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/string_builder_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/string_builder_probe.exe"
WORK = REPO / "work" / "string_builder"

BYTES_40 = bytes([0x61 + (i % 26) for i in range(40)])
BYTES_150 = bytes([0x41 + (i % 26) for i in range(150)])
BYTES_2000 = bytes([0x30 + (i % 10) for i in range(2000)])

NAN_NEGATIVE = struct.unpack("<d", struct.pack("<Q", 0xFFF8000000000000))[0]

# (precision, values) for the fixed-double section; every value's digits come from
# Python's own formatter in the independent check, never from the oracle.
PRECISION_CASES = [
    (0, [0.0, -0.0, 0.5, 1.5, 2.5, 3.5, -2.5, 0.125, 1 / 3]),
    (1, [0.0, 1.5, 2.45, 1 / 3, 1e-7]),
    (2, [0.0, -1.005, 1 / 3, 2 / 3]),
    (6, [1 / 3, 2 / 3, 0.1, 1e15, 1e16, 1e-7, 123456789.123456789]),
    (17, [0.1, 1 / 3, 1e300]),
    (40, [0.1, 1 / 3]),
    (1000, [1e300]),
    (-1, [1.5, -2.0]),
    (-5, [2.0]),
]

DOUBLE_VALUES = [
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.5,
    1.5,
    2.5,
    3.5,
    0.1,
    1 / 3,
    2 / 3,
    1e-7,
    1e15,
    1e16,
    1e21,
    1e22,
    0.0000005,
    1.0000005,
    4.35,
    0.045,
    123456789.123456789,
    math.inf,
    -math.inf,
    math.nan,
    NAN_NEGATIVE,
    5e-324,
    1e-320,
    2.2250738585072014e-308,
    1.7976931348623157e308,
    1e300,
    1e-300,
]


def bits_of(value: float) -> str:
    """The IEEE-754 pattern as the 16 hex digits the probes parse (big-endian)."""
    return struct.unpack("<Q", struct.pack("<d", value))[0].to_bytes(8, "big").hex()


def fixed_model(d: float, precision: int) -> bytes:
    """The documented classic-locale fixed formatting, recomputed in Python."""
    if math.isinf(d):
        return b"-inf" if d < 0 else b"inf"
    if math.isnan(d):
        return b"nan"
    digits = 6 if precision < 0 else precision
    return format(d, f".{digits}f").encode()


class Corpus:
    def __init__(self) -> None:
        self.entries: list[tuple[str, bytes | None, bool]] = []

    def op(self, line: str, expect: bytes | None = None) -> None:
        self.entries.append((line, expect, True))

    def note(self, text: str) -> None:
        self.entries.append(("# " + text, None, False))

    def blank(self) -> None:
        self.entries.append(("", None, False))

    def text(self) -> str:
        return "\n".join(entry[0] for entry in self.entries) + "\n"

    def ops(self) -> list[tuple[str, bytes | None]]:
        return [(entry[0], entry[1]) for entry in self.entries if entry[2]]


def build_corpus() -> Corpus:
    corpus = Corpus()
    corpus.note("default builder: 130 allocated, 100 usable; fill it exactly, then grow")
    corpus.op("new")
    corpus.op("dump")
    corpus.op("ac 100 97")
    corpus.op("dump")
    corpus.op("ac 1 98")
    corpus.op("dump")
    corpus.op("s " + BYTES_40.hex())
    corpus.op("dump")
    corpus.blank()

    corpus.note("cap <= 30 ignores the slice and allocates 130 without growth")
    for cap in (0, 1, 5, 30):
        corpus.op(f"new {cap}")
        corpus.op("ac 100 65")
        corpus.op("dump")
        corpus.op("ac 1 66")
        corpus.op("dump")
        corpus.op("ac 30 67")
        corpus.op("dump")
        corpus.op("s 61")
        corpus.op("dump")
        corpus.blank()

    corpus.note("empty ops at an exactly full non-growable builder are no-ops without error")
    corpus.op("new 0")
    corpus.op("ac 100 65")
    corpus.op("ac 0 66")
    corpus.op("s -")
    corpus.op("ac 0 67")
    corpus.op("dump")
    corpus.blank()

    corpus.note("cap > 30: usable = cap - 30, never grows, truncates at end + 29")
    corpus.op("new 31")
    corpus.op("s " + BYTES_40.hex())
    corpus.op("dump")
    corpus.op("s 61")
    corpus.op("dump")
    corpus.op("pb 62")
    corpus.op("dump")
    corpus.op("ac 1 63")
    corpus.op("dump")
    corpus.op("new 32")
    corpus.op("i64 -9223372036854775808", b"-9223372036854775808")
    corpus.op("dump")
    corpus.op("s 61")
    corpus.op("dump")
    corpus.op("pb 61")
    corpus.op("dump")
    corpus.op("new 40")
    corpus.op("s " + BYTES_40.hex())
    corpus.op("dump")
    corpus.op("new 31")
    corpus.op("ac 30 65")
    corpus.op("dump")
    corpus.op("ac 1 66")
    corpus.op("dump")
    corpus.op("new 31")
    corpus.op("ac 31 65")
    corpus.op("dump")
    corpus.op("ac 5 66")
    corpus.op("dump")
    corpus.op("new 31")
    corpus.op("ac 5 67")
    corpus.op("dump")
    corpus.op("s 6262")
    corpus.op("dump")
    corpus.blank()

    corpus.note("cap 100: usable 70")
    corpus.op("new 100")
    corpus.op("ac 70 65")
    corpus.op("ac 1 66")
    corpus.op("dump")
    corpus.op("ac 30 67")
    corpus.op("dump")
    corpus.op("new 100")
    corpus.op("s " + BYTES_40.hex())
    corpus.op("s " + bytes([0x62] * 40).hex())
    corpus.op("dump")
    corpus.blank()

    corpus.note("clear resets the position and the error flag, not the capacity")
    corpus.op("new")
    corpus.op("ac 10 65")
    corpus.op("s 6262")
    corpus.op("clr")
    corpus.op("dump")
    corpus.op("ac 5 66")
    corpus.op("dump")
    corpus.op("new 31")
    corpus.op("s " + BYTES_40.hex())
    corpus.op("ac 5 65")
    corpus.op("clr")
    corpus.op("dump")
    corpus.op("s 61")
    corpus.op("dump")
    corpus.blank()

    corpus.note("Slice keeps NULs, const char * stops at one, literals contribute N - 1 bytes")
    corpus.op("new")
    corpus.op("s -")
    corpus.op("s 61")
    corpus.op("s 610062")
    corpus.op("dump")
    corpus.op("cstr 610062")
    corpus.op("dump")
    corpus.op("cstr -")
    corpus.op("cstr 00")
    corpus.op("lit 0")
    corpus.op("lit 1")
    corpus.op("lit 2")
    corpus.op("dump")
    corpus.blank()

    corpus.note("char, signed char and unsigned char take different print_int paths")
    corpus.op("new")
    corpus.op("ch 0")
    corpus.op("ch 65")
    corpus.op("ch 127")
    corpus.op("ch 128", bytes([128]))
    corpus.op("ch 255", bytes([255]))
    corpus.op("sch 0", b"0")
    corpus.op("sch 65", b"65")
    corpus.op("sch 127", b"127")
    corpus.op("sch 128", b"-128")
    corpus.op("sch 255", b"-1")
    corpus.op("uch 0", b"0")
    corpus.op("uch 128", b"128")
    corpus.op("uch 255", b"255")
    corpus.op("b 0", b"false")
    corpus.op("b 1", b"true")
    corpus.op("b 7", b"true")
    corpus.op("dump")
    corpus.blank()

    corpus.note("integers: digit counts, the below-100 fast path, both minima and both maxima")
    corpus.op("new")
    for value in (0, 7, 9, 10, 99, 100, 101, 999, 2147483647):
        corpus.op(f"i32 {value}", str(value).encode())
    for value in (-1, -9, -10, -99, -100, -2147483647, -2147483648):
        corpus.op(f"i32 {value}", str(value).encode())
    for value in (0, 9, 10, 99, 100, 4000000000, 4294967295):
        corpus.op(f"u32 {value}", str(value).encode())
    for value in (0, 1, -1, 2147483647, -2147483648):
        corpus.op(f"il {value}", str(value).encode())
    for value in (0, 4294967295):
        corpus.op(f"ul {value}", str(value).encode())
    for value in (0, 1, -1, 1000000000000, 9223372036854775807, -9223372036854775807, -9223372036854775808):
        corpus.op(f"i64 {value}", str(value).encode())
    for value in (0, 1, 10000000000000000000, 18446744073709551615):
        corpus.op(f"u64 {value}", str(value).encode())
    corpus.op("dump")
    corpus.blank()

    corpus.note("integers at the boundary: one reserved byte is enough for 20 digits")
    corpus.op("new 32")
    corpus.op("i64 -9223372036854775808", b"-9223372036854775808")
    corpus.op("dump")
    corpus.op("i32 5")
    corpus.op("dump")
    corpus.op("new 33")
    corpus.op("i32 10", b"10")
    corpus.op("i32 10", b"10")
    corpus.op("dump")
    corpus.op("i32 1")
    corpus.op("dump")
    corpus.op("new 31")
    corpus.op("i32 -5", b"-5")
    corpus.op("dump")
    corpus.op("il 7")
    corpus.op("dump")
    corpus.op("new 0")
    corpus.op("ac 100 97")
    corpus.op("i32 7")
    corpus.op("dump")
    corpus.op("u64 18446744073709551615")
    corpus.op("dump")
    corpus.op("new")
    corpus.op("ac 100 97")
    corpus.op("i32 1234567890", b"1234567890")
    corpus.op("dump")
    corpus.blank()

    corpus.note("fixed doubles from raw bit patterns: inf, nan, -0.0, subnormal, DBL_MAX")
    corpus.op("new")
    for value in DOUBLE_VALUES:
        corpus.op(f"d {bits_of(value)}", fixed_model(value, 6))
    corpus.op("dump")
    for precision, values in PRECISION_CASES:
        for value in values:
            corpus.op(f"dp {precision} {bits_of(value)}", fixed_model(value, precision))
    corpus.op("dump")
    corpus.blank()

    corpus.note("cap 5000 with a 1000-digit fixed double, then a truncating slice")
    corpus.op("new 5000")
    corpus.op("s " + BYTES_2000.hex())
    corpus.op("dp 1000 " + bits_of(1e300), fixed_model(1e300, 1000))
    corpus.op("s " + BYTES_2000.hex())
    corpus.op("dump")
    corpus.op("s 61")
    corpus.op("dump")
    corpus.blank()

    corpus.note("constant pointers only: %p is 16 lowercase hex digits, no 0x")
    corpus.op("new")
    for text in ("0", "1", "10", "f", "1234", "deadbeefcafebabe", "ffffffffffffffff"):
        corpus.op(f"ptr {text}", f"{int(text, 16):016x}".encode())
    corpus.op("dump")
    corpus.op("new 31")
    corpus.op("ptr 1234", b"0000000000001234")
    corpus.op("dump")
    corpus.op("ptr 0")
    corpus.op("dump")
    corpus.blank()

    corpus.note("nested formatting: std::pair and std::vector separators")
    pairs = [
        ("i32", "-5", "s", "6162", str(-5).encode() + b";ab"),
        ("ch", "65", "u32", "7", b"A;7"),
        ("s", "-", "i32", "0", b";0"),
        ("u32", "4000000000", "s", "610062", b"4000000000;a\x00b"),
    ]
    corpus.op("new")
    for left_tag, left_arg, right_tag, right_arg, inner in pairs:
        corpus.op(f"pair {left_tag} {left_arg} {right_tag} {right_arg}", b"[" + inner + b"]")
    corpus.op("dump")
    vectors = [
        ("i32", [], b""),
        ("i32", ["1"], b"1"),
        ("i32", ["1", "22", "-333", "4"], b"1, 22, -333, 4"),
        ("u32", ["0", "4294967295"], b"0, 4294967295"),
        ("ch", ["65", "66", "0", "67"], b"A, B, \x00, C"),
        ("s", ["6162", "-", "63"], b"ab, , c"),
        ("b", ["1", "0", "1"], b"true, false, true"),
    ]
    for ty, args, inner in vectors:
        suffix = (" " + " ".join(args)) if args else ""
        corpus.op(f"vec {ty}{suffix}", b"{" + inner + b"}")
    corpus.op("dump")
    corpus.op("new 33")
    corpus.op("vec i32 1 2 3")
    corpus.op("dump")
    corpus.blank()

    corpus.note("append_char, push_back and pop_back")
    corpus.op("new")
    corpus.op("ac 0 65")
    corpus.op("ac 1 66")
    corpus.op("ac 5 67")
    corpus.op("ac 29 68")
    corpus.op("ac 30 69")
    corpus.op("dump")
    corpus.op("pb 70")
    corpus.op("pb 71")
    corpus.op("dump")
    corpus.op("pop")
    corpus.op("dump")
    corpus.op("pop")
    corpus.op("pop")
    corpus.op("dump")
    corpus.op("new")
    corpus.op("ac 100 97")
    corpus.op("pb 98")
    corpus.op("dump")
    corpus.op("new 31")
    corpus.op("pb 65")
    corpus.op("pb 66")
    corpus.op("dump")
    corpus.op("pop")
    corpus.op("dump")
    corpus.op("new")
    corpus.op("s 616263")
    corpus.op("pop")
    corpus.op("dump")
    corpus.op("pop")
    corpus.op("pop")
    corpus.op("dump")
    corpus.blank()

    corpus.note("embedded NUL bytes stay inside size()")
    corpus.op("new")
    corpus.op("s 006162")
    corpus.op("dump")
    corpus.op("ch 0")
    corpus.op("dump")
    corpus.op("ac 3 0")
    corpus.op("dump")
    corpus.op("cstr 006162")
    corpus.op("dump")
    return corpus


ABORT_CASES = [
    # (name, script, must_abort)
    ("empty_default", "new\npop\n", True),
    ("empty_slice", "new 5\npop\n", True),
    ("after_clear", "new\nac 5 65\nclr\npop\n", True),
    ("cleared_error", "new 31\ns " + BYTES_40.hex() + "\nclr\npop\n", True),
    ("one_byte", "new\ns 61\npop\ndump\n", False),
    ("after_truncate", "new 31\ns " + BYTES_40.hex() + "\npop\ndump\n", False),
    ("after_append_char", "new 31\nac 30 65\npop\ndump\n", False),
]


def run_script(binary: str, script_path: Path, env: dict[str, str]) -> tuple[int, str]:
    proc = subprocess.run([binary, str(script_path)], capture_output=True, env=env)
    return proc.returncode, proc.stdout.decode("utf-8", "replace").replace("\r\n", "\n")


def suppress_crash_dialogs() -> None:
    """Without SEM_NOGPFAULTERRORBOX every abort() waits ~4 s for WerFault.exe."""
    if sys.platform != "win32":
        return
    import ctypes

    ctypes.windll.kernel32.SetErrorMode(0x0001 | 0x0002 | 0x8000)


def compare(cpp_rc: int, cpp_out: str, rs_rc: int, rs_out: str) -> int:
    if cpp_rc == rs_rc and cpp_out == rs_out:
        return 0
    for index, (left, right) in enumerate(zip(cpp_out.splitlines(), rs_out.splitlines())):
        if left != right:
            print(f"DIFF line {index + 1}: oracle={left!r} rust={right!r}")
            return 1
    if cpp_rc != rs_rc:
        print(f"DIFF exit code: oracle={cpp_rc} rust={rs_rc}")
        return 1
    print(f"DIFF line count: oracle={len(cpp_out.splitlines())} rust={len(rs_out.splitlines())}")
    return 1


def compare_checks(args, env: dict[str, str]) -> int:
    for name, script_text, must_abort in ABORT_CASES:
        case_path = WORK / f"abort_{name}.txt"
        case_path.write_text(script_text)
        cpp_rc, cpp_out = run_script(args.cpp, case_path, env)
        rs_rc, rs_out = run_script(args.rs, case_path, env)
        cpp_aborted = cpp_rc != 0
        rs_aborted = rs_rc != 0
        if cpp_aborted != must_abort:
            print(f"FLOOR FAIL: oracle pop case {name} aborted={cpp_aborted}, expected {must_abort}")
            return 1
        if cpp_aborted != rs_aborted or cpp_out != rs_out:
            print(
                f"DIFF check {name}: oracle aborted={cpp_aborted} out={cpp_out!r} / "
                f"rust aborted={rs_aborted} out={rs_out!r}"
            )
            return 1
    return 0


OUT_PATTERN = re.compile(r" out=(\S+) ")


def check_independent(cpp_out: str, ops: list[tuple[str, bytes | None]]) -> int:
    """Recompute the expected `out=` bytes in Python and hold the oracle to them.

    A wrong implementation on both sides would still agree with itself; this check
    proves the oracle runs the documented algorithm instead.
    """
    lines = cpp_out.splitlines()
    if len(lines) != len(ops):
        print(f"SELFTEST FAIL: oracle printed {len(lines)} lines, script has {len(ops)} ops")
        return -1
    checked = 0
    for index, ((script_line, expect), line) in enumerate(zip(ops, lines)):
        if expect is None:
            continue
        match = OUT_PATTERN.search(line)
        if match is None:
            print(f"SELFTEST FAIL: no out= on line {index + 1} for {script_line!r}")
            return -1
        got = match.group(1)
        wanted = expect.hex() if expect else "-"
        if got != wanted:
            print(
                f"SELFTEST FAIL: line {index + 1} {script_line!r} oracle appends {got!r}, "
                f"Python says {wanted!r}"
            )
            return -1
        checked += 1
    return checked


def main() -> int:
    overall = time.perf_counter()
    parser = argparse.ArgumentParser()
    parser.add_argument("--cpp", default=DEFAULT_CPP)
    parser.add_argument("--rs", default=str(DEFAULT_RS))
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()

    for path in (args.cpp, args.rs):
        if not Path(path).exists():
            print(f"MISSING {path}")
            return 2

    corpus = build_corpus()
    text = corpus.text()
    ops = corpus.ops()
    digest = hashlib.md5(text.encode()).hexdigest()
    print(f"corpus md5={digest} lines={len(text.splitlines())}")

    WORK.mkdir(parents=True, exist_ok=True)
    script_path = WORK / "string_builder_script.txt"
    script_path.write_text(text)

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")
    suppress_crash_dialogs()

    started = time.perf_counter()
    cpp_rc, cpp_out = run_script(args.cpp, script_path, env)
    rs_rc, rs_out = run_script(args.rs, script_path, env)
    elapsed = time.perf_counter() - started
    status = compare(cpp_rc, cpp_out, rs_rc, rs_out)
    if status != 0:
        return status

    if run_script(args.cpp, script_path, env) != (cpp_rc, cpp_out):
        print("FLOOR FAIL: the oracle is not deterministic")
        return 1

    if compare_checks(args, env) != 0:
        return 1

    if args.selftest:
        checked = check_independent(cpp_out, ops)
        if checked < 0:
            return 1
        if checked < 100:
            print(f"SELFTEST FAIL: only {checked} lines are independently recomputed")
            return 1
        # One flipped byte must be caught by the same comparator.
        lines = cpp_out.splitlines()
        index = next(index for index, line in enumerate(lines) if " size=" in line)
        lines[index] = re.sub(r" size=(\d+)", lambda m: f" size={int(m.group(1)) + 1}", lines[index], count=1)
        captured = io.StringIO()
        with contextlib.redirect_stdout(captured):
            flipped = compare(0, "\n".join(lines) + "\n", 0, cpp_out)
        if flipped != 1:
            print(f"SELFTEST FAIL: the comparator missed a changed size\n{captured.getvalue()}")
            return 1
        print(
            f"SELFTEST OK: {checked} lines recomputed independently "
            f"(integer widths incl. INT32_MIN/INT64_MIN/UINT64_MAX, fixed doubles); flipped size caught"
        )

    total = time.perf_counter() - overall
    print(
        f"PASS lines={len(cpp_out.splitlines())} checks={len(ABORT_CASES)} "
        f"probes={elapsed * 1000:.0f}ms total={total * 1000:.0f}ms"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())