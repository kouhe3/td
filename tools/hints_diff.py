"""Compare tdutils Hints with TDLib, unicode words and transliteration maps.

    uv run python tools/hints_diff.py
    uv run python tools/hints_diff.py --selftest

The script drives one Hints instance: adds, an idempotent re-add, a rename, a
remove, ratings, prefix searches, intersections, every limit shape (0, 1, 2,
larger than the match count, negative) and the empty query. Results are printed
as `total` (before the limit) plus the key list sorted by (rating ascending, key
ascending) — the *lowest* rating comes first.

The unicode phase exercises `utf8_get_search_words` (full-width ASCII, Greek
tonos, Cyrillic case, diacritics) and the two maps of `Hints`: every word of a
name is indexed verbatim, and `fix_words(get_word_transliterations(word, false))`
is indexed into the transliteration map. A query looks up the transliteration
map, then the transliteration of the query with `allow_partial = true` against
the word map, then dedups the union. So `shc` only finds `щука` through the
partial rule `shch` and `schuka` only through the transliteration map, because
the reverse rule turns `sch` into `ш`, not `щ`. A rename at the end proves that
both maps are cleaned up.
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
DEFAULT_CPP = "C:/tmp/td-probe/hints_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/hints_probe.exe"
WORK = REPO / "work" / "hints"


def hx(text: str) -> str:
    return text.encode().hex() if text else "-"


def script() -> str:
    lines = ["# one Hints instance: add, re-add, rename, remove, ratings, limits"]
    lines += [
        "size",
        f"add 1 {hx('alice')}",
        f"add 2 {hx('alice smith')}",
        f"add 3 {hx('bob')}",
        f"add 4 {hx('bob smith')}",
        f"add 5 {hx('alice bob')}",
        f"add 1 {hx('alice')}",  # idempotent
        f"add 6 {hx('a')}",
        f"add 7 {hx('ab')}",
        "size",
    ]
    for query, limit, flag in [
        ("alice", 10, "all"),
        ("ali", 10, "all"),
        ("a", 10, "all"),
        ("alice s", 10, "all"),
        ("bob", 10, "all"),
        ("bob a", 10, "all"),
        ("smith", 10, "all"),
        ("xyz", 10, "all"),
        ("", 10, "all"),
        ("", 10, ""),
    ]:
        lines.append(f"search {hx(query)} {limit} {flag}".strip())
    for limit in (0, 1, 2, 3, 5, 100, -1):
        lines.append(f"search {hx('a')} {limit} all")
        lines.append(f"sempty {limit}")
    lines += [
        "rate 1 5",
        "rate 2 1",
        "rate 3 3",
        "search " + hx("a") + " 10 all",
        "search " + hx("a") + " 2 all",
        "search " + hx("a") + " 3 all",
        "has 1",
        "has 99",
        f"name 1",
        "name 99",
        "remove 2",
        "size",
        "has 2",
        "name 2",
        f"search {hx('alice')} 10 all",
        f"search {hx('smith')} 10 all",
        f"add 3 {hx('robert')}",
        "size",
        f"search {hx('bob')} 10 all",
        f"search {hx('robert')} 10 all",
        f"name 3",
        "search " + hx("a") + " 10 all",
        "remove 1",
        "remove 4",
        "remove 5",
        "remove 6",
        "remove 7",
        "size",
        f"search {hx('a')} 10 all",
        "sempty 5",
        # a repeated word must be collapsed by fix_words, otherwise add_word hits
        # its CHECK(!contains(keys, key))
        f"add 9 {hx('alice alice')}",
        "size",
        f"search {hx('alice')} 10 all",
    ]
    lines.append("# utf8_get_search_words before fix_words: folding and splitting")
    for text in ["ПРИВЕТ", "ＰＲＩＶＥＴ", "ΣΟΦΊΑ", "ΑΘΗΝΑ", "αθήνα", "ий", "ё", "привет-мир!", "İstanbul", "naïve",
                 "ÄÖÜ", ""]:
        lines.append(f"words {hx(text)}")
    lines.append("# a Cyrillic name, its Latin twin, and the rules that are not symmetric")
    for key, name in [
        (10, "привет"),
        (11, "привет мир"),
        (12, "privet"),
        (13, "й"),
        (14, "ё"),
        (15, "кс"),
        (16, "ий"),
        (17, "Иван"),
        (18, "ivan"),
        (19, "ＰＲＩＶＥＴ"),
        (20, "ΣΟΦΙΑ"),
        (21, "αθήνα"),
        (22, "привет-мир!"),
        (23, "Ёлка"),
        (24, "щука"),
        (25, "ый"),
    ]:
        lines.append(f"add {key} {hx(name)}")
    lines += ["size", f"add 10 {hx('привет')}", "size"]
    for query in ["привет", "privet", "priv", "ПРИВЕТ", "ＰＲＩＶＥＴ", "ｉｖａｎ", "и", "й", "ё", "е", "e", "y",
                  "к", "кс", "ks", "x", "shc", "schuka", "щука", "иван", "ivan", "ИВАН", "iv", "ыи", "yi", "mir",
                  "мир", "ΣΟΦΙΑ", "σοφια", "αθήνα", "αθηνα", "ΑΘΗΝΑ"]:
        lines.append(f"search {hx(query)} 10 all")
    for limit in (0, 2, 100, -1):
        lines.append(f"search {hx('привет')} {limit} all")
    lines += [
        "rate 11 2",
        "rate 13 5",
        "rate 22 -1",
        f"search {hx('привет')} 10 all",
        f"search {hx('привет')} 2 all",
        f"search {hx('y')} 10 all",
        "# a rename must clear the old word and its transliteration from both maps",
        f"add 26 {hx('привет')}",
        "size",
        f"search {hx('privet')} 10 all",
        "name 26",
        f"add 26 {hx('пока')}",
        "size",
        "name 26",
        f"search {hx('privet')} 0 all",
        f"search {hx('пока')} 10 all",
        f"search {hx('poka')} 10 all",
        f"search {hx('привет')} 10 all",
        "remove 26",
        "size",
        f"search {hx('poka')} 10 all",
        f"search {hx('пока')} 10 all",
        "has 26",
    ]
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
    script_path = WORK / "hints_script.txt"
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
            (f"search q={hx('alice')} limit=10 total=3 keys=1,2,5", "prefix search for alice"),
            (f"search q={hx('a')} limit=10 total=5 keys=1,2,5,6,7", "the one-letter prefix"),
            (f"search q={hx('bob a')} limit=10 total=1 keys=5", "two words intersect"),
            (f"search q={hx('smith')} limit=10 total=2 keys=2,4", "the second word of a name"),
            (f"search q={hx('xyz')} limit=10 total=0 keys=-", "no match"),
            (f"search q=- limit=10 total=0 keys=-", "an empty query without the all flag"),
            (f"search q={hx('a')} limit=0 total=5 keys=-", "limit 0 keeps the total and drops the list"),
            (f"search q={hx('a')} limit=-1 total=7 keys=-", "a negative limit returns the size and nothing"),
            (f"search q={hx('a')} limit=10 total=5 keys=5,6,7,2,1",
             "results are ordered by rating ascending, so the lowest rating comes first"),
            ("size n=7", "the key count"),
            ("has key=1 r=1", "has_key for a present key"),
            ("has key=99 r=0", "has_key for a missing key"),
            ("name key=99 name=-", "key_to_string for a missing key"),
            ("remove key=2 size=6", "remove"),
            ("size n=6", "size after remove"),
            (f"search q={hx('bob')} limit=10 total=2 keys=4,5", "the rename removed the old word"),
            ("add key=9 size=2", "a name with a duplicated word is accepted"),
            (f"search q={hx('alice')} limit=10 total=1 keys=9", "the duplicated word is indexed once"),
            (f"words in={hx('ПРИВЕТ')} out={hx('привет')}", "Cyrillic case is folded by the search words"),
            (f"words in={hx('ＰＲＩＶＥＴ')} out={hx('privet')}", "full-width ASCII is folded to ASCII"),
            (f"words in={hx('ΣΟΦΊΑ')} out={hx('σοφια')}", "the Greek tonos is removed"),
            (f"words in={hx('ий')} out={hx('ии')}", "й is folded to и before any transliteration"),
            (f"words in={hx('ё')} out={hx('е')}", "ё is folded to е before any transliteration"),
            (f"words in={hx('привет-мир!')} out={hx('привет')},{hx('мир')}",
             "punctuation splits a name into two search words"),
            (f"search q={hx('ПРИВЕТ')} limit=10 total=5 keys=10,11,12,19,22",
             "a Cyrillic query reaches the Latin names through the transliteration map"),
            (f"search q={hx('privet')} limit=10 total=5 keys=10,11,12,19,22",
             "a Latin query reaches the Cyrillic names"),
            (f"search q={hx('schuka')} limit=10 total=1 keys=24",
             "only the transliteration map finds щука, the reverse rule gives шкука"),
            (f"search q={hx('shc')} limit=10 total=1 keys=24",
             "the partial rule shch -> щ is the only path from shc to щука"),
            (f"search q={hx('и')} limit=10 total=4 keys=13,16,17,18",
             "the prefix word finds й, ий, Иван and the Latin ivan"),
            (f"search q={hx('ё')} limit=10 total=2 keys=14,23",
             "ё is searched as е, so it meets е and the transliteration of елка"),
            (f"search q={hx('σοφια')} limit=10 total=1 keys=20", "Greek stays Greek, only case and accents fold"),
            (f"search q={hx('αθηνα')} limit=10 total=1 keys=21", "the tonos of αθήνα is gone for the query too"),
            (f"search q={hx('привет')} limit=0 total=5 keys=-", "limit 0 keeps the total on the unicode path"),
            (f"search q={hx('привет')} limit=-1 total=18 keys=-", "a negative limit returns the size of the whole Hints"),
            (f"search q={hx('привет')} limit=10 total=5 keys=22,10,12,19,11",
             "the rating of 22 is -1, so it sorts before the unrated keys, and 11 rated 2 comes last"),
            (f"search q={hx('y')} limit=10 total=6 keys=14,16,17,23,25,13",
             "the transliteration map answers yi for ый, and 13 rated 5 sorts last"),
            ("add key=26 size=19", "a second name for key 26"),
            (f"search q={hx('privet')} limit=10 total=6 keys=22,10,12,19,26,11",
             "the new Cyrillic name lands in the transliteration map, ordered by the ratings set above"),
            (f"search q={hx('privet')} limit=0 total=5 keys=-",
             "the rename removed the privet transliteration of key 26"),
            (f"search q={hx('пока')} limit=10 total=1 keys=26", "the new word of the renamed key"),
            (f"search q={hx('poka')} limit=10 total=1 keys=26", "the new transliteration of the renamed key"),
            (f"search q={hx('poka')} limit=10 total=0 keys=-", "remove clears the transliteration map as well"),
            ("size n=18", "the key count after removing 26"),
        ]
        for expected, failure in checks:
            if expected not in cpp_out:
                print(f"SELFTEST FAIL: {failure} (missing {expected!r})")
                return 1
        print(f"SELFTEST OK: {len(checks)} hand-checked cells (prefix, intersection, limits, lifecycle)")

    print(f"PASS lines={len(cpp_out.splitlines())}")
    return 0


if __name__ == "__main__":
    sys.exit(main())