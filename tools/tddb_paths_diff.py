"""Judge td::TdDb's path helpers: C++ oracle vs the Rust port, line for line.

    uv run python tools/tddb_paths_diff.py
    uv run python tools/tddb_paths_diff.py --selftest

Two public-ish statics of `td::TdDb` are under test, both taken from
td/telegram/TdDb.cpp:

  * `as_db_key` (:671-676) -- "" becomes the raw key "cucumber", anything else
    becomes the raw key of the same bytes. `DbKey` exposes exactly
    empty/raw_key/password and `data()` (tddb/td/db/DbKey.h), so that is all the
    narration can carry; the 32-byte AES key is derived later by
    BinlogKeyValue/AesCtrEncryption and is not a property of a DbKey.
  * `check_parameters` (:621-669) -- the "." default, the msg -> chat -> file
    implication chain, `prepare_dir` (append TD_DIR_SLASH, `mkpath(…, 0750)`,
    `realpath(…, true)`, append a separator again if the result lost it) and the
    two 400 messages. `check_parameters` is *private* in TdDb.h:192; the oracle
    widens access with `#define private public` around the TdDb.h include and
    then runs td's own compiled function.

What only the selftest can judge, and why: `get_binlog_path` and
`get_sqlite_path` (TdDb.cpp:49-57) are `static` inside TdDb.cpp's anonymous
namespace -- no probe can link to them, so no oracle output can carry them. The
four path templates (`<dir>td.binlog`, `<dir>td_test.binlog`, `<dir>db.sqlite`,
`<dir>db_test.sqlite`) are therefore asserted in `--selftest` against a
transcription of those two functions, together with the flag chain and the
message composition, which the corpus cells can then be checked against.

Measured rules the corpus pins down (all re-measurable with `--selftest`, which
contains the one-line containment cell for each):
  1. an empty `database_directory_` is replaced by "." before anything else, so
     "" and "." resolve to the cwd; an empty `files_directory_` is *copied* from
     the already normalised database directory (no second prepare_dir);
  2. the implication chain only ever sets flags: msg => chat, chat => file;
  3. the resolved directory always ends with one TD_DIR_SLASH ('\\' in this
     build) and is absolute, because Windows' `realpath` is GetFullPathNameW --
     lexical: it folds '/' to '\\', collapses "." and "..", and never touches
     the filesystem or resolves symlinks. Measured directly: `C:\\a\\` ->
     `C:\\a\\`, `C:\\a` -> `C:\\a`, `C:\\a\\\\` -> `C:\\a\\`, `C:\\a\\.` ->
     `C:\\a`, `C:\\a\\..\\a\\` -> `C:\\a\\`, `C:\\` -> `C:\\`, `.\\` ->
     `<cwd>\\`, `..\\` -> `<parent>\\`. It *preserves* the separator
     prepare_dir appended, so the two
     `if (res.back() != TD_DIR_SLASH)` appends in td fire only inside a mutant --
     they are dead code by measurement, and ported anyway;
  4. `mkpath` splits on '\\' *only*, so a path written with '/' separators gets
     exactly one CreateDirectoryW call for the whole path (minus the separator
     prepare_dir appended): `work/tddb-paths/nope/a/b` fails with
     ERROR_PATH_NOT_FOUND (3) while `work\\tddb-paths\\nope2\\a\\b` creates the
     same depth one component at a time and succeeds;
  5. `mkdir` accepts ERROR_ALREADY_EXISTS, and Windows reports that code for a
     path that is an existing regular *file*, so `<…>/marker.txt` passes as a
     "directory" and comes back with a trailing separator;
  6. a failing mkpath is reported as
     `[WindowsError : <FormatMessage> : <code> : Can't create directory "<path>"]`
     and wrapped by
     `Can't init database in the directory "<as given>": <that status>`, code
     400; once the database directory has been normalised, a failing files
     directory keeps it normalised in the printed parameters;
  7. when mkpath's last failing component differs from the first one, the two
     are chained as `<last>: <first>` (work\\tddb-paths\\bad<name\\sub), because
     mkpath keeps a `last_error` that every mkdir call overwrites;
  8. `is_test_dc` is not read by check_parameters at all -- it only selects the
     file names in the (unreachable) path templates, which is why the selftest
     asserts them while the corpus only shows the flag surviving untouched.

Coverage boundary and invalidation: only the composed behaviour above is judged.
`TdDb::open`, the SQLite/binlog files themselves (the KV lane), `change_key`,
`destroy`, and `td::realpath` as a standalone function are out of scope; so is
every path at or past MAX_PATH (GetFullPathNameW is called with a 260-unit
buffer in td, and a longer path leaves that branch unspecified), and any
non-ASCII path (the corpus is ASCII, so the UTF-16 conversion is exercised only
by the localized FormatMessage text). The Windows error strings come from
FormatMessage and are locale-dependent; both sides call the same API, so the
comparison is stable but the bytes differ between locales. `realpath(…, true)`'s
`errno == ERROR_ACCESS_DENIED` branch is a source quirk (CRT errno against a
Win32 code) and is unreachable here. Any edit to TdDb.cpp's
check_parameters/get_*_path, to mkpath/mkdir/realpath in
tdutils/td/utils/port/path.cpp, or to Status's renderings must be re-measured
before this gate is trusted.

Both sides run with cwd = the repo root and the same freshly reset scratch tree
(`work/tddb-paths`, containing one regular file `marker.txt`), because the
resolved paths are absolute and compared literally. Two things are compared:
stdout, byte for byte after CRLF folding (the C++ probe's stderr is ignored),
and the directory tree each run leaves under `work/tddb-paths`. The tree is
needed because `realpath` is lexical: a port that skipped a CreateDirectoryW
call would print exactly the same paths, and only the tree says it did not.

Exit codes: 0 = PASS, 1 = diff / self-test failure, 2 = missing binary.
"""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import io
import os
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/tddb_paths_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/tddb_paths_probe.exe"
WORK = REPO / "work" / "tddb-paths"
SCRATCH = "work/tddb-paths"

# as_db_key inputs (raw bytes; the probe reads them as hex, "-" means empty).
KEYS = [
    b"",  # the empty input: raw key "cucumber"
    b"hi",
    b"cucumber",  # the fallback spelled out is not special
    bytes(range(16)),
    "你好".encode(),  # multi-byte UTF-8, stored verbatim
    b"a\x00b",  # a raw key may contain NUL
    b"\xff\xfe",  # ... and need not be valid UTF-8
    bytes(range(100)),  # no length cap on the raw key
]

# (database_directory, files_directory, is_test_dc, use_file, use_chat, use_msg)
CHECKPARAMS = [
    # the "." default, "" and "." alike, then the same with every flag set
    ("", "", 0, 0, 0, 0),
    ("", "", 1, 1, 1, 1),
    (".", "", 0, 0, 0, 0),
    # one segment, with and without a trailing separator
    (f"{SCRATCH}/rel", "", 0, 0, 0, 0),
    (f"{SCRATCH}/rel/", "", 0, 0, 0, 0),
    # backslash separators let mkpath create the missing parents, '/' ones do not
    ("work\\tddb-paths\\bs\\a\\b", "", 0, 0, 0, 0),
    (f"{SCRATCH}/nope/a/b", "", 0, 0, 0, 0),
    ("work\\tddb-paths\\nope2\\a\\b", "", 0, 0, 0, 0),
    # doubled and dot-dot separators are folded by GetFullPathNameW
    (f"{SCRATCH}//double//", "", 0, 0, 0, 0),
    (f"{SCRATCH}/./dot/../dotted", "", 0, 0, 0, 0),
    ("..", "", 0, 0, 0, 0),
    (str(REPO / SCRATCH / "abs"), "", 0, 0, 0, 0),
    # a regular file passes as a directory; one level below it does not
    (f"{SCRATCH}/marker.txt", "", 0, 0, 0, 0),
    (f"{SCRATCH}/marker.txt/sub", "", 0, 0, 0, 0),
    # an invalid name, and a chain of two different mkpath failures
    (f"{SCRATCH}/bad<name", "", 0, 0, 0, 0),
    ("work\\tddb-paths\\bad<name\\sub", "", 0, 0, 0, 0),
    # files_directory: empty (copy), set, the same as the database directory, bad
    (f"{SCRATCH}/rel", f"{SCRATCH}/files", 0, 0, 0, 0),
    (f"{SCRATCH}/rel", f"{SCRATCH}/rel", 0, 0, 0, 0),
    (f"{SCRATCH}/rel", f"{SCRATCH}/marker.txt/sub", 0, 0, 0, 0),
    # the whole use_* matrix on one directory
    *[(f"{SCRATCH}/flags", "", 0, file_flag, chat_flag, msg_flag)
      for msg_flag in (0, 1) for chat_flag in (0, 1) for file_flag in (0, 1)],
]


def script() -> str:
    lines = ["# as_db_key"]
    for key in KEYS:
        lines.append(f"key {key.hex() if key else '-'}")
    lines.append("# check_parameters")
    for database_directory, files_directory, is_test_dc, use_file, use_chat, use_msg in CHECKPARAMS:
        lines.append(
            "checkparams\t{}\t{}\t{}\t{}\t{}\t{}".format(
                database_directory, files_directory, is_test_dc, use_file, use_chat, use_msg
            )
        )
    return "\n".join(lines) + "\n"


def labels() -> list[str]:
    names = [f"key {'-' if not key else key.hex()}" for key in KEYS]
    names += [
        f"checkparams db={database_directory} files={files_directory} tdc={is_test_dc} "
        f"file={use_file} chat={use_chat} msg={use_msg}"
        for database_directory, files_directory, is_test_dc, use_file, use_chat, use_msg in CHECKPARAMS
    ]
    return names


# --- the parts no oracle can reach: TdDb.cpp:49-57 and the flag/message model ---


def get_binlog_path(database_directory: str, is_test_dc: bool) -> str:
    # PSTRING() << parameters.database_directory_ << "td"
    #            << (parameters.is_test_dc_ ? "_test" : "") << ".binlog"
    return database_directory + "td" + ("_test" if is_test_dc else "") + ".binlog"


def get_sqlite_path(database_directory: str, is_test_dc: bool) -> str:
    # const string db_name = "db" + (is_test_dc_ ? string("_test") : string());
    # return database_directory_ + db_name + ".sqlite";
    return database_directory + "db" + ("_test" if is_test_dc else "") + ".sqlite"


class PrepareError(Exception):
    pass


def prepare_dir_model(directory: str, calls: list[str]) -> str:
    """Transcription of check_parameters' prepare_dir, without a filesystem."""
    calls.append(directory)
    return "<" + directory + ">"


def check_parameters_model(database_directory, files_directory, use_file, use_chat, use_msg, prepare):
    """Transcription of TdDb::check_parameters with an injectable prepare_dir."""
    if database_directory == "":
        database_directory = "."
    if use_msg and not use_chat:
        use_chat = True
    if use_chat and not use_file:
        use_file = True
    calls: list[str] = []
    try:
        database_directory = prepare(database_directory, calls)
    except PrepareError as error:
        return {
            "error": f'Can\'t init database in the directory "{database_directory}": {error}',
            "calls": calls,
        }
    if files_directory == "":
        files_directory = database_directory
    else:
        try:
            files_directory = prepare(files_directory, calls)
        except PrepareError as error:
            return {
                "error": f'Can\'t init files directory "{files_directory}": {error}',
                "database": database_directory,
                "calls": calls,
            }
    return {
        "database": database_directory,
        "files": files_directory,
        "file": use_file,
        "chat": use_chat,
        "msg": use_msg,
        "calls": calls,
    }


# --- probing ---


SCRIPT_NAME = "script.txt"


def reset_scratch() -> None:
    """Leave `work/tddb-paths` as the corpus expects to find it: one regular file."""
    WORK.mkdir(parents=True, exist_ok=True)
    for entry in WORK.iterdir():
        if entry.name == SCRIPT_NAME:
            continue
        if entry.is_dir():
            shutil.rmtree(entry)
        else:
            entry.unlink()
    (WORK / "marker.txt").write_bytes(b"marker\n")


def run(binary: str, script_path: Path, env: dict[str, str]) -> tuple[int, list[bytes]]:
    proc = subprocess.run([binary, str(script_path)], capture_output=True, env=env, cwd=str(REPO))
    return proc.returncode, proc.stdout.replace(b"\r\n", b"\n").splitlines()


def tree() -> list[str]:
    """What the run left behind, relative to the scratch root."""
    entries = []
    for root, directories, files in os.walk(WORK):
        relative = Path(root).relative_to(WORK)
        for name in directories:
            entries.append(f"d {relative / name}")
        for name in files:
            entries.append(f"f {relative / name}")
    entries.sort()
    return entries


def compare_tree(cpp_tree: list[str], rs_tree: list[str]) -> int:
    if cpp_tree == rs_tree:
        return 0
    for index in range(max(len(cpp_tree), len(rs_tree))):
        left = cpp_tree[index] if index < len(cpp_tree) else "<missing>"
        right = rs_tree[index] if index < len(rs_tree) else "<missing>"
        if left != right:
            print(f"DIFF tree line {index + 1}: oracle={left!r} rust={right!r}")
            return 1
    return 1


def window(line: bytes, prefix: int, suffix: int, span: int = 40) -> str:
    start = max(0, prefix - span)
    stop = min(len(line), len(line) - suffix + span)
    text = line[start:stop].decode("utf-8", "backslashreplace")
    return ("…" if start else "") + text + ("…" if stop < len(line) else "")


def compare(cpp_rc: int, cpp: list[bytes], rs_rc: int, rs: list[bytes], names: list[str]) -> int:
    for index, (left, right) in enumerate(zip(cpp, rs)):
        if left == right:
            continue
        prefix = 0
        while prefix < min(len(left), len(right)) and left[prefix] == right[prefix]:
            prefix += 1
        suffix = 0
        while (
            suffix < min(len(left), len(right)) - prefix
            and left[len(left) - 1 - suffix] == right[len(right) - 1 - suffix]
        ):
            suffix += 1
        name = names[index] if index < len(names) else "?"
        print(
            f"DIFF line {index + 1}: at byte {prefix} [{name}] "
            f"oracle={window(left, prefix, suffix)!r} rust={window(right, prefix, suffix)!r}"
        )
        return 1
    if len(cpp) != len(rs):
        print(f"DIFF line count: oracle={len(cpp)} rust={len(rs)}")
        return 1
    if cpp_rc != rs_rc:
        print(f"DIFF exit code: oracle={cpp_rc} rust={rs_rc}")
        return 1
    return 0


def captured_compare(*args) -> str:
    buffer = io.StringIO()
    with contextlib.redirect_stdout(buffer):
        status = compare(*args)
    return buffer.getvalue() if status == 1 else ""


def selftest(cpp: list[bytes], cpp_tree: list[str]) -> bool:
    checks: list[tuple[bool, str]] = []

    def check(condition: bool, failure: str) -> None:
        checks.append((condition, failure))

    # The four path templates, from the transcription of TdDb.cpp:49-57. A
    # trailing separator is part of the contract: the templates concatenate, so
    # nothing inserts one.
    for is_test_dc, binlog, sqlite in [
        (False, "C:\\d\\td.binlog", "C:\\d\\db.sqlite"),
        (True, "C:\\d\\td_test.binlog", "C:\\d\\db_test.sqlite"),
    ]:
        check(get_binlog_path("C:\\d\\", is_test_dc) == binlog, f"binlog template with is_test_dc={is_test_dc}")
        check(get_sqlite_path("C:\\d\\", is_test_dc) == sqlite, f"sqlite template with is_test_dc={is_test_dc}")
    check(get_binlog_path("C:\\d", False) == "C:\\dtd.binlog", "the templates do not add a separator")

    # The implication chain, all eight combinations, hand-computed.
    for use_file, use_chat, use_msg, file_out, chat_out, msg_out in [
        (0, 0, 0, 0, 0, 0),
        (1, 0, 0, 1, 0, 0),
        (0, 1, 0, 1, 1, 0),
        (1, 1, 0, 1, 1, 0),
        (0, 0, 1, 1, 1, 1),
        (0, 1, 1, 1, 1, 1),
        (1, 0, 1, 1, 1, 1),
        (1, 1, 1, 1, 1, 1),
    ]:
        result = check_parameters_model("d", "", use_file, use_chat, use_msg, prepare_dir_model)
        check(
            (result["file"], result["chat"], result["msg"]) == (file_out, chat_out, msg_out),
            f"flag chain for file={use_file} chat={use_chat} msg={use_msg}",
        )

    # The "." default and the files_directory copy: prepare_dir sees "." and the
    # files directory is the *normalised* database directory, not the input.
    result = check_parameters_model("", "", 0, 0, 0, prepare_dir_model)
    check(result["calls"] == ["."], 'an empty database directory is prepared as "."')
    check(result["database"] == "<.>", 'the database directory is the prepared "."')
    check(result["files"] == "<.>", "an empty files directory copies the prepared one")
    result = check_parameters_model("db", "", 0, 0, 0, prepare_dir_model)
    check(result["calls"] == ["db"] and result["files"] == "<db>", "a set files directory is prepared on its own")

    # The two 400 texts, with the status rendered into the message.
    def explode(directory: str, calls: list[str]) -> str:
        calls.append(directory)
        raise PrepareError('[WindowsError : sys : 3 : Can\'t create directory "' + directory + '"]')

    result = check_parameters_model("db", "", 0, 0, 0, explode)
    check(
        result["error"] == 'Can\'t init database in the directory "db": '
        '[WindowsError : sys : 3 : Can\'t create directory "db"]',
        "the database failure text",
    )
    result = check_parameters_model(
        "db",
        "files",
        0,
        0,
        0,
        lambda directory, calls: prepare_dir_model(directory, calls) if directory == "db" else explode(directory, calls),
    )
    check(
        result["error"] == 'Can\'t init files directory "files": '
        '[WindowsError : sys : 3 : Can\'t create directory "files"]',
        "the files failure text",
    )
    check(result["database"] == "<db>", "a files failure keeps the normalised database directory")

    # One containment cell per measured rule; these are the oracle's own bytes.
    text = "\n".join(line.decode("utf-8", "backslashreplace") for line in cpp)
    rel = REPO / SCRATCH / "rel"
    corpus_cells = [
        (f"db_in=[] files_in=[] testdc=0 file=0 chat=0 msg=0 -> ok=1 code=0 db=[{REPO}\\] files=[{REPO}\\]",
         "rule 1: an empty database directory resolves to the cwd, and files copies it"),
        ("db_in=[] files_in=[] testdc=1 file=1 chat=1 msg=1 -> ok=1 code=0", "rule 1: empty with is_test_dc=1"),
        ("file=0 chat=0 msg=1 -> ok=1 code=0", "rule 2: msg implies chat and file"),
        ("file=1 chat=0 msg=0 -> ok=1 code=0", "rule 2: an explicit file flag is left alone"),
        (f"db=[{rel}\\] files=[{rel}\\]", "rule 3: exactly one trailing separator, absolute"),
        (f"db=[{REPO.parent}\\]", "rule 3: dot-dot is folded, not appended"),
        ("work\\tddb-paths\\nope2\\a\\b]", "rule 4: a backslash path creates its parents"),
        ("db_in=[work/tddb-paths/nope/a/b]", "rule 4: a slash path is not split by mkpath"),
        (f"db=[{REPO / SCRATCH / 'marker.txt'}\\]", "rule 5: a regular file passes as a directory"),
        ('Can\'t init database in the directory "work/tddb-paths/marker.txt/sub": [WindowsError :',
         "rule 6: the database failure text, code 400"),
        ("-> ok=0 code=400", "rule 6: the outer code is 400"),
        (": 123 : Can't create directory \"work/tddb-paths/bad<name\"]",
         "rule 6: an invalid name is ERROR_INVALID_NAME, unlocalised digits"),
        ('\\bad<name\\sub": [WindowsError', "rule 7: two distinct mkpath failures are chained"),
        ('files_in=[work/tddb-paths/marker.txt/sub] testdc=0 file=0 chat=0 msg=0 -> ok=0 code=400 '
         f'db=[{rel}\\]', "rule 6: a files failure keeps the normalised database directory"),
        ("testdc=1 file=1 chat=1 msg=1 -> ok=1", "rule 8: is_test_dc does not change the flags"),
    ]
    for expected, failure in corpus_cells:
        check(expected in text, f"the oracle output is missing {expected!r} ({failure})")

    # One flipped byte must be caught by the same comparator.
    flipped = list(cpp)
    first = flipped[0]
    flipped[0] = first[:8] + bytes([first[8] ^ 1]) + first[9:]
    captured = captured_compare(0, flipped, 0, cpp, labels())
    check(captured.startswith("DIFF line 1: at byte 8 "), "the comparator catches a one-byte flip")

    # The tree mkpath must have created, hand-computed from the corpus: one
    # directory per successful cell (marker.txt is a file, `..`/`.` create
    # nothing), with `bs\a\b` and `nope2\a\b` each getting three levels because
    # mkpath splits on '\\', while the '/' cells only ever create one directory.
    expected_tree = [
        "d abs",
        "d bs",
        "d bs\\a",
        "d bs\\a\\b",
        "d dotted",
        "d double",
        "d files",
        "d flags",
        "d nope2",
        "d nope2\\a",
        "d nope2\\a\\b",
        "d rel",
        "f marker.txt",
        "f script.txt",
    ]
    check(cpp_tree == expected_tree, f"the created tree: {cpp_tree!r}")
    tree_buffer = io.StringIO()
    with contextlib.redirect_stdout(tree_buffer):
        caught = compare_tree(expected_tree, expected_tree[:-1])
    check(caught == 1, "the tree comparator catches a missing directory")

    for condition, failure in checks:
        if not condition:
            print(f"SELFTEST FAIL: {failure}")
            return False
    print(
        "SELFTEST OK: path templates, flag chain, \".\" default, files copy, 400 texts, 15 corpus cells, "
        "created tree, comparators"
    )
    return True


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
    script_path = WORK / SCRIPT_NAME
    script_path.write_text(text, encoding="utf-8", newline="\n")

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    reset_scratch()
    cpp_rc, cpp = run(args.cpp, script_path, env)
    cpp_tree = tree()
    reset_scratch()
    rs_rc, rs = run(args.rs, script_path, env)
    rs_tree = tree()
    status = compare(cpp_rc, cpp, rs_rc, rs, labels())
    if status != 0:
        return status
    # realpath is lexical, so a skipped mkdir would leave the narration intact:
    # the directory tree each side creates is part of the criterion.
    if compare_tree(cpp_tree, rs_tree) != 0:
        return 1

    reset_scratch()
    floor_rc, floor = run(args.cpp, script_path, env)
    if (floor_rc, floor, tree()) != (cpp_rc, cpp, cpp_tree):
        print("FLOOR FAIL: the oracle is not deterministic")
        return 1

    if args.selftest and not selftest(cpp, cpp_tree):
        return 1

    print(f"PASS lines={len(cpp)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())