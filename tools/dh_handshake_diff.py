"""Compare DhHandshake::check_config() with TDLib.

    uv run python tools/dh_handshake_diff.py
    uv run python tools/dh_handshake_diff.py --selftest

The gate has four ordered decisions: bit length, the quadratic-residue table on
`p mod 4g`, `p` primality, then `(p - 1) / 2` primality. The corpus feeds each
stage directly, including a 2048-bit prime whose half is composite (searched
here, with a fixed seed, so the corpus is stable).
"""

from __future__ import annotations

import argparse
import hashlib
import os
import random
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/dh_handshake_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/dh_handshake_probe.exe"

# RFC 3526 group 14: a 2048-bit safe prime. p mod 8 = 7, mod 3 = 2, mod 5 = 4,
# mod 24 = 23, mod 7 = 5, so every legal g passes the residue table.
GROUP14 = int(
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74"
    "020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F1437"
    "4FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED"
    "EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3DC2007CB8A163BF05"
    "98DA48361C55D39A69163FA8FD24CF5F83655D23DCA3AD961C62F356208552BB"
    "9ED529077096966D670C354E4ABC9804F1746C08CA18217C32905E462E36CE3B"
    "E39E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2BCBF695581718"
    "3995497CEA956AE515D2261898FA051015728E5A8AACAA68FFFFFFFFFFFFFFFF",
    16,
)


# A 2048-bit prime whose (p - 1) / 2 is composite (p = 6r + 1 with r odd), so
# the corpus reaches the last branch of check_config. Searched once with a fixed
# seed; hard-coded so the run does not spend seconds finding it again.
NON_SAFE_PRIME = int(
    "c51a316a8e0ed94ccc36b7352c16e7785b596615a9bc460c55bb4dc0"
    "683e52f7067dd2b780426031c96101a0371e314c52a9cbcceffb985e"
    "789da4f82227e50d21506d089765c217f6de10aa794300f183b2fa34"
    "7d5578b1b409282fc1910501883e387e98bf8db75bfea8fee8b5b6d2"
    "077cb01a82c6e98584af019f6b57f4fdedf3c101f4e945a567ae6111"
    "3ad12b6eb4e50ba541c782206f4108206b7bada441fc79a9da7cfbd1"
    "9c3a1acd39b62ce67b834b1f4e804b235c871495ee59978868c7c006"
    "23b8ed1823675979dccb19331edf3292c6cb2f77df110c8785ee078f"
    "01a605a2655679651f4de020894bf42018f57f6e91d0b60377dcbd37"
    "828d0c47",
    16,
)


def cases() -> list[tuple[int, int]]:
    out: list[tuple[int, int]] = []
    # every legal g against a real safe prime, plus the residue-table rejects
    for g in (2, 3, 4, 5, 6, 7, 0, 1, 8, -1, 2 ** 31 - 1):
        out.append((g, GROUP14))
    # bit-length boundaries
    out.append((2, GROUP14 >> 1))
    out.append((2, GROUP14 | (1 << 2048)))
    out.append((4, 1))
    out.append((4, 0))
    # composites that still pass the residue table, so the primality test decides
    out.append((2, GROUP14 + 8))
    out.append((4, GROUP14 + 2))
    out.append((4, 1 << 2047))
    # a prime whose half is composite
    out.append((4, NON_SAFE_PRIME))
    out.append((2, NON_SAFE_PRIME))
    # random moduli of every nearby width
    rng = random.Random(20261127)
    for _ in range(12):
        bits = rng.choice([2047, 2048, 2049])
        value = rng.getrandbits(bits) | (1 << (bits - 1)) | 1
        out.append((rng.choice([2, 3, 4, 5, 6, 7, 0]), value))
    return out


def encode(prime: int) -> str:
    if prime == 0:
        return "00"
    return prime.to_bytes((prime.bit_length() + 7) // 8, "big").hex()


def run(binary: str, g_int: int, prime: int, env: dict[str, str]) -> tuple[int, str]:
    proc = subprocess.run([binary, str(g_int), encode(prime)], capture_output=True, env=env)
    return proc.returncode, proc.stdout.decode("utf-8", "replace").replace("\r\n", "\n")


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

    table = cases()
    digest = hashlib.md5(
        "\x1f".join(f"{g}:{encode(p)}" for g, p in table).encode()
    ).hexdigest()
    print(f"corpus md5={digest} cases={len(table)}")

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    for g_int, prime in table:
        cpp_rc, cpp_out = run(args.cpp, g_int, prime, env)
        rs_rc, rs_out = run(args.rs, g_int, prime, env)
        if cpp_rc != rs_rc or cpp_out != rs_out:
            print(f"DIFF g={g_int} bits={prime.bit_length()}: oracle={cpp_out.strip()!r} rust={rs_out.strip()!r}")
            return 1

    floor = table[0]
    if run(args.cpp, *floor, env)[1] != run(args.cpp, *floor, env)[1]:
        print("FLOOR FAIL: the oracle is not deterministic")
        return 1

    if args.selftest:
        # Two corpus changes the comparator has to notice: a different g on the
        # same prime, and a composite instead of the safe prime.
        same_prime = run(args.cpp, 2, GROUP14, env)[1]
        if same_prime == run(args.cpp, 0, GROUP14, env)[1]:
            print("SELFTEST FAIL: a different g produced the same output")
            return 1
        if same_prime == run(args.cpp, 2, GROUP14 + 8, env)[1]:
            print("SELFTEST FAIL: a composite produced the same output as the safe prime")
            return 1
        print("SELFTEST OK: comparator sees a g change and a primality change")

    print(f"PASS cases={len(table)} nonsafe_prime_bits={NON_SAFE_PRIME.bit_length()}")
    return 0


if __name__ == "__main__":
    sys.exit(main())