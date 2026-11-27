"""Compare AuthData's salt / server-time / packet state machine with TDLib.

The probe takes a script of operations and prints one line per operation; every
`now` comes from the script, so the whole state machine is deterministic even
though TDLib normally reads Time::now(). Both probes (oracle and Rust) read the
same script, and their stdout must be byte-identical.

Usage:
    uv run python tools/auth_data_diff.py
    uv run python tools/auth_data_diff.py --selftest

Exit codes: 0 = PASS, 1 = mismatch, 2 = missing binary.
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
DEFAULT_CPP = "C:/tmp/td-probe/auth_data_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/auth_data_probe.exe"


def message_id(high: int, low: int = 1) -> int:
    return (high << 32) | low


def hand_script() -> str:
    """Every op, every boundary that is reachable through the public API."""
    lines = [
        "# salt validity boundaries: set at 1000 -> valid_until 1600, valid is > now+60",
        "session 77",
        "set 1111 1000",
        "get 1000",
        "valid 1539",
        "valid 1540",
        "has 1539",
        "has 1540",
        "need 1000",
        "servertime 1000",
        "# future salts: sorted by valid_since descending, adopted from the back while valid_since < server_time",
        "future 1000 200 1000 1200 300 900 1100",
        "get 1000",
        "need 1000",
        "has 1100",
        "get 1100",
        "need 1100",
        "get 1200",
        "need 1200",
        "# an empty future list must not clear the current salt",
        "future 1200",
        "get 1200",
        "# server time difference: first value wins, then only strictly larger than cur + 1e-4",
        "diff 5",
        "diff 4",
        "diff 5",
        "diff 5.00005",
        "diff 5.0002",
        "servertime 1000",
        "reset 0",
        "servertime 1000",
        "reset -3.5",
        "servertime 1000",
        "# outbound window: server_time-150 < id_time < server_time+30",
        f"out {message_id(849)} 1000",
        f"out {message_id(850)} 1000",
        f"out {message_id(1029)} 1000",
        f"out {message_id(1030)} 1000",
        "# inbound window: server_time-300 < id_time < server_time+30",
        f"in {message_id(699)} 1000",
        f"in {message_id(700)} 1000",
        f"in {message_id(1029)} 1000",
        f"in {message_id(1030)} 1000",
        "# packet: session mismatch, parity, first packet, duplicate",
        f"packet 78 {message_id(1000)} 1000",
        f"packet 77 {message_id(1000, 0)} 1000",
        f"packet 77 {message_id(1000)} 1000",
        f"packet 77 {message_id(1000)} 1000",
        f"packet 77 {message_id(999)} 1000",
        f"packet 77 {message_id(1001)} 1000",
        "# a time difference update is reported by the next packet only once",
        f"packet 77 {message_id(1002)} 1001",
    ]
    # Fill the 1000-entry duplicate window, then ask for an id below the oldest
    # known one: that is the "very old" path of check_message_id_duplicates.
    lines.append("# fill the duplicate window")
    for high in range(1000, 2000):
        lines.append(f"packet 77 {message_id(high)} 1000")
    lines.append(f"packet 77 {message_id(500)} 1000")
    lines.append(f"packet 77 {message_id(1500)} 1000")
    # 1001 more inserts trigger the 2N compaction; the dropped ids become "very old".
    lines.append("# force the 2N compaction")
    for high in range(2000, 3001):
        lines.append(f"packet 77 {message_id(high)} 1000")
    lines.append(f"packet 77 {message_id(1000)} 1000")
    lines.append(f"packet 77 {message_id(2999)} 1000")
    lines.append(f"packet 77 {message_id(3001)} 1000")
    return "\n".join(lines) + "\n"


def random_script(rng: random.Random, steps: int) -> str:
    """Random sequences over the same ops; only the combinations are random.

    Constraints: `set` always comes first (a fresh AuthData's salt is random in
    TDLib), `future` uses distinct valid_since values (std::sort is unstable), and
    every second/msg_id stays a finite double with a whole-second high word.
    """
    lines = [f"session {rng.randrange(1, 1 << 63)}", f"set {rng.randrange(0, 1 << 64)} {rng.randrange(-100, 2000)}"]
    now = 1000
    for _ in range(steps):
        op = rng.choices(
            ["get", "valid", "has", "need", "future", "diff", "reset", "servertime", "out", "in", "packet"],
            weights=[10, 6, 6, 6, 6, 5, 3, 3, 5, 5, 20],
        )[0]
        now += rng.choice([-37, -1, 0, 0, 1, 59, 60, 61, 299, 301])
        if op == "future":
            count = rng.randrange(0, 4)
            since_pool = rng.sample(range(-2000, 4000), count)
            triples = " ".join(
                f"{rng.randrange(0, 1 << 64)} {since} {since + rng.choice([0, 60, 120, 600])}"
                for since in since_pool
            )
            lines.append(f"future {now} {triples}".rstrip())
            continue
        if op in {"diff", "reset"}:
            lines.append(f"{op} {rng.choice([-5, 0, 0.00005, 0.0002, 1, 5.5, 1000])}")
            continue
        if op == "servertime":
            lines.append(f"servertime {now}")
            continue
        if op in {"out", "in"}:
            high = now + rng.choice([-301, -300, -299, -151, -150, -149, 29, 30, 31])
            lines.append(f"{op} {message_id(high, rng.choice([1, 3]))} {now}")
            continue
        if op == "packet":
            session = 77 if rng.random() < 0.9 else rng.randrange(1, 100)
            high = now + rng.choice([-301, -300, -299, 0, 29, 30, 31])
            lines.append(f"packet {session} {message_id(high, rng.choice([0, 1, 3]))} {now}")
            continue
        lines.append(f"{op} {now}")
    return "\n".join(lines) + "\n"


def corpus_md5(scripts: list[str]) -> str:
    digest = hashlib.md5()
    for script in scripts:
        digest.update(script.encode("utf-8"))
        digest.update(b"\x00")
    return digest.hexdigest()


def run(binary: str, script_path: Path, env: dict[str, str]) -> tuple[int, str, str]:
    proc = subprocess.run([binary, str(script_path)], capture_output=True, env=env)
    return (
        proc.returncode,
        # printf on Windows translates \n to \r\n; Rust writes bytes as-is.
        proc.stdout.decode("utf-8", "replace").replace("\r\n", "\n"),
        proc.stderr.decode("utf-8", "replace"),
    )


def first_difference(left: str, right: str) -> str:
    left_lines = left.splitlines()
    right_lines = right.splitlines()
    for index, (a, b) in enumerate(zip(left_lines, right_lines)):
        if a != b:
            return f"line {index + 1}: oracle={a!r} rust={b!r}"
    if len(left_lines) != len(right_lines):
        return f"line count: oracle={len(left_lines)} rust={len(right_lines)}"
    return "no difference"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cpp", default=DEFAULT_CPP)
    parser.add_argument("--rs", default=str(DEFAULT_RS))
    parser.add_argument("--selftest", action="store_true")
    parser.add_argument("--random-scripts", type=int, default=8)
    parser.add_argument("--steps", type=int, default=120)
    parser.add_argument("--seed", type=int, default=20261127)
    args = parser.parse_args()

    for path in (args.cpp, args.rs):
        if not Path(path).exists():
            print(f"MISSING {path}")
            return 2

    rng = random.Random(args.seed)
    scripts = [hand_script()] + [random_script(rng, args.steps) for _ in range(args.random_scripts)]
    print(f"corpus md5={corpus_md5(scripts)} scripts={len(scripts)}")

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")
    script_path = REPO / "work" / "auth_data_script.txt"
    script_path.parent.mkdir(exist_ok=True)

    total_lines = 0
    hand_output = ""
    for index, script in enumerate(scripts):
        script_path.write_bytes(script.encode("utf-8"))
        rc, oracle, err = run(args.cpp, script_path, env)
        if rc != 0:
            print(f"FAIL oracle rc={rc} script={index} {err.strip()[:200]}")
            return 1
        rc, rust, err = run(args.rs, script_path, env)
        if rc != 0:
            print(f"FAIL rust rc={rc} script={index} {err.strip()[:200]}")
            return 1
        if oracle != rust:
            print(f"DIFF script={index}: {first_difference(oracle, rust)}")
            return 1
        if index == 0:
            hand_output = oracle
        total_lines += len(oracle.splitlines())

    # A/A floor: the same script twice on the oracle, independent processes.
    rc, again, _ = run(args.cpp, script_path, env)
    if rc != 0 or again != oracle:
        print("FLOOR FAIL: the oracle is not deterministic")
        return 1

    if args.selftest:
        mutated = hand_output.replace("updated=1", "updated=0", 1)
        if mutated == hand_output:
            print("SELFTEST FAIL: tamper did not change the output")
            return 1
        if first_difference(hand_output, mutated) == "no difference":
            print("SELFTEST FAIL: comparator accepted a tampered output")
            return 1
        print("SELFTEST OK: comparator sees a one-digit change")

    print(f"PASS scripts={len(scripts)} steps={total_lines}")
    script_path.unlink(missing_ok=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())