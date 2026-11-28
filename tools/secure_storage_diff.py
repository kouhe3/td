"""Judge `td/telegram/SecureStorage.cpp`: C++ oracle vs Rust.

    uv run python tools/secure_storage_diff.py
    uv run python tools/secure_storage_diff.py --selftest

The oracle is `tools/fixtures/secure_storage_probe.cpp` over `libtdcore.a` plus
the `tdutils` crypto (no `Td`, no actor, no clock).

Criteria:

  (a) narration line by line for everything deterministic: sha256 over `Slice`
      and over a `DataView` with its 128 KiB chunk log, both `AesCbcState`
      derivations (observed by encrypting a zero block), `Secret` creation and
      checksums, `Secret::encrypt`/`EncryptedSecret::decrypt`, the prefix
      structure, and both stream state machines;
  (b) FLOOR: a second oracle run prints the same lines;
  (c) **cross-read**: one side's `encrypt_value` ciphertext is decrypted by the
      other (both directions), with a structural check on the ciphertext length;
  (d) a one-bit flip in the last ciphertext block reaches `Hash mismatch` on
      both sides with the same message;
  (e) `--selftest`: hand-computed cells (known `sha256` vectors, the prefix
      formula, the `to_skip_` reset, the checksum arithmetic).

Not covered (return conditions in F244): the content of the random prefix,
`Secret::create_new`'s randomness, and `encrypt_file`/`decrypt_file` (paths and
OS errors).
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
DEFAULT_CPP = "C:/tmp/td-probe/secure_storage_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/secure_storage_probe.exe"
WORK = REPO / "work" / "secure_storage"

# Fixed inputs the corpus is built from (deterministic ciphertexts included, so
# the stream cases need no second phase).
PLAIN_48 = (bytes([32]) + bytes(range(32)) + bytes(15)).hex()
CIPHER_48 = (
    "c84d284df0435e27c6d88a535f412e4b98e6ee851b6f836849cdbb23461dbe17"
    "0f58913b2378368d46ec096bfb2d9fbb"
)
CIPHER_5 = (
    "18edd3c5255226400a69f3e9c9d173b84c0cd1677d0d6ab7b3562f33b9211f4f"
    "fa96b83c43b7cc1b764b619189551505"
)
BIG = "61" * 300000
ZERO_HASH = "00" * 32

CASES = [
    (
        "hashes",
        [
            "vhash 616263",
            "vhash -",
            "dhash 616263",
            "dhash -",
            f"dhash {BIG}",
            "vcreate 00",
            f"vcreate {ZERO_HASH}",
        ],
    ),
    (
        "derivations",
        [
            "aes 00112233",
            "aes -",
            "pbkdf2 6b6579 73616c74",
        ],
    ),
    (
        "secrets",
        [
            "skey 00",
            f"skey {ZERO_HASH}",
            f"skey {'00' * 31}ef",
            f"skey {'00' * 31}ee",
            "enc 6b6579 73616c74 0",
            "enc 6b6579 73616c74 1",
            "dec 3760b12385ab237867bb9afc0fdbd0260a0ff543f288a8d0d5d8c12f56210573 6b6579 73616c74 0",
            "dec 1e89c2aadd5c4effa902c102580ad0e7a4f00166fc919ec2cc18930f8f3739e9 6b6579 73616c74 1",
            "dec 3760b12385ab237867bb9afc0fdbd0260a0ff543f288a8d0d5d8c12f56210573 6b6579 73616c74 1",
            "dec 3760b12385ab237867bb9afc0fdbd0260a0ff543f288a8d0d5d8c12f56210573 6b657a 73616c74 0",
            "dec 00 6b6579 73616c74 0",
        ],
    ),
    (
        "streams",
        [
            "prefix 0",
            "prefix 1",
            "prefix 16",
            "prefix 17",
            "prefix 4096",
            f"estate 00112233 {PLAIN_48}",
            "epread 16 16",
            "epread 0 16",
            "epread 16 0",
            "epread 16 15",
            "epread 32 16",
            f"estate 00112233 {PLAIN_48}",
            "epread 0 15",
            "epread 0 16",
            "dstate 00112233",
            f"dappend {CIPHER_48}",
            "dfinish",
            "dstate 00112233",
            f"dappend {CIPHER_48[:32]}",
            f"dappend {CIPHER_48[32:]}",
            "dfinish",
            "dstate 00112233",
            "dappend -",
            "dfinish",
            "dstate 00112233",
            "dappend 00",
            "dfinish",
            "dstate 00112233",
            "dfinish",
            "dstate 00112233",
            f"dappend {CIPHER_5}",
            "dfinish",
        ],
    ),
    (
        "values",
        [
            f"decval {ZERO_HASH} 00",
            f"decval {ZERO_HASH} -",
        ],
    ),
]

# Hand-computed cells: (case, 0-based line index, expected text). The sha256
# vectors are the published ones for "" and "abc", the prefix lengths follow
# ((32 + 15 + n) & -16) - n, and `dfinish` is sha256 over the whole plaintext.
SELFTEST_CELLS = [
    ("hashes", 0, "vhash=ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
    ("hashes", 1, "vhash=e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
    ("hashes", 3, "dhash=e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 chunks="),
    (
        "hashes",
        4,
        "dhash=12e1b9b179b29a4f7e5889b185d7ac71bff0ad1f49a7b391d0911b737a0f5381 "
        "chunks=0:131072,131072:131072,262144:37856",
    ),
    ("derivations", 0, "aes=294ad629fc5f288b0826ebb3bd4f2a1248d752db692227d2ef49b8830b899fbd"),
    (
        "derivations",
        2,
        "pbkdf2=1e89c2aadd5c4effa902c102580ad0e7d245adedab36050291e60bc689b06afa",
    ),
    (
        "secrets",
        2,
        "skey hash=-6916148830597643433 hex=" + "00" * 31 + "ef",
    ),
    ("secrets", 8, "dec err=Wrong checksum 23"),
    ("streams", 0, "prefix len=32 first=32"),
    ("streams", 4, "prefix len=32 first=32"),
    ("streams", 5, "epread err=Arbitrary offset is not supported"),
    ("streams", 12, "dappend ok len=16 hex=1f000000000000000000000000000000"),
    (
        "streams",
        13,
        "dfinish ok=27b75f0bcf3ab1f0270d18dc2f09e95576199d1d5a1292fa1f69f3cf9294e167",
    ),
    ("streams", 16, "dfinish err=Too small random prefix"),
    ("values", 1, "decval err=No data was given"),
]


def script_text(lines) -> str:
    return "\n".join(lines) + "\n"


def disable_error_dialogs() -> None:
    """Keep a crashing child from parking a WerFault dialog."""
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


def prefix_length(data_size: int) -> int:
    return ((32 + 15 + data_size) & -16) - data_size


def parse_encval(lines) -> tuple[str, str]:
    for line in lines:
        if line.startswith("encval hash="):
            _, rest = line.split(" hash=", 1)
            digest, data = rest.split(" data=", 1)
            return digest, data
    raise AssertionError(f"no encval line in {lines!r}")


def cross_read(cpp: Path, rs: Path, env: dict, scripts_dir: Path, payload: bytes) -> bool:
    """One side encrypts, both sides decrypt; then the tampered variant."""
    payload_hex = payload.hex()
    for producer, name in ((cpp, "cpp"), (rs, "rust")):
        script = scripts_dir / f"cross_{name}.txt"
        script.write_text(script_text([f"encval {payload_hex}"]))
        rc, lines, err = run(producer, script, env)
        if rc != 0:
            print(f"FAIL cross_{name}: {producer.name} exited {rc}: {err!r}")
            return False
        digest, data = parse_encval(lines)

        expected = prefix_length(len(payload)) + len(payload)
        if len(data) != expected * 2:
            print(f"DIFF cross_{name} structure: ciphertext {len(data) // 2} bytes, expected {expected}")
            return False
        if len(bytes.fromhex(digest)) != 32:
            print(f"DIFF cross_{name} structure: hash length {len(digest) // 2}")
            return False

        tampered = bytearray(bytes.fromhex(data))
        tampered[-1] ^= 1
        for label, script_lines in (
            ("payload", [f"decval {digest} {data}"]),
            ("tamper", [f"decval {digest} {tampered.hex()}"]),
        ):
            script = scripts_dir / f"cross_{name}_{label}.txt"
            script.write_text(script_text(script_lines))
            cpp_rc, cpp_out, cpp_err = run(cpp, script, env)
            rs_rc, rs_out, rs_err = run(rs, script, env)
            if cpp_rc != rs_rc or cpp_rc != 0:
                print(f"DIFF cross_{name}_{label} exit: oracle={cpp_rc} rust={rs_rc}")
                return False
            if not compare_lines(f"cross_{name}_{label}", cpp_out, rs_out):
                return False
        print(f"  cross-read {name}: payload ok, tampered block reaches the same message")
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

    if not cross_read(Path(args.cpp), Path(args.rs), env, scripts_dir, b"hello, secure storage"):
        return 1
    if args.selftest and not selftest(cpp_lines):
        return 1

    elapsed = time.time() - started
    print(f"PASS cases={len(CASES)} lines={total_lines} elapsed={elapsed:.2f}s")
    return 0


if __name__ == "__main__":
    sys.exit(main())