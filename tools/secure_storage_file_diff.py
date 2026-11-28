"""Judge the file-level half of `td/telegram/SecureStorage.cpp`: C++ oracle vs Rust.

    uv run python tools/secure_storage_file_diff.py
    uv run python tools/secure_storage_file_diff.py --selftest

The oracle is `tools/fixtures/secure_storage_file_probe.cpp` over `libtdcore.a`
plus the `tdutils` crypto (no `Td`, no actor, no clock): `encrypt_file` and
`decrypt_file` only touch `FileFd`, `AesCbcState` and the two stream state
machines. Each probe runs with its own work directory and relative paths, so the
two implementations print identical `FileFd` error strings.

Criteria (every one of them is a `check`, and the PASS line counts them):

  (a) narration line by line for the deterministic part: for `encrypt_file` the
      source/destination sizes, the 16-byte alignment, the skipped prefix's first
      byte (the prefix length, 32..47), whether sha256(plaintext) equals the
      returned hash, and whether the ciphertext contains the payload; for
      `decrypt_file` the byte length and the md5 of the file it wrote, or the
      exact error string plus the size of the destination it left behind;
  (b) FLOOR: a second oracle run prints the same lines (the random prefix is
      published to a sidecar file, never narrated);
  (c) **cross-read**, both directions: ciphertext written by one side's
      `encrypt_file` is decrypted by the other side's `decrypt_file` with the
      producer's hash, and the narration must be the expected `len` + `md5` of
      the payload; the ciphertext size is checked against
      `prefix_length(n) + n` first;
  (d) tampering: flipping the last ciphertext byte must fail on both sides with
      the same message (`Hash mismatch`) and the same already-written
      destination size;
  (e) `--selftest`: hand-computed cells for the empty file, 1 byte, 64 zero
      bytes, 100 fixed bytes, the two 128 KiB boundaries, the crafted fixed
      vector, and every pinned error path (`Wrong hash size`, the two `FileFd`
      messages, `Part size must be divisible by 16`, `No data was given`,
      `Hash mismatch`). The md5 cells are computed here with `hashlib` and the
      `mkenc` hash from the prefix formula plus sha256, i.e. not copied from the
      probes;
  (f) no abort path: this half has no `CHECK`/`UNREACHABLE` on valid input; the
      error paths above are `Status` values.

The corpus is fixed: every payload is a literal below, nothing is generated at
run time. Not covered / not measurable (`unmeasured`):

  * the *content* of the random prefix, and therefore the ciphertext bytes and
    the value of the returned hash — only the structure is judged, and the
    `contains` field is a substring search where a false hit would need a
    2^-128 coin;
  * PBKDF2: the file path derives only from `calc_aes_cbc_state_sha512(secret ||
    hash)`, `calc_aes_cbc_state_pbkdf2` is exercised by the `secure_storage`
    lane, not here;
  * a `decrypt_file` of a short random ciphertext: whether the first decrypted
    byte is a plausible prefix length decides between `Too small random prefix`
    and `Hash mismatch`, so that path is not judged; the only success-path `dec`
    is the crafted fixed vector, and the random-ciphertext success path is the
    `encdec` round trip;
  * OS failures other than the two pinned `FileFd` messages (permission denied,
    disk full, a source that is a directory), and `get_size` failures;
  * `Secret` construction and `encrypt_value`/`decrypt_value` (the
    `secure_storage` lane).
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
DEFAULT_CPP = "C:/tmp/td-probe/secure_storage_file_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/secure_storage_file_probe.exe"
WORK = REPO / "work" / "secure_storage_file"

# Fixed payloads: (literal, nothing generated at run time).
ZERO_HASH = "00" * 32
ONE = "41"
ZEROS64 = "00" * 64
ENTROPY100 = (
    "2976b3085d82c734519e2b60a5ea1f4cf94603d88d7237e4612efbb0551acf7cc91653a8fd2267d4317e8bc0"
    "054aff2c19e6a3782d925704c18e5b10b57a2fdc69b6f3489dc2077491de6ba0e52a5f8cb906c3984d32f7a4"
    "21eebb7015da8f3c095693e8"
)
FIXED60 = (
    "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b"
    "2c2d2e2f303132333435363738393a3b"
)
CROSS200 = (
    "051626394b58687f899aaac5d7e4f4031d2e3e415360708791a2b2ddeffc0c1b352656497b68988fb9aadaf5"
    "e71404332d5e4e71639080b7a1d2c2ed1f0c3c2b65764659abb8889fe9faca25370414637d4e5ea1b38090e7"
    "f1c2d23d0f1c6c7b5546b6a99b88f8efd9ca3a15077464534dbeae9183f0e0d7c132220d7f6c5c4bc5d6e6f9"
    "8b98a8bf495a6a05172434c3ddeefe8193a0b0475162721d2f3cccdbf5e69689bba8584f796a1a3527d4c4f3"
    "ed9e8eb1a35040776112022ddfccfceba5b686996b78485f"
)
BIG128K = "5a" * 131072
BIG128K1 = "5a" * 131072 + "5b"
BAD15 = "303132333435363738396162636465"  # 15 bytes: not a multiple of 16


def prefix_length(data_size: int) -> int:
    """`gen_random_prefix`: ((32 + 15 + n) & -16) - n."""
    return ((32 + 15 + data_size) & -16) - data_size


def crafted_prefix(data_size: int) -> bytes:
    """The deterministic prefix `mkenc` builds: length in byte 0, then i * 37 + 11."""
    size = prefix_length(data_size)
    body = bytearray(size)
    for i in range(1, size):
        body[i] = (i * 37 + 11) & 0xFF
    body[0] = size
    return bytes(body)


def md5_hex(data: bytes) -> str:
    return hashlib.md5(data).hexdigest()


ONE_BYTES = bytes.fromhex(ONE)
ZEROS64_BYTES = bytes.fromhex(ZEROS64)
ENTROPY_BYTES = bytes.fromhex(ENTROPY100)
FIXED60_BYTES = bytes.fromhex(FIXED60)
CROSS_BYTES = bytes.fromhex(CROSS200)
BIG128K_BYTES = bytes.fromhex(BIG128K)
BIG128K1_BYTES = bytes.fromhex(BIG128K1)

# The hash `mkenc fixed60` must publish: sha256 over its own crafted prefix + payload.
FIXED60_HASH = hashlib.sha256(crafted_prefix(len(FIXED60_BYTES)) + FIXED60_BYTES).hexdigest()

CASES = [
    ("empty", ["put empty.in -", "enc empty", "encdec empty"]),
    ("one", [f"put one.in {ONE}", "enc one", "encdec one"]),
    ("zeros64", [f"put zeros64.in {ZEROS64}", "enc zeros64", "encdec zeros64"]),
    ("entropy", [f"put entropy.in {ENTROPY100}", "enc entropy", "encdec entropy"]),
    ("big128k", [f"put big128k.in {BIG128K}", "enc big128k", "encdec big128k"]),
    ("big128k1", [f"put big128k1.in {BIG128K1}", "enc big128k1", "encdec big128k1"]),
    (
        "crafted",
        [
            f"mkenc fixed60 {FIXED60}",
            f"dec fixed60 {FIXED60_HASH}",
            "tamper fixed60",
            f"dec fixed60 {FIXED60_HASH}",
        ],
    ),
    (
        "errors",
        [
            f"put bad15.enc {BAD15}",
            f"dec bad15 {ZERO_HASH}",
            "put nil.enc -",
            f"dec nil {ZERO_HASH}",
            "dec absent -",
            "dec absent 00",
            f"dec absent {ZERO_HASH}",
            "enc absent",
            "put p.in 3031",
            "enc nodst p.in nodir/x.enc",
            f"put nodst.enc {BAD15}",
            f"dec nodst {ZERO_HASH} nodst.enc nodir/x.dec",
        ],
    ),
]

# Hand-computed cells: (case, 0-based line index, expected text). The prefix
# length and the destination size follow ((32 + 15 + n) & -16) - n; the md5
# cells are `hashlib` over the literal payloads; FIXED60_HASH is sha256 over the
# crafted prefix + payload; the error strings are read off `SecureStorage.cpp`
# and `FileFd.cpp`.
SELFTEST_CELLS = [
    ("empty", 1, "enc empty src=0 dst=32 aligned=1 prefix=32 stored=1 contains=-"),
    ("empty", 2, "encdec empty same=1 len=0 md5=" + md5_hex(b"")),
    ("one", 1, "enc one src=1 dst=48 aligned=1 prefix=47 stored=1 contains=-"),
    ("one", 2, "encdec one same=1 len=1 md5=" + md5_hex(ONE_BYTES)),
    ("zeros64", 1, "enc zeros64 src=64 dst=96 aligned=1 prefix=32 stored=1 contains=0"),
    ("zeros64", 2, "encdec zeros64 same=1 len=64 md5=" + md5_hex(ZEROS64_BYTES)),
    ("entropy", 1, "enc entropy src=100 dst=144 aligned=1 prefix=44 stored=1 contains=0"),
    ("entropy", 2, "encdec entropy same=1 len=100 md5=" + md5_hex(ENTROPY_BYTES)),
    (
        "big128k",
        1,
        "enc big128k src=131072 dst=131104 aligned=1 prefix=32 stored=1 contains=0",
    ),
    ("big128k", 2, "encdec big128k same=1 len=131072 md5=" + md5_hex(BIG128K_BYTES)),
    (
        "big128k1",
        1,
        "enc big128k1 src=131073 dst=131120 aligned=1 prefix=47 stored=1 contains=0",
    ),
    ("big128k1", 2, "encdec big128k1 same=1 len=131073 md5=" + md5_hex(BIG128K1_BYTES)),
    (
        "crafted",
        0,
        f"mkenc fixed60 hash={FIXED60_HASH} size=96 prefix=36",
    ),
    (
        "crafted",
        1,
        f"dec fixed60 ok len=60 md5={md5_hex(FIXED60_BYTES)}",
    ),
    ("crafted", 2, "tamper fixed60 size=96"),
    ("crafted", 3, "dec fixed60 err=Hash mismatch leftover=60"),
    ("errors", 0, f"put bad15.enc len={len(bytes.fromhex(BAD15))}"),
    ("errors", 1, "dec bad15 err=Part size must be divisible by 16 leftover=0"),
    ("errors", 3, "dec nil err=No data was given leftover=0"),
    ("errors", 4, "dec absent err=Wrong hash size 0 leftover=-"),
    ("errors", 5, "dec absent err=Wrong hash size 1 leftover=-"),
    ("errors", 6, 'dec absent err=File "absent.enc" can\'t be opened for reading leftover=-'),
    ("errors", 7, 'enc absent err=File "absent.in" can\'t be opened for reading dst=-'),
    ("errors", 8, "put p.in len=2"),
    (
        "errors",
        9,
        'enc nodst err=File "nodir/x.enc" can\'t be opened/created for writing with truncation dst=-',
    ),
    (
        "errors",
        11,
        'dec nodst err=File "nodir/x.dec" can\'t be opened/created for writing with truncation leftover=-',
    ),
]

CHECKS = 0


def check(ok: bool, message: str) -> bool:
    global CHECKS
    CHECKS += 1
    if not ok:
        print(f"FAIL {message}")
    return ok


def script_text(lines) -> str:
    return "\n".join(lines) + "\n"


def disable_error_dialogs() -> None:
    """Keep a crashing child from parking a WerFault dialog."""
    if sys.platform != "win32":
        return
    import ctypes

    ctypes.windll.kernel32.SetErrorMode(0x0001 | 0x0002 | 0x8000)


def run(exe: Path, script: Path, workdir: Path, env: dict):
    proc = subprocess.run(
        [str(exe), str(script), str(workdir)],
        capture_output=True,
        text=True,
        errors="replace",
        env=env,
    )
    return proc.returncode, proc.stdout.splitlines(), proc.stderr.strip()


def compare_lines(name: str, cpp, rs) -> bool:
    ok = True
    if len(cpp) != len(rs):
        ok = check(False, f"{name}: oracle lines={len(cpp)} rust lines={len(rs)}")
    for index, (a, b) in enumerate(zip(cpp, rs)):
        if not check(a == b, f"{name} line {index + 1}: oracle={a!r} rust={b!r}"):
            ok = False
    return ok


def fresh_dir(path: Path) -> None:
    path.mkdir(parents=True, exist_ok=True)
    for child in path.iterdir():
        if child.is_file():
            child.unlink()
        else:
            raise AssertionError(f"unexpected directory in the work area: {child}")


def cross_read(cpp: Path, rs: Path, env: dict, dirs: dict, scripts: Path) -> bool:
    """One side encrypts, both sides decrypt; then the tampered variant."""
    payload = CROSS_BYTES
    expected_size = prefix_length(len(payload)) + len(payload)
    expected_line = f"dec xread ok len={len(payload)} md5={md5_hex(payload)}"
    tampered_line = f"dec xread err=Hash mismatch leftover={len(payload)}"
    ok = True

    for producer, name in ((cpp, "cpp"), (rs, "rust")):
        script = scripts / f"cross_produce_{name}.txt"
        script.write_text(script_text([f"put xread.in {CROSS200}", "enc xread"]))
        rc, lines, err = run(producer, script, dirs[name], env)
        ok &= check(rc == 0 and len(lines) == 2, f"cross_{name}: producer exited {rc}: {err!r} {lines!r}")
        if not ok:
            return False

        cipher = (dirs[name] / "xread.enc").read_bytes()
        digest = (dirs[name] / "xread.hash").read_bytes()
        ok &= check(len(cipher) == expected_size, f"cross_{name} structure: {len(cipher)} bytes, expected {expected_size}")
        ok &= check(len(digest) == 32, f"cross_{name} structure: hash file {len(digest)} bytes")
        if not ok:
            return False

        # Both sides decrypt the same producer-written ciphertext.
        for side in ("cpp", "rust"):
            (dirs[side] / "xread.enc").write_bytes(cipher)
        script = scripts / "cross_dec.txt"
        script.write_text(script_text([f"dec xread {digest.hex()}"]))
        cpp_rc, cpp_out, cpp_err = run(cpp, script, dirs["cpp"], env)
        rs_rc, rs_out, rs_err = run(rs, script, dirs["rust"], env)
        ok &= check(cpp_rc == 0 and rs_rc == 0, f"cross_{name} payload exit: cpp={cpp_rc} rust={rs_rc} {cpp_err!r} {rs_err!r}")
        ok &= compare_lines(f"cross_{name} payload", cpp_out, rs_out)
        ok &= check(cpp_out == [expected_line], f"cross_{name} payload: {cpp_out!r} != {expected_line!r}")

        # Flip the last ciphertext byte: both sides must fail the same way.
        tampered = bytearray(cipher)
        tampered[-1] ^= 1
        for side in ("cpp", "rust"):
            (dirs[side] / "xread.enc").write_bytes(bytes(tampered))
        cpp_rc, cpp_out, cpp_err = run(cpp, script, dirs["cpp"], env)
        rs_rc, rs_out, rs_err = run(rs, script, dirs["rust"], env)
        ok &= check(cpp_rc == 0 and rs_rc == 0, f"cross_{name} tamper exit: cpp={cpp_rc} rust={rs_rc} {cpp_err!r} {rs_err!r}")
        ok &= compare_lines(f"cross_{name} tamper", cpp_out, rs_out)
        ok &= check(cpp_out == [tampered_line], f"cross_{name} tamper: {cpp_out!r} != {tampered_line!r}")
        print(f"  cross-read {name}: payload ok, tampered last block reaches the same message")

    return ok


def selftest(cpp_lines: dict[str, list[str]]) -> bool:
    ok = True
    for name, index, expected in SELFTEST_CELLS:
        lines = cpp_lines.get(name)
        if lines is None or index >= len(lines):
            ok &= check(False, f"cell {name}[{index}]: missing line")
            continue
        ok &= check(lines[index] == expected, f"cell {name}[{index}]: oracle={lines[index]!r} expected={expected!r}")
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

    dirs = {name: WORK / name for name in ("cpp", "rust", "scripts")}
    for path in dirs.values():
        fresh_dir(path)
    scripts = dirs["scripts"]

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    started = time.time()
    total_lines = 0
    cpp_lines: dict[str, list[str]] = {}

    for name, body in CASES:
        script = scripts / f"{name}.txt"
        script.write_text(script_text(body))
        cpp_rc, cpp_out, cpp_err = run(Path(args.cpp), script, dirs["cpp"], env)
        rs_rc, rs_out, rs_err = run(Path(args.rs), script, dirs["rust"], env)
        if not check(cpp_rc == rs_rc, f"{name} exit code: oracle={cpp_rc} rust={rs_rc} {cpp_err!r} {rs_err!r}"):
            return 1
        if not check(cpp_rc == 0, f"{name}: both sides exited {cpp_rc}: {cpp_err!r} {rs_err!r}"):
            return 1
        cpp_lines[name] = cpp_out
        total_lines += len(cpp_out) + len(rs_out)
        if not compare_lines(f"{name} narration", cpp_out, rs_out):
            return 1

        floor_rc, floor_out, _ = run(Path(args.cpp), script, dirs["cpp"], env)
        if not check(floor_rc == cpp_rc and floor_out == cpp_out, f"FLOOR {name}: the oracle is not deterministic"):
            return 1

    if not cross_read(Path(args.cpp), Path(args.rs), env, dirs, scripts):
        return 1
    if args.selftest and not selftest(cpp_lines):
        return 1

    elapsed = time.time() - started
    print(f"PASS cases={len(CASES)} checks={CHECKS} lines={total_lines} elapsed={elapsed:.2f}s")
    return 0


if __name__ == "__main__":
    sys.exit(main())