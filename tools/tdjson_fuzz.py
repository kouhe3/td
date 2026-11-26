"""Randomised differential fuzz lane over the `td_execute` whitelist.

命令:
    uv run python tools/tdjson_fuzz.py [--seed N] [--cases N]

判据:
    退出码 0 = 21 个无状态 action 的随机输入两侧逐字节相同，且 oracle 侧把同一份语料跑两遍
    逐条相同（A/A 地板）。任一分歧都打印两侧原文并退出 1。

覆盖边界:
    只覆盖无状态 action。8 个日志类 action（verbosity / stream / tags / addLogMessage）在同一进程里
    留全局状态，两侧交错跑会互相污染，它们由 `tdjson_diff.py` 的 SEQUENCE 段按有序序列覆盖。
    action 白名单从 `td/telegram/SynchronousRequests.cpp` 现读，不在这里手工维护。

确定性:
    语料只由 `--seed` 决定（Mersenne Twister），生成过程中不迭代 set；每轮打印语料 md5，漂移可见。
    字符串样本与变异字母表全是合法 UTF-8：`td::JsonBuilder` 遇到非法 UTF-8 是 CHECK 失败，
    会把 oracle 进程直接打死，而非法 UTF-8 的拒绝路径已由手写语料覆盖。
"""

from __future__ import annotations

import argparse
import hashlib
import json
import random
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gen_tl_schema
import tdjson_diff as instrument

SYNCHRONOUS_SOURCE = Path(__file__).resolve().parents[1] / "td/telegram/SynchronousRequests.cpp"

# 全局状态类 action：有序序列才可判（tdjson_diff.py 的 SEQUENCE 段）
STATEFUL = {
    "addLogMessage",
    "getLogStream",
    "getLogTagVerbosityLevel",
    "getLogTags",
    "getLogVerbosityLevel",
    "setLogStream",
    "setLogTagVerbosityLevel",
    "setLogVerbosityLevel",
}

TEXT_SAMPLES = [
    "",
    "a",
    "hello world",
    "line\nbreak",
    "tab\ttab",
    '"quoted"',
    "back\\slash",
    "你好",
    "😀",
    "é",
    "a" * 300,
    "#tag @user $USD $btc",
    "https://e.com/path?x=1",
    "tg://resolve?domain=durov",
    "a@b.com",
    "01:23",
    "4111111111111111",
    "<b>bold</b> &amp",
    "*bold* __it__ `code`",
    "\\ud83d\\ude00",
    "\u0000nul",
    "\u200d",
    "a" * 64 + ".txt",
    " "
    * 3,
    "-",
    "Xy",
]

WRONG_VALUES = [None, 0, 1, -1, 1.5, True, False, "", "x", [], {}, [1], {"@type": "nope"}]


def synchronous_functions() -> list[str]:
    """The whitelist is read from the oracle's own source, not maintained here (F016/F026)."""
    names = set(re.findall(r"case td_api::([A-Za-z_]+)::ID:", SYNCHRONOUS_SOURCE.read_text(encoding="utf-8")))
    names -= {"textParseModeHTML", "textParseModeMarkdown"}
    if len(names) != 29:
        raise SystemExit(f"FAIL expected 29 synchronous functions in {SYNCHRONOUS_SOURCE}, found {len(names)}")
    return sorted(names)


def class_of(definitions: dict[str, tuple[list[tuple[str, str]], str]], name: str) -> str:
    return definitions[name][1].split("=", 1)[1].strip()


def constructors_of(definitions: dict[str, tuple[list[tuple[str, str]], str]], class_name: str) -> list[str]:
    return sorted(
        name
        for name in definitions
        if name != class_name and class_of(definitions, name) == class_name
    )


def scalar(kind: str, rng: random.Random):
    if kind in ("int32", "int53", "int64"):
        return rng.choice([0, 1, -1, 7, 2147483647, -2147483648, 2**31, 2**53, 9007199254740993, "x", 1.5, None, True])
    if kind in ("Bool", "bool"):
        return rng.choice([True, False, 0, 1, "true", None])
    if kind in ("double", "Double"):
        return rng.choice([0.5, -0.0, 1e308, 1e-7, 0, 1, "x", None])
    if kind in ("string", "String", "bytes"):
        return rng.choice(TEXT_SAMPLES + [5, None, [], {}, True])
    return rng.choice(WRONG_VALUES)


def value_of(
    definitions: dict[str, tuple[list[tuple[str, str]], str]],
    kind: str,
    rng: random.Random,
    depth: int,
):
    base = kind.rstrip("?")
    if base.startswith("vector<"):
        inner = base[len("vector<"):-1]
        return [value_of(definitions, inner, rng, depth) for _ in range(rng.randrange(0, 3))]
    if base in ("int32", "int53", "int64", "Bool", "bool", "double", "Double", "string", "String", "bytes"):
        return scalar(base, rng)
    if base not in definitions:
        return rng.choice(WRONG_VALUES)
    # class or a concrete constructor name
    roll = rng.random()
    if roll < 0.15:
        return rng.choice([None, 5, "x", []])
    if roll < 0.30:
        return {}
    if depth >= 2:
        return {"@type": base} if class_of(definitions, base) != base else {}
    if class_of(definitions, base) == base:
        options = constructors_of(definitions, base)
        if not options:
            return {}
        return object_of(definitions, rng.choice(options), rng, depth + 1)
    return object_of(definitions, base, rng, depth + 1)


def object_of(
    definitions: dict[str, tuple[list[tuple[str, str]], str]],
    name: str,
    rng: random.Random,
    depth: int,
):
    obj: dict[str, object] = {"@type": name}
    for field, kind in definitions[name][0]:
        roll = rng.random()
        if roll < 0.10:
            continue
        if roll < 0.22:
            obj[field] = rng.choice(WRONG_VALUES)
        else:
            obj[field] = value_of(definitions, kind, rng, depth)
    return obj


def function_call(
    definitions: dict[str, tuple[list[tuple[str, str]], str]],
    name: str,
    rng: random.Random,
):
    """A near-valid request: every field present, a few corrupted or dropped."""
    request = object_of(definitions, name, rng, 0)
    if name == "getLanguagePackString":
        # 随机路径会让 oracle 在任意目录建 sqlite 库；路径固定到 fixture，其余字段照旧随机
        request["language_pack_database_path"] = "@LP@/full.db"
        request["localization_target"] = rng.choice(["android", "ios", "a", "A_1", "androidx", ""])
        request["language_pack_id"] = rng.choice(["en", "de", "Xy", "X", "e", ""])
        request["key"] = rng.choice(["Continue", "Plural", "Deleted", "Absent", "a.b-c_d", "!version", "bad key"])
    return request


MUTATION_BYTES = [bytes([byte]) for byte in b'{}[]":,0123456789abcdefXYZ \t\n\\/.@_-#$'] + [
    "é".encode(),
    "你".encode(),
    "😀".encode(),
]


def mutate(rng: random.Random, text: bytes) -> bytes:
    data = bytearray(text)
    for _ in range(rng.randrange(1, 4)):
        if not data:
            break
        op = rng.randrange(3)
        pos = rng.randrange(len(data))
        insert = rng.choice(MUTATION_BYTES)
        if op == 0:
            data[pos : pos + 1] = insert
        elif op == 1:
            data[pos:pos] = insert
        else:
            del data[pos : pos + 1]
    return bytes(data)


def build_corpus(seed: int, cases: int) -> list[tuple[str, bytes]]:
    rng = random.Random(seed)
    definitions = gen_tl_schema.parse_schema()
    targets = sorted(set(synchronous_functions()) - STATEFUL)
    corpus: list[tuple[str, bytes]] = []

    for index in range(cases * len(targets)):
        name = targets[index % len(targets)]
        request = function_call(definitions, name, rng)
        text = json.dumps(request, ensure_ascii=False, separators=(",", ":"))
        corpus.append((name, text.encode()))

    # 变异：从手写语料里取种子（跳过 getLanguagePackString 与其空路径），做字节级改动
    seeds = [
        case
        for case in instrument.MATCH
        if b"getLanguagePackString" not in (case if isinstance(case, bytes) else case.encode())
    ]
    for _ in range(max(1, cases // 4)):
        case = rng.choice(seeds)
        payload = case if isinstance(case, bytes) else case.encode()
        if payload[:1] != b"{" or b'"@type"' not in payload:
            continue
        corpus.append(("mutation", mutate(rng, payload)))

    return corpus


def corpus_md5(corpus: list[tuple[str, bytes]]) -> str:
    digest = hashlib.md5()
    for name, payload in corpus:
        digest.update(name.encode())
        digest.update(b"\x00")
        digest.update(payload)
        digest.update(b"\x01")
    return digest.hexdigest()


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--seed", type=int, default=20261126)
    parser.add_argument("--cases", type=int, default=40, help="cases per target before mutations")
    parser.add_argument("--skip-md5", action="store_true")
    args = parser.parse_args(argv)

    if not instrument.CPP.is_file():
        print(f"FAIL oracle missing: {instrument.CPP}")
        return 1
    if not instrument.RS.is_file():
        print(f"FAIL rust dll missing: {instrument.RS}")
        return 1
    actual = instrument.md5(instrument.CPP)
    if not args.skip_md5 and actual != instrument.EXPECTED_CPP_MD5:
        print(f"FAIL oracle md5 changed: {actual} != {instrument.EXPECTED_CPP_MD5}")
        return 1

    instrument.add_runtime_dirs()
    if not instrument.ensure_language_pack_dirs():
        print(f"FAIL language pack fixtures missing: {instrument.LANGPACK_ROOT}")
        return 1
    cpp = instrument.load(instrument.CPP, "cpp")
    rs = instrument.load(instrument.RS, "rs")

    corpus = build_corpus(args.seed, args.cases)
    targets = sorted(set(synchronous_functions()) - STATEFUL)
    print(f"oracle md5={actual}")
    print(f"seed={args.seed} cases={len(corpus)} corpus md5={corpus_md5(corpus)}")
    print(f"fuzz targets={len(targets)}: {' '.join(targets)}")

    failures: list[str] = []
    empty = 0
    per_action: dict[str, int] = {}
    cpp_first = [instrument.run(cpp, payload) for _, payload in corpus]
    cpp_second = [instrument.run(cpp, payload) for _, payload in corpus]
    for index, (name, payload) in enumerate(corpus):
        per_action[name] = per_action.get(name, 0) + 1
        if cpp_first[index] is None or cpp_first[index] == "":
            empty += 1
        if cpp_first[index] != cpp_second[index]:
            failures.append(f"floor unstable on {name}: {instrument.label(payload)}")
            print(f"FAIL floor {instrument.label(payload)}")
            print(f"  a: {cpp_first[index]}")
            print(f"  b: {cpp_second[index]}")

    rs_first = [instrument.run(rs, payload) for _, payload in corpus]
    differenced = 0
    for index, (name, payload) in enumerate(corpus):
        if cpp_first[index] != rs_first[index]:
            differenced += 1
            failures.append(f"diverged on {name}: {instrument.label(payload)}")
            print(f"FAIL {name} {instrument.label(payload)}")
            print(f"  cpp: {cpp_first[index]}")
            print(f"  rs : {rs_first[index]}")

    missing = [name for name in targets if per_action.get(name, 0) == 0]
    if missing:
        failures.append(f"targets never fuzzed: {missing}")
    if empty * 10 > len(corpus):
        failures.append(f"{empty} of {len(corpus)} cases produced no output (pipeline suspect)")

    print("\nper-action cases:")
    for name in targets:
        print(f"  {name}: {per_action.get(name, 0)}")
    print(f"\ncases={len(corpus)} diverged={differenced} empty={empty} failures={len(failures)}")
    for failure in failures[:20]:
        print(f"  - {failure}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))