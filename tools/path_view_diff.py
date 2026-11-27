"""Compare tdutils PathView with TDLib.

    uv run python tools/path_view_diff.py
    uv run python tools/path_view_diff.py --selftest

PathView is pure byte slicing, so the whole surface fits in one script: the
constructor's last_slash/last_dot scan, the five name/extension accessors,
`parent_dir` vs `parent_dir_noslash` (which returns TD_DIR_SLASH for a root-only
path — '\\\\' in this Windows build), `is_dir` / `is_absolute`, the
string-prefix `relative`, and the two-segment `dir_and_file`.

Every slice is printed as hex ("-" for empty), so backslashes and trailing
slashes stay unambiguous.
"""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import io
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/path_view_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/path_view_probe.exe"
WORK = REPO / "work" / "path"

PATHS = [
    "",
    ".",
    "..",
    "a",
    "abc",
    ".bashrc",
    "a.b",
    "a.b.c",
    "a.",
    "a..b",
    "/",
    "//",
    "/a",
    "/a/",
    "/a/b",
    "/a/b/",
    "/a/b/c",
    "/a/b/c/",
    "\\a\\b",
    "a\\b\\c",
    "C:\\x",
    "C:/x",
    "C:",
    "c:x",
    "C:\\",
    "\\\\srv\\share",
    "dir/.hidden",
    "dir/.hidden.txt",
    "a.b/c",
    "/a/.b/c",
    "a//b",
    "a/b.",
    "x",
    "x/y",
]

REL = [
    ("/a/bc", "/a/b", False),
    ("/a/b/c", "/a/", False),
    ("/x", "/y", False),
    ("/x", "/y", True),
    ("abc", "abc", False),
    ("abcd", "abc", False),
    ("", "", False),
    ("/a", "", False),
    ("C:\\x", "C:\\", False),
]

DIRFILE = ["a/b/c", "a/b", "/x/y", "x/y", "/x", "a/b/", "a/b/c/d", "", "/", "//", "a/b/c/"]


def token(text: str) -> str:
    """An empty path cannot be a script token, so it is written as '-'."""
    return text if text else "-"


def hex_of(text: str) -> str:
    return text.encode().hex() if text else "-"


def script() -> str:
    lines = ["# one PathView per path, then the two static helpers"]
    for path in PATHS:
        lines.append(f"path {token(path)}")
    for path, directory, force in REL:
        lines.append(f"rel {token(path)} {token(directory)} {1 if force else 0}")
    for path in DIRFILE:
        lines.append(f"dirfile {token(path)}")
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
    script_path = WORK / "path_view_script.txt"
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
            ("path in=- empty=1 dir=0 abs=0 rel=1 parent=- pns=2e ext=- stem=- name=- nodot=- nox=-",
             "the empty path"),
            (f"path in={hex_of('.bashrc')} empty=0 dir=0 abs=0 rel=1 parent=- pns=2e ext=- "
             f"stem={hex_of('.bashrc')} name={hex_of('.bashrc')} nodot={hex_of('.bashrc')} nox={hex_of('.bashrc')}",
             "a leading dot must not count as an extension"),
            (f"path in={hex_of('a.b')} empty=0 dir=0 abs=0 rel=1 parent=- pns=2e ext=62 stem=61 "
             f"name={hex_of('a.b')} nodot=61 nox=61",
             "a.b split"),
            (f"path in={hex_of('a.')} empty=0 dir=0 abs=0 rel=1 parent=- pns=2e ext=- stem=61 "
             f"name={hex_of('a.')} nodot=61 nox=61",
             "a trailing dot gives an empty extension"),
            (f"path in={hex_of('C:\\x')} empty=0 dir=0 abs=1 rel=0 parent={hex_of('C:\\')} pns={hex_of('C:')} "
             f"ext=- stem=78 name=78 nodot=78 nox={hex_of('C:\\x')}",
             "a drive-letter absolute path"),
            (f"path in={hex_of('/a')} empty=0 dir=0 abs=1 rel=0 parent=2f pns=5c ext=- stem=61 name=61 "
             f"nodot=61 nox=2f61",
             "parent_dir_noslash of a root-only path is the Windows separator"),
            ("rel in=2f612f6263 dir=2f612f62 force=0 out=63",
             "relative is a plain string prefix, not a path-component match"),
            ("rel in=2f78 dir=2f79 force=1 out=-", "relative with force returns empty on a mismatch"),
            (f"dirfile in={hex_of('a/b/c')} out={hex_of('b/c')}", "dir_and_file returns the last two segments"),
            (f"dirfile in={hex_of('a/b')} out=-", "dir_and_file needs two separators"),
        ]
        for expected, failure in checks:
            if expected not in cpp_out:
                print(f"SELFTEST FAIL: {failure} (missing {expected!r})")
                return 1
        # One flipped output field must be caught by the same comparator.
        lines = cpp_out.splitlines()
        index = next(index for index, line in enumerate(lines) if line.startswith("path "))
        lines[index] = lines[index].replace(" dir=", " dir=9", 1)
        captured = io.StringIO()
        with contextlib.redirect_stdout(captured):
            status = compare(0, "\n".join(lines) + "\n", 0, cpp_out)
        if status != 1:
            print(f"SELFTEST FAIL: the comparator missed a flipped field\n{captured.getvalue()}")
            return 1
        print("SELFTEST OK: dot scan, empty/trailing dot, drive letter, TD_DIR_SLASH, relative, dir_and_file")

    print(f"PASS lines={len(cpp_out.splitlines())}")
    return 0


if __name__ == "__main__":
    sys.exit(main())