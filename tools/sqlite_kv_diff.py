"""Judge `td::SqliteKeyValue` (tddb/td/db/SqliteKeyValue.h): C++ oracle vs the Rust port.

    uv run python tools/sqlite_kv_diff.py
    uv run python tools/sqlite_kv_diff.py --selftest

The oracle is the repository's own SQLCipher 3.31 build, the Rust side is
`rust/src/sqlite_kv.rs` on `rusqlite` (bundled SQLite 3.53.1). Those two are 22
minor versions apart, so **file bytes are not a criterion** -- the page layout
and `sqlite_master` text differ by version, not by behaviour. What is judged:

  (a) the narration of every command, line by line, including the *order* the
      rows come back in -- `getall`/`get_by_prefix("")` scan the table in rowid
      order while `get_by_range`/`get_by_prefix(p)` walk the `k BLOB PRIMARY KEY`
      index in key order, and `REPLACE INTO` moves an overwritten row to the end
      of the rowid order;
  (b) the C++ side opens the *Rust* database and dumps what it wrote itself --
      i.e. the file format is interoperable in that direction;
  (c) the Rust side opens the *C++* database and dumps what it wrote itself;
  (d) FLOOR: a second oracle run over the same script produces the same lines;
  (e) `--check`, one process per case: a zero-length key in the table aborts
      `get_all()` (`CHECK(!key.empty())`) while `get_by_prefix("", cb)` does not.
      Only "did it abort" is compared -- TDLib leaves through a fail-fast
      exception and Rust's `panic=abort` through the CRT, so the numbers differ
      by platform, not by behaviour.

Measured facts behind the corpus (docs/facts.md F196/F197):
  * `get_by_range("", till)` ignores `till` (an empty `from` selects the
    all-rows statement); `get_by_range(from, "")` takes the `?1 <= k` rare form.
  * `get_by_prefix` strips the prefix off the returned keys; `get_by_range` does
    not.
  * `next_prefix` bumps the last non-0xFF byte; an all-0xFF prefix has no
    successor, so `prefix ff` and `eraseprefix ff` take the rare form, which is
    exact because nothing sorts above 0xFF...FF except keys starting with it.
    `eraseprefix -` (the empty prefix) therefore drops the whole table.
  * an empty `Slice` is `""`, never nullptr, so an empty key or value is a
    zero-length **blob**, never NULL; `get` cannot tell a missing key from an
    empty value (both `""`), and `crc` of either is 0.
  * `commit` without a matching `begin` is `No matching begin for commit`;
    nested begins collapse into one SQL `BEGIN` and are closed by one commit.
  * `close()` is `*this = SqliteKeyValue()` (the connection is dropped), so a
    `footprint` afterwards shows no `-journal`/`-wal`/`-shm`; `drop()` leaves the
    object empty and a later `reopen` recreates an empty table; `destroy` removes
    all four paths.

Not covered (each with a return condition in F196): SQLite file bytes, the
SQLCipher paths (`DbKey` non-empty), `SqliteStatement::explain()`, `trace()`,
`RawSqliteDb::last_error`'s delete-on-CORRUPT, and `set_all` with more than one
entry (its `FlatHashMap` iteration order would decide the rowid order).
"""

from __future__ import annotations

import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import time
import zlib
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/sqlite_kv_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/sqlite_kv_probe.exe"
WORK = REPO / "work" / "sqlite-kv"

BIG = "ab" * 65536  # 64 KiB value, only its crc and vlen are printed

CASES = [
    (
        "basic",
        [
            "set 61 31",
            "set 62 32",
            "get 61",
            "get 7a",
            "getall",
            "range 61 7b",
            "prefix 36",
            "count",
            "close",
        ],
    ),
    (
        "overwrite",
        [
            "set 61 31",
            "set 62 32",
            "set 61 39",
            "getall",
            "range 61 7b",
            "get 61",
            "close",
        ],
    ),
    (
        "empty_value",
        [
            "set 65 -",
            "get 65",
            "crc 65",
            "get 66",
            "crc 66",
            "getall",
            "set 65 76",
            "get 65",
            "erase 65",
            "get 65",
            "getall",
            "close",
        ],
    ),
    (
        "prefix_carry",
        [
            "set ff61 31",
            "set ffff 32",
            "set 7a 33",
            "prefix ff",
            "prefix ffff",
            "prefix fe",
            "range ff -",
            "eraseprefix ff",
            "getall",
            "eraseprefix -",
            "getall",
            "close",
        ],
    ),
    (
        "range_edges",
        [
            "set 61 31",
            "set 62 32",
            "set 63 33",
            "range - 62",
            "range 62 -",
            "range 62 62",
            "range 63 61",
            "range - -",
            "close",
        ],
    ),
    (
        "erase_kinds",
        [
            "set 61 31",
            "set 62 32",
            "set 63 33",
            "erase 61",
            "erase 61",
            "erasebatch 62 7a 63 62",
            "getall",
            "set 6162 31",
            "set 6163 32",
            "set 6261 33",
            "eraseprefix 61",
            "getall",
            "close",
        ],
    ),
    (
        "transactions",
        [
            "commit",
            "begin",
            "begin",
            "set 61 31",
            "commit",
            "commit",
            "commit",
            "wbegin",
            "wbegin",
            "set 62 32",
            "commit",
            "commit",
            "commit",
            "getall",
            "close",
        ],
    ),
    (
        "lifecycle",
        [
            "set 61 31",
            "close",
            "footprint",
            "reopen",
            "getall",
            "drop",
            "reopen",
            "getall",
            "close",
        ],
    ),
    (
        "destroy",
        [
            "set 61 31",
            "close",
            "footprint",
            "destroy",
            "footprint",
        ],
    ),
    (
        "bigvalue",
        [
            f"set 62 {BIG}",
            "get 62",
            "crc 62",
            "getall",
            "close",
        ],
    ),
    (
        "binary_keys",
        [
            "set 00 31",
            "set 80 32",
            "set ff 33",
            "set 0000 34",
            "set 00ff 35",
            "set 00000000000000000000000000000000 36",
            "getall",
            "range 00 ff",
            "prefix 00",
            "prefix 0000",
            "close",
        ],
    ),
]

CHECK_CASES = ["empty_getall", "empty_prefix"]

READ_SCRIPT = [
    "attach",
    "getall",
    "range - -",
    "range 61 -",
    "prefix ff",
    "prefix -",
    "crc 61",
    "count",
    "close",
]


def narration(lines, command: str):
    """Lines whose first token is exactly `command` (`get` must not catch `getall`)."""
    return [line for line in lines if line.split(" ", 1)[0] == command]


def compare_lines(name: str, cpp, rs) -> bool:
    if len(cpp) != len(rs):
        print(f"DIFF {name}: line count oracle={len(cpp)} rust={len(rs)}")
        for index in range(min(len(cpp), len(rs))):
            if cpp[index] != rs[index]:
                print(f"  first mismatch at line {index + 1}: oracle={cpp[index]!r} rust={rs[index]!r}")
                break
        return False
    for index, (left, right) in enumerate(zip(cpp, rs)):
        if left != right:
            print(f"DIFF {name} line {index + 1}: oracle={left!r} rust={right!r}")
            return False
    return True


def script_text(lines) -> str:
    return "\n".join(lines) + "\n"


def run(exe: Path, script: Path, db: Path, env: dict):
    proc = subprocess.run([str(exe), str(script), str(db)], capture_output=True, env=env)
    return (
        proc.returncode,
        proc.stdout.decode("utf-8", "replace").splitlines(),
        proc.stderr.decode("utf-8", "replace").strip(),
    )


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

    text = script_text([f"# {name}" for name, _ in CASES] + sum((body for _, body in CASES), []))
    digest = hashlib.md5(text.encode()).hexdigest()
    print(f"corpus md5={digest} lines={len(text.splitlines())}")

    for sub in ("scripts", "cpp", "rs", "read", "check"):
        shutil.rmtree(WORK / sub, ignore_errors=True)
        (WORK / sub).mkdir(parents=True, exist_ok=True)
    (WORK / "scripts" / "read.txt").write_text(script_text(READ_SCRIPT))

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    started = time.time()
    total_bytes = 0
    cpp_lines: dict[str, list[str]] = {}

    for name, body in CASES:
        script = WORK / "scripts" / f"{name}.txt"
        script.write_text(script_text(body))
        cpp_db = WORK / "cpp" / f"{name}.db"
        rs_db = WORK / "rs" / f"{name}.db"
        cpp_rc, cpp_lines[name], cpp_err = run(Path(args.cpp), script, cpp_db, env)
        rs_rc, rs_out, rs_err = run(Path(args.rs), script, rs_db, env)
        if cpp_rc != rs_rc:
            print(f"DIFF {name} exit code: oracle={cpp_rc} rust={rs_rc} stderr={cpp_err!r} {rs_err!r}")
            return 1
        if cpp_rc != 0:
            print(f"FAIL {name}: both sides exited {cpp_rc}: {cpp_err!r} {rs_err!r}")
            return 1
        if not compare_lines(f"{name} narration", cpp_lines[name], rs_out):
            return 1

        # FLOOR: the oracle must be deterministic on the same script and path
        floor_rc, floor_lines, _ = run(Path(args.cpp), script, cpp_db, env)
        if floor_rc != cpp_rc or floor_lines != cpp_lines[name]:
            print(f"FLOOR FAIL: the oracle is not deterministic on {name}")
            return 1

        for db in (cpp_db, rs_db):
            if db.exists():
                total_bytes += db.stat().st_size
            for suffix in ("-journal", "-wal", "-shm"):
                leftover = Path(str(db) + suffix)
                if leftover.exists():
                    print(f"FAIL {name}: {leftover.name} was left behind")
                    return 1

        if not cross_read(args, env, name, cpp_db, rs_db):
            return 1

    if not check_abort_pair(args, env):
        return 1

    if args.selftest and not selftest(cpp_lines):
        return 1

    elapsed = time.time() - started
    print(
        f"PASS cases={len(CASES)} checks={len(CHECK_CASES)} bytes={total_bytes} elapsed={elapsed:.2f}s"
    )
    return 0


def cross_read(args, env, name: str, cpp_db: Path, rs_db: Path) -> bool:
    """(b) and (c): four fresh copies of whichever databases survived the case."""
    if not cpp_db.exists() or not rs_db.exists():
        return True
    read_dir = WORK / "read"
    paths = {
        "cpp_own": read_dir / f"{name}.cpp.cpp.db",
        "cpp_rs": read_dir / f"{name}.rs.cpp.db",
        "rs_own": read_dir / f"{name}.rs.rs.db",
        "rs_cpp": read_dir / f"{name}.cpp.rs.db",
    }
    shutil.copyfile(cpp_db, paths["cpp_own"])
    shutil.copyfile(rs_db, paths["cpp_rs"])
    shutil.copyfile(rs_db, paths["rs_own"])
    shutil.copyfile(cpp_db, paths["rs_cpp"])
    script = WORK / "scripts" / "read.txt"

    results = {}
    for key, exe in (
        ("cpp_own", Path(args.cpp)),
        ("cpp_rs", Path(args.cpp)),
        ("rs_own", Path(args.rs)),
        ("rs_cpp", Path(args.rs)),
    ):
        rc, lines, stderr = run(exe, script, paths[key], env)
        if rc != 0:
            print(f"DIFF cross={key}/{name} exit={rc} stderr={stderr!r}")
            return False
        results[key] = lines

    for label, left, right in (
        ("cpp-on-rust", "cpp_own", "cpp_rs"),
        ("rust-on-cpp", "rs_own", "rs_cpp"),
        ("cross-narration", "cpp_own", "rs_own"),
    ):
        if not compare_lines(f"{name} {label}", results[left], results[right]):
            return False
    return True


def check_abort_pair(args, env) -> bool:
    """(e): the abort pair, one process per case; only the rc *class* is compared.

    `expect` is not decoration: it asserts that the oracle really does abort on
    `get_all()` and really does not on `get_by_prefix("", cb)`. Without it a
    silent copy-paste of the two cases would still "pass".
    """
    expect = {"empty_getall": True, "empty_prefix": False}
    ok = True
    for case in CHECK_CASES:
        cpp_rc, cpp_out, _ = run_check(Path(args.cpp), WORK / "check" / f"cpp-{case}.db", case, env)
        rs_rc, rs_out, rs_err = run_check(Path(args.rs), WORK / "check" / f"rs-{case}.db", case, env)
        cpp_abort = cpp_rc != 0
        rs_abort = rs_rc != 0
        if cpp_abort != expect[case]:
            print(
                f"FAIL check {case}: oracle rc={cpp_rc}, expected "
                f"{'an abort' if expect[case] else 'a clean exit'}"
            )
            ok = False
        if rs_abort != cpp_abort:
            print(f"DIFF check {case}: oracle rc={cpp_rc} rust rc={rs_rc} stderr={rs_err!r}")
            ok = False
        if not cpp_abort and cpp_out != rs_out:
            print(f"DIFF check {case} stdout: oracle={cpp_out!r} rust={rs_out!r}")
            ok = False
        print(f"  check {case}: abort={'yes' if cpp_abort else 'no'} cpp_rc={cpp_rc} rs_rc={rs_rc}")
    return ok


def run_check(exe: Path, db: Path, case: str, env: dict):
    proc = subprocess.run([str(exe), "--check", str(db), case], capture_output=True, env=env)
    return (
        proc.returncode,
        proc.stdout.decode("utf-8", "replace").splitlines(),
        proc.stderr.decode("utf-8", "replace").strip(),
    )


def selftest(cpp_lines: dict[str, list[str]]) -> bool:
    """Hand-computed facts, checked against the oracle before trusting the diff."""

    def want(condition: bool, message: str) -> bool:
        if not condition:
            print(f"SELFTEST FAIL: {message}")
        return condition

    ok = True

    # 1. insertion order is rowid order; REPLACE moves the row to the end of it,
    #    while the index keeps handing the same rows back sorted -- reversed.
    ok &= want(
        narration(cpp_lines["basic"], "getall") == ["getall n=2 61=31 62=32"],
        "two inserts come back in insertion order",
    )
    ok &= want(
        narration(cpp_lines["overwrite"], "getall") == ["getall n=2 62=32 61=39"],
        "an overwritten key leaves the front of the rowid order",
    )
    ok &= want(
        narration(cpp_lines["overwrite"], "range") == ["range from=61 till=7b n=2 61=39 62=32"],
        "the index hands the same two rows back in key order",
    )

    # 2. get_by_prefix strips the prefix; an all-0xFF prefix takes the rare form.
    prefixes = narration(cpp_lines["prefix_carry"], "prefix")
    ok &= want(
        prefixes[:2] == ["prefix p=ff n=2 61=31 ff=32", "prefix p=ffff n=1 -=32"],
        "the prefix is stripped, and 0xff/0xffff each match only their own keys",
    )
    ok &= want(prefixes[2] == "prefix p=fe n=0", "0xfe has no key below 0xff")

    # 3. an empty `from` ignores `till`; an empty `till` takes the `?1 <= k` form.
    ranges = narration(cpp_lines["range_edges"], "range")
    ok &= want(
        ranges[0] == "range from=- till=62 n=3 61=31 62=32 63=33",
        'get_by_range("", till) runs the all-rows statement and ignores till',
    )
    ok &= want(
        ranges[1] == "range from=62 till=- n=2 62=32 63=33",
        "an empty till selects the ?1 <= k form",
    )
    ok &= want(ranges[2] == "range from=62 till=62 n=0", "a half-open empty range stays empty")
    ok &= want(ranges[3] == "range from=63 till=61 n=0", "from > till yields nothing")

    # 4. a missing key and an empty value are the same thing to get(); both crc 0.
    #    The two lines differ only in the key they echo, never in the value part.
    gets = narration(cpp_lines["empty_value"], "get")
    ok &= want(
        gets[0] == "get key=65 value=- vlen=0" and gets[1] == "get key=66 value=- vlen=0",
        "an empty value and a missing key narrate identically",
    )
    ok &= want(
        narration(cpp_lines["empty_value"], "crc")[:2]
        == ["crc key=65 vlen=0 crc=00000000", "crc key=66 vlen=0 crc=00000000"],
        "an empty blob crc32s to 0, so crc cannot separate them either",
    )
    ok &= want(
        narration(cpp_lines["empty_value"], "getall")[0] == "getall n=1 65=-",
        "get_all() lists the empty value but not the missing key",
    )

    # 5. the empty prefix erases the whole table.
    ok &= want(
        narration(cpp_lines["prefix_carry"], "getall")[-1] == "getall n=0",
        'erase_by_prefix("") drops every row',
    )

    # 6. commit without a matching begin is an error; nested begins collapse into
    #    one SQL BEGIN, so two begins are closed by two commits and only the third
    #    commit is unmatched.
    ok &= want(
        narration(cpp_lines["transactions"], "commit")
        == [
            "commit ok=0 err=No matching begin for commit",
            "commit ok=1",
            "commit ok=1",
            "commit ok=0 err=No matching begin for commit",
            "commit ok=1",
            "commit ok=1",
            "commit ok=0 err=No matching begin for commit",
        ],
        "the begin counter nests: 0 matches nothing, two begins need two commits",
    )

    # 7. erase_by_prefix is a byte-prefix range, not a string prefix.
    ok &= want(
        narration(cpp_lines["erase_kinds"], "eraseprefix") == ["eraseprefix p=61 removed=2"],
        "0x6162/0x6163 match but 0x6261 does not",
    )
    ok &= want(
        narration(cpp_lines["erase_kinds"], "getall")[-1] == "getall n=1 6261=33",
        "only the non-matching key survives",
    )

    # 8. lifecycle: no journal after close, drop empties the object, a reopen
    #    after a drop recreates an empty table, destroy removes every path.
    ok &= want(
        "footprint f=1 j=0 w=0 s=0" in cpp_lines["lifecycle"],
        "a closed database leaves no -journal/-wal/-shm",
    )
    ok &= want("drop ok=1 empty=1" in cpp_lines["lifecycle"], "drop() closes the object")
    ok &= want(
        narration(cpp_lines["lifecycle"], "getall")[-1] == "getall n=0",
        "reopening after a drop recreates an empty table",
    )
    ok &= want(
        "footprint f=0 j=0 w=0 s=0" in cpp_lines["destroy"],
        "destroy removes all four paths",
    )

    # 9. binary keys: rowid order is insertion order, index order sorts bytes.
    ok &= want(
        narration(cpp_lines["binary_keys"], "getall")
        == ["getall n=6 00=31 80=32 ff=33 0000=34 00ff=35 00000000000000000000000000000000=36"],
        "getall keeps insertion order even for 0x00/0x80/0xff keys",
    )
    ok &= want(
        narration(cpp_lines["binary_keys"], "range")
        == [
            "range from=00 till=ff n=5 00=31 0000=34 00000000000000000000000000000000=36 "
            "00ff=35 80=32"
        ],
        "the range is [0x00, 0xff) in byte order: 0x80 in, 0xff out, shorter prefix first",
    )
    ok &= want(
        narration(cpp_lines["binary_keys"], "prefix")
        == [
            "prefix p=00 n=4 -=31 00=34 " + "0" * 30 + "=36 ff=35",
            "prefix p=0000 n=2 -=34 " + "0" * 28 + "=36",
        ],
        "stripping 0x00 leaves 15 zero bytes, stripping 0x0000 leaves 14",
    )

    # 10. a 64 KiB value: vlen is the real length and the crc covers every byte,
    #     recomputed here with zlib as an independent third opinion.
    payload = bytes([0xAB]) * 65536
    ok &= want(
        narration(cpp_lines["bigvalue"], "crc")
        == [f"crc key=62 vlen=65536 crc={zlib.crc32(payload):08x}"],
        "a 64 KiB value is summarized by zlib's crc32 over all of it",
    )
    ok &= want(
        narration(cpp_lines["bigvalue"], "get") == ["get key=62 value=" + "ab" * 32 + " vlen=65536"],
        "get() truncates the hex at 32 bytes and reports the true length",
    )
    return ok


if __name__ == "__main__":
    sys.exit(main())