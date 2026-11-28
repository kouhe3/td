"""Compare td/utils/crypto.h with TDLib: PBKDF2-SHA256, HMAC-SHA256, CRC-32
and AesCtrState.

    uv run python tools/crypto_diff.py
    uv run python tools/crypto_diff.py --selftest

What is judged: one script drives the C++ oracle `td::pbkdf2_sha256`,
`td::hmac_sha256`, `td::crc32` and `td::AesCtrState` (init + encrypt, and the
same state fed through several encrypt() calls). Each side writes its output to
a file and the two files are compared byte for byte; the judged bytes are the
hex payloads, so both files are first rewritten with LF line endings (the C++
side prints through stdio in text mode and would otherwise emit CRLF).

Coverage: PBKDF2 over every password/salt length in 0/1/15/16/31/32/64/100
times iterations 1/2/3, plus 60002 iterations on four tiny inputs (the slow
case is deliberately limited to those four, ~0.1 s of HMAC-SHA256); HMAC keys
of 0/1/15/16/31/32/63/64/65/100/128 bytes, i.e. both sides of the 64-byte
block size where HMAC hashes instead of pads the key (plus RFC 4231's 131-byte
key), crossed with messages of 0/1/15/16/17/31/32/33/64/1000 bytes; CRC-32
including the 0xCBF43926 check
value for "123456789"; AES-256-CTR over two keys, four initial counter blocks
(all-zero, 0x01-prefixed, an incrementing one, a random-looking one) and data
of 0/1/15/16/17/31/32/33/64/1000/4097 bytes. 4097 bytes is 257 blocks, so the
low counter byte wraps and the carry into the next byte is exercised; the split
cases resume the state at 1/15/16/17-byte boundaries and after 4096 bytes, and
in three parts.

Failure conditions: any byte difference, a differing exit status, a
non-deterministic oracle (FLOOR rerun), or a corpus entry that disagrees with
the independent Python recomputation in `--selftest`. The last one is what
keeps this honest: PBKDF2, HMAC and CRC-32 are recomputed with hashlib/hmac/zlib,
and the published vectors are NIST SP 800-38A F.5.5 (CTR-AES256), RFC 7914 §11
(PBKDF2, c = 1) and RFC 4231 test cases 1 and 6 (HMAC, the latter with a
131-byte key), so the oracle is checked against an outside authority and not
only against hashlib/OpenSSL or against Rust.

Inputs with no bytes to compare are CHECKed away by the td wrappers:
`pbkdf2_sha256` with 0 iterations (crypto.cpp:945) or a destination that is not
32 bytes (crypto.cpp:944), and `AesCtrState::init` with a key that is not 32
bytes or an iv that is not 16 bytes (crypto.cpp:683-684). Both sides must refuse
them; the criterion there is a nonzero exit status, reported as a `REJECT` line.
"""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import hmac as hmac_mod
import io
import os
import subprocess
import sys
import zlib
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/crypto_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/crypto_probe.exe"
WORK = REPO / "work" / "crypto"

# NIST SP 800-38A F.5.5 CTR-AES256.Encrypt: the published ciphertext is the
# outside authority for the AES-CTR side, including the split/resume cases.
NIST_KEY = bytes.fromhex("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4")
NIST_IV = bytes.fromhex("f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff")
NIST_PLAINTEXT = bytes.fromhex(
    "6bc1bee22e409f96e93d7e117393172a"
    "ae2d8a571e03ac9c9eb76fac45af8e51"
    "30c81c46a35ce411e5fbc1191a0a52ef"
    "f69f2445df4f9b17ad2b417be66c3710"
)
NIST_CIPHERTEXT = bytes.fromhex(
    "601ec313775789a5b7a7f504bbf3d228"
    "f443e3ca4d62b59aca84e990cacaf5c5"
    "2b0930daa23de94ce87017ba2d84988d"
    "dfc9c58db67aada613c2dd08457941a6"
)

PBKDF2_LENGTHS = [0, 1, 15, 16, 31, 32, 64, 100]
HMAC_KEY_LENGTHS = [0, 1, 15, 16, 31, 32, 63, 64, 65, 100, 128]
MESSAGE_LENGTHS = [0, 1, 15, 16, 17, 31, 32, 33, 64, 1000]
CTR_DATA_LENGTHS = [0, 1, 15, 16, 17, 31, 32, 33, 64, 1000, 4097]


def pattern(length: int, seed: int) -> bytes:
    """Deterministic random-looking bytes; the corpus text is frozen by its md5."""
    state = (0x9E3779B97F4A7C15 ^ (seed * 0x100000001B3)) & 0xFFFFFFFFFFFFFFFF
    out = bytearray()
    for _ in range(length):
        state = (state * 6364136223846793005 + 1442695040888963407) & 0xFFFFFFFFFFFFFFFF
        out.append((state >> 33) & 0xFF)
    return bytes(out)


def hex_token(data: bytes) -> str:
    return "-" if not data else data.hex()


def build_corpus() -> list[dict]:
    cases: list[dict] = []

    def pbkdf2(password: bytes, salt: bytes, iterations: int) -> None:
        line = f"pbkdf2 {hex_token(password)} {hex_token(salt)} {iterations} 32"
        cases.append(
            {"op": "pbkdf2", "line": line, "password": password, "salt": salt, "iterations": iterations}
        )

    for password_length in PBKDF2_LENGTHS:
        for salt_length in PBKDF2_LENGTHS:
            password = pattern(password_length, 21)
            salt = pattern(salt_length, 22)
            for iterations in (1, 2, 3):
                pbkdf2(password, salt, iterations)

    # 60002 iterations only on tiny inputs; the other 192 cases stay at 1/2/3.
    for password, salt in ((b"", b""), (pattern(1, 21), pattern(1, 22)), (pattern(15, 21), pattern(16, 22)),
                           (pattern(100, 21), b"")):
        pbkdf2(password, salt, 60002)

    def hmac(key: bytes, message: bytes) -> None:
        cases.append({"op": "hmac", "line": f"hmac {hex_token(key)} {hex_token(message)}", "key": key,
                      "message": message})

    for key_length in HMAC_KEY_LENGTHS:
        for message_length in MESSAGE_LENGTHS:
            hmac(pattern(key_length, 31), pattern(message_length, 32))

    def crc32(data: bytes) -> None:
        cases.append({"op": "crc32", "line": f"crc32 {hex_token(data)}", "data": data})

    crc32(b"123456789")
    for length in [0, 1, 15, 16, 17, 31, 32, 33, 64, 1000, 4097]:
        crc32(pattern(length, 41))
    crc32(b"\x00" * 16)
    crc32(b"\xff" * 16)
    crc32(bytes(range(256)))

    ctr_keys = [pattern(32, 51), NIST_KEY]
    ctr_ivs = [b"\x00" * 16, b"\x01" + b"\x00" * 15, bytes(range(16)), pattern(16, 52)]

    def ctr(key: bytes, iv: bytes, data: bytes) -> None:
        cases.append({"op": "ctr", "line": f"ctr {hex_token(key)} {hex_token(iv)} {hex_token(data)}",
                      "key": key, "iv": iv, "data": data})

    for key in ctr_keys:
        for iv in ctr_ivs:
            for length in CTR_DATA_LENGTHS:
                ctr(key, iv, pattern(length, 53))

    # The published NIST plaintext under its own key/iv, single-shot.
    ctr(NIST_KEY, NIST_IV, NIST_PLAINTEXT)

    def ctrsplit(key: bytes, iv: bytes, parts: list[bytes]) -> None:
        tokens = " ".join(hex_token(part) for part in parts)
        cases.append({"op": "ctrsplit", "line": f"ctrsplit {hex_token(key)} {hex_token(iv)} {tokens}",
                      "key": key, "iv": iv, "data": b"".join(parts), "parts": len(parts)})

    def split(data: bytes, shape: list[int]) -> list[bytes]:
        assert sum(shape) == len(data), (shape, len(data))
        parts = []
        offset = 0
        for length in shape:
            parts.append(data[offset:offset + length])
            offset += length
        return parts

    ctrsplit(NIST_KEY, NIST_IV, [NIST_PLAINTEXT[:16], NIST_PLAINTEXT[16:]])
    ctrsplit(NIST_KEY, NIST_IV, [NIST_PLAINTEXT[:1], NIST_PLAINTEXT[1:16], NIST_PLAINTEXT[16:]])
    ctrsplit(NIST_KEY, NIST_IV, [b"", NIST_PLAINTEXT])

    # 4097 bytes, split so that the resume point lands at 1/15/16/17 and at the
    # 4096-byte (256-block) counter wrap; the same 4097 bytes are also one call.
    split_shapes_4097 = [[1, 4096], [15, 4082], [16, 4081], [17, 4080], [4096, 1], [1, 16, 4080], [1, 15, 4081]]
    data_4097 = pattern(4097, 53)
    for key in ctr_keys:
        for iv in (ctr_ivs[0], ctr_ivs[3]):
            for shape in split_shapes_4097:
                ctrsplit(key, iv, split(data_4097, shape))

    data_33 = pattern(33, 53)
    for shape in ([33], [1, 32], [15, 18], [16, 17], [17, 16], [1, 15, 17], [1, 16, 16]):
        ctrsplit(ctr_keys[0], ctr_ivs[0], split(data_33, shape))
    data_16 = pattern(16, 53)
    for shape in ([16], [1, 15], [8, 8], [8, 0, 8]):
        ctrsplit(ctr_keys[0], ctr_ivs[2], split(data_16, shape))
    ctrsplit(ctr_keys[0], ctr_ivs[1], [b""])
    ctrsplit(ctr_keys[1], ctr_ivs[2], [b"", b""])

    # Published vectors, used by --selftest as the outside authority.
    pbkdf2(b"passwd", b"salt", 1)  # RFC 7914 §11, c = 1, first 32 of 64 bytes
    hmac(bytes([0x0B]) * 20, b"Hi There")  # RFC 4231 test case 1
    # RFC 4231 test case 6: a 131-byte key, i.e. the hashed-not-padded path.
    hmac(bytes([0xAA]) * 131, b"Test Using Larger Than Block-Size Key - Hash Key First")

    return cases


def script(cases: list[dict]) -> str:
    lines = ["# pbkdf2 / hmac / crc32 / ctr / ctrsplit corpus; `-` is the empty string"]
    lines += [case["line"] for case in cases]
    return "\n".join(lines) + "\n"


def run(binary: str, script_path: Path, env: dict[str, str]) -> tuple[int, bytes]:
    proc = subprocess.run([binary, str(script_path)], capture_output=True, env=env)
    return proc.returncode, proc.stdout.replace(b"\r\n", b"\n")


def first_diff(left: bytes, right: bytes) -> str:
    limit = min(len(left), len(right))
    offset = limit
    for index in range(limit):
        if left[index] != right[index]:
            offset = index
            break
    start = max(0, offset - 24)
    window_left = left[start:offset + 24]
    window_right = right[start:offset + 24]
    line_number = left.count(b"\n", 0, offset) + 1
    if offset == limit and len(left) != len(right):
        return (f"DIFF length offset {offset} (line {line_number}): "
                f"oracle={len(left)} bytes rust={len(right)} bytes")
    return f"DIFF offset {offset} (line {line_number}): oracle={window_left!r} rust={window_right!r}"


def compare(cpp_rc: int, cpp: bytes, rs_rc: int, rs: bytes) -> int:
    if cpp_rc == rs_rc and cpp == rs:
        return 0
    if cpp != rs:
        print(first_diff(cpp, rs))
        return 1
    print(f"DIFF exit code: oracle={cpp_rc} rust={rs_rc}")
    return 1


def value_of(line: str, tag: str) -> str:
    return line.split(tag + "=")[1]


def check_rejects(cpp: str, rs: str, env: dict[str, str]) -> int:
    checks = [
        ("pbkdf2 - - 0 32", "pbkdf2 iteration_count 0"),
        (f"ctr {pattern(16, 1).hex()} {'00' * 16} 00", "AesCtrState 16-byte key"),
        ("pbkdf2 - - 1 16", "pbkdf2 dest 16 bytes"),
    ]
    WORK.mkdir(parents=True, exist_ok=True)
    # A CHECK abort costs ~3 s of Windows crash-reporting per process, so all of
    # them run concurrently instead of one after another.
    jobs = []
    for index, (line, label) in enumerate(checks):
        path = WORK / f"reject{index}.txt"
        path.write_text(line + "\n")
        jobs.append((label, subprocess.Popen([cpp, str(path)], stdout=subprocess.PIPE,
                                             stderr=subprocess.PIPE, env=env),
                     subprocess.Popen([rs, str(path)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)))
    for label, cpp_proc, rs_proc in jobs:
        cpp_out, _ = cpp_proc.communicate()
        rs_out, _ = rs_proc.communicate()
        cpp_rc = cpp_proc.returncode
        rs_rc = rs_proc.returncode
        if cpp_rc == 0 or rs_rc == 0:
            print(f"REJECT FAIL {label}: oracle_rc={cpp_rc} rust_rc={rs_rc} (one side produced bytes)")
            return 1
        if cpp_out or rs_out:
            print(f"REJECT FAIL {label}: unexpected stdout oracle={cpp_out!r} rust={rs_out!r}")
            return 1
        print(f"REJECT {label}: oracle_rc={cpp_rc} rust_rc={rs_rc}")
    return 0


def selftest(cases: list[dict], oracle_lines: list[str]) -> int:
    mismatches = 0
    for case, line in zip(cases, oracle_lines):
        if case["op"] == "pbkdf2":
            expected = hashlib.pbkdf2_hmac(
                "sha256", case["password"], case["salt"], case["iterations"], 32
            ).hex()
            got = value_of(line, "out")
        elif case["op"] == "hmac":
            expected = hmac_mod.new(case["key"], case["message"], hashlib.sha256).hexdigest()
            got = value_of(line, "out")
        elif case["op"] == "crc32":
            expected = f"{zlib.crc32(case['data']) & 0xFFFFFFFF:08x}"
            got = value_of(line, "v")
        else:
            continue
        if got != expected:
            mismatches += 1
            if mismatches == 1:
                print(f"SELFTEST FAIL: {case['op']} {case['line'][:60]}… oracle={got} python={expected}")
    if mismatches:
        print(f"SELFTEST FAIL: {mismatches} oracle values disagree with hashlib/hmac/zlib")
        return 1

    # hashlib.pbkdf2_hmac is OpenSSL's PKCS5 as well, so pin the oracle to
    # published vectors too: RFC 7914 §11 (PBKDF2, c = 1, 32 of 64 bytes) and
    # RFC 4231 test cases 1 and 6 (the latter with a 131-byte key, i.e. the
    # hashed-not-padded path).
    published = [
        ("pbkdf2", b"passwd", b"salt", 1,
         "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc"),
        ("hmac", bytes([0x0B]) * 20, b"Hi There", None,
         "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"),
        ("hmac", bytes([0xAA]) * 131, b"Test Using Larger Than Block-Size Key - Hash Key First", None,
         "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"),
    ]
    for op, key, text_or_message, iterations, expected in published:
        if op == "pbkdf2":
            index = next(i for i, case in enumerate(cases) if case["op"] == "pbkdf2"
                         and case["password"] == key and case["salt"] == text_or_message
                         and case["iterations"] == iterations)
        else:
            index = next(i for i, case in enumerate(cases) if case["op"] == "hmac"
                         and case["key"] == key and case["message"] == text_or_message)
        got = value_of(oracle_lines[index], "out")
        if got != expected:
            print(f"SELFTEST FAIL: published {op} vector mismatch: oracle={got} published={expected}")
            return 1

    nist_hex = NIST_CIPHERTEXT.hex()
    nist_cases = [index for index, case in enumerate(cases) if case["op"] in ("ctr", "ctrsplit")
                  and case["key"] == NIST_KEY and case["iv"] == NIST_IV and case["data"] == NIST_PLAINTEXT]
    if len(nist_cases) != 4:
        print(f"SELFTEST FAIL: expected 4 NIST cases, found {len(nist_cases)}")
        return 1
    for index in nist_cases:
        got = value_of(oracle_lines[index], "out")
        if got != nist_hex:
            print(f"SELFTEST FAIL: NIST SP 800-38A F.5.5 mismatch on {cases[index]['op']}: {got}")
            return 1

    # Every split must reproduce the single-shot ciphertext of the same data.
    single_shot = {(case["key"], case["iv"], case["data"]): index for index, case in enumerate(cases)
                   if case["op"] == "ctr"}
    resumed = 0
    for index, case in enumerate(cases):
        if case["op"] != "ctrsplit":
            continue
        match = single_shot.get((case["key"], case["iv"], case["data"]))
        if match is None:
            continue
        resumed += 1
        if value_of(oracle_lines[index], "out") != value_of(oracle_lines[match], "out"):
            print(f"SELFTEST FAIL: ctrsplit and ctr disagree on {case['line'][:60]}…")
            return 1
    if resumed < 10:
        print(f"SELFTEST FAIL: only {resumed} split cases could be cross-checked")
        return 1

    flipped = list(oracle_lines)
    target = next(index for index, case in enumerate(cases) if case["op"] == "ctr" and case["data"])
    marker = "out="
    position = flipped[target].index(marker) + len(marker)
    flipped[target] = flipped[target][:position] + ("0" if flipped[target][position] != "0" else "1") \
        + flipped[target][position + 1:]
    captured = io.StringIO()
    with contextlib.redirect_stdout(captured):
        status = compare(0, "\n".join(oracle_lines).encode(), 0, "\n".join(flipped).encode())
    if status != 1 or "DIFF offset" not in captured.getvalue():
        print(f"SELFTEST FAIL: the comparator missed a flipped hex digit\n{captured.getvalue()}")
        return 1

    print(f"SELFTEST OK: hashlib/hmac/zlib recompute {len(cases)} values, "
          f"published vectors RFC 7914/RFC 4231 x{len(published)} + NIST F.5.5 x{len(nist_cases)}, "
          f"{resumed} splits resume, flipped bit caught")
    return 0


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

    cases = build_corpus()
    text = script(cases)
    digest = hashlib.md5(text.encode()).hexdigest()
    print(f"corpus md5={digest} lines={len(text.splitlines())}")

    WORK.mkdir(parents=True, exist_ok=True)
    script_path = WORK / "crypto_script.txt"
    script_path.write_text(text)

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    cpp_rc, cpp_out = run(args.cpp, script_path, env)
    rs_rc, rs_out = run(args.rs, script_path, env)

    if cpp_rc != 0 or rs_rc != 0:
        print(f"DIFF exit code: oracle={cpp_rc} rust={rs_rc}")
        return 1
    if len(cpp_out.splitlines()) != len(cases):
        print(f"BAD OUTPUT: {len(cpp_out.splitlines())} lines for {len(cases)} cases")
        return 1

    (WORK / "oracle.out").write_bytes(cpp_out)
    (WORK / "rust.out").write_bytes(rs_out)
    status = compare(cpp_rc, (WORK / "oracle.out").read_bytes(), rs_rc, (WORK / "rust.out").read_bytes())
    if status != 0:
        return status

    if run(args.cpp, script_path, env) != (cpp_rc, cpp_out):
        print("FLOOR FAIL: the oracle is not deterministic")
        return 1

    if check_rejects(args.cpp, args.rs, env) != 0:
        return 1

    if args.selftest:
        status = selftest(cases, cpp_out.decode().splitlines())
        if status != 0:
            return status

    print(f"PASS lines={len(cpp_out.splitlines())}")
    return 0


if __name__ == "__main__":
    sys.exit(main())