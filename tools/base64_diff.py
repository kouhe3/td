"""Compare tdutils base64 / base32 with TDLib.

    uv run python tools/base64_diff.py
    uv run python tools/base64_diff.py --selftest

The decoder strips trailing '=' first: 3+ of them is "Wrong string padding", a
length that is not a multiple of 4 (after padding) is "Wrong padding length",
and the unused bits of a short final group must be zero or it is "Wrong padding
in the string" — that is why "ab==" is rejected while "aQ==" decodes to 'i'.
The url variant uses -_ and encodes without padding. `is_base64` repeats the
length checks and also inspects the last character's low bits.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/base64_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/base64_probe.exe"
WORK = REPO / "work" / "base64"

ENCODE_INPUTS = ["", "66", "666f", "666f6f", "666f6f62", "666f6f6261", "666f6f626172",
                 "00", "fff", "e4bda0", "80ff00", "0102030405"]
DECODE_TEXTS = ["", "Zg==", "Zm8=", "Zm9v", "Zm9vYg==", "Zm9vYmE=", "Zm9vYmFy", "Zg", "Zg=",
                "====", "ab==", "aQ==", "aQ", "Zm!v", "Zm9v!", "Zm9v\n", "_w", "-w", "Z", "AAAA", "////", "++//"]
ISANY_BYTES = [0x41, 0x2D, 0x2B, 0x2F, 0x5F, 0x3D, 0x20, 0x80, 0x30]
FILTER_INPUTS = ["", "a b\nc=", "a-b_c", "Zg==", "+/"]
B32_DECODE_TEXTS = ["mzxw6", "MZXW6", "my", "mz", "mzxw6===", "", "mzxw6yq", "mzxw6ytb", "mzxw6ytboi"]


def txt(text: str) -> str:
    return text.encode().hex() if text else "-"


def script() -> str:
    lines = ["# encoders, decoders, predicates, filter, base32"]
    for value in ENCODE_INPUTS:
        lines.append(f"enc {value or '-'}")
        lines.append(f"encurl {value or '-'}")
    for value in DECODE_TEXTS:
        lines.append(f"dec {txt(value)}")
        lines.append(f"decurl {txt(value)}")
    for value in DECODE_TEXTS:
        for op in ("is64", "is64url", "is64c", "is64urlc"):
            lines.append(f"{op} {txt(value)}")
    for byte in ISANY_BYTES:
        lines.append(f"isany {byte}")
    for value in FILTER_INPUTS:
        lines.append(f"filter {txt(value)}")
    for value in ENCODE_INPUTS:
        lines.append(f"b32 {value or '-'} 0")
        lines.append(f"b32 {value or '-'} 1")
    for value in B32_DECODE_TEXTS:
        lines.append(f"b32d {txt(value)}")
    return "\n".join(lines) + "\n"


def run(binary: str, script_path: Path, env: dict[str, str]) -> tuple[int, str]:
    proc = subprocess.run([binary, str(script_path)], capture_output=True, env=env)
    return proc.returncode, proc.stdout.decode("utf-8", "replace").replace("\r\n", "\n")


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


def err(message: str) -> str:
    return message.encode().hex()


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

    text = script()
    digest = hashlib.md5(text.encode()).hexdigest()
    print(f"corpus md5={digest} lines={len(text.splitlines())}")

    WORK.mkdir(parents=True, exist_ok=True)
    script_path = WORK / "base64_script.txt"
    script_path.write_text(text)

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    cpp_rc, cpp_out = run(args.cpp, script_path, env)
    rs_rc, rs_out = run(args.rs, script_path, env)
    status = compare(cpp_rc, cpp_out, rs_rc, rs_out)
    if status != 0:
        return status

    if run(args.cpp, script_path, env) != (cpp_rc, cpp_out):
        print("FLOOR FAIL: the oracle is not deterministic")
        return 1

    if args.selftest:
        checks = [
            (f"enc in={txt('foo')} out={txt('Zm9v')}", "base64 of foo"),
            (f"enc in={txt('f')} out={txt('Zg==')}", "base64 pads a single byte with two '='"),
            (f"encurl in={txt('f')} out={txt('Zg')}", "base64url does not pad"),
            (f"dec in={txt('Zg==')} ok=1 out=66", "decode of Zg=="),
            (f"dec in={txt('aQ==')} ok=1 out=69", "aQ== decodes to 'i'"),
            (f"dec in={txt('ab==')} ok=0 err={err('Wrong padding in the string')}",
             "ab== has nonzero unused bits"),
            (f"dec in={txt('Zg')} ok=0 err={err('Wrong padding length')}", "standard base64 needs padding"),
            (f"decurl in={txt('Zg')} ok=1 out=66", "url base64 accepts the unpadded form"),
            (f"decurl in={txt('Z')} ok=0 err={err('Wrong string length')}", "a single url character is bad"),
            (f"dec in={txt('====')} ok=0 err={err('Wrong string padding')}", "four '=' chars"),
            (f"dec in={txt('Zm!v')} ok=0 err={err('Wrong character in the string')}", "invalid character"),
            (f"dec in={txt('Zm9v!')} ok=0 err={err('Wrong padding length')}",
             "a 5-character string fails the length check before the character check"),
            (f"is64 in={txt('ab==')} r=0", "is_base64 checks the last character's low bits"),
            (f"is64 in={txt('aQ==')} r=1", "is_base64 accepts aQ=="),
            (f"is64c in={txt('aQ==')} r=0", "the character check does not allow '='"),
            (f"is64c in={txt('aQ')} r=1", "the character check alone"),
            (f"filter in={txt('a b\nc=')} out={txt('abc=')}", "filter keeps base64 characters and '='"),
            ("isany c=45 r=1", "'-' belongs to the url alphabet"),
            ("isany c=61 r=0", "'=' is not an alphabet character"),
            (f"b32 in={txt('foo')} upper=0 out={txt('mzxw6')}", "base32 lowercase, no padding"),
            (f"b32 in={txt('foo')} upper=1 out={txt('MZXW6')}", "base32 uppercase"),
            (f"b32d in={txt('mzxw6')} ok=1 out={txt('foo')}", "base32 decode"),
            (f"b32d in={txt('mz')} ok=0 err={err('Nonzero padding')}", "base32 leftover bits must be zero"),
        ]
        for expected, failure in checks:
            if expected not in cpp_out:
                print(f"SELFTEST FAIL: {failure} (missing {expected!r})")
                return 1
        print(f"SELFTEST OK: {len(checks)} hand-checked cells (padding, unused bits, url, filter, base32)")

    print(f"PASS lines={len(cpp_out.splitlines())}")
    return 0


if __name__ == "__main__":
    sys.exit(main())