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

# 有状态 action 的随机序列段数；每段自带恢复步骤
SEQUENCE_COUNT = 20

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


HEADER_IDS = gen_tl_schema.header_ids()


def constructor_id(name: str) -> int | None:
    """The build's own id table (F091); builtins without a dispatch id return None."""
    return HEADER_IDS.get(name)


def bytes_allowlist() -> set[str]:
    """Actions whose corpus already contains invalid-UTF-8 cells (F063), i.e. safe to mutate that way.

    Anywhere else invalid UTF-8 can reach td::JsonBuilder, which CHECK-fails and kills the oracle
    process instead of returning an error.
    """
    names: set[str] = set()
    for case in instrument.MATCH:
        if not isinstance(case, bytes):
            continue
        match = re.search(rb'"@type":"([A-Za-z]+)"', case)
        if match:
            names.add(match.group(1).decode())
    return names


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
    if depth >= 3:
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
    # "@type" may be the dispatch id from the build's own table instead of the name (F082, F091)
    dispatch_id = constructor_id(name)
    type_name: object = name
    if dispatch_id is not None and rng.random() <= 0.2:
        type_name = dispatch_id
    obj: dict[str, object] = {"@type": type_name}
    for field, field_kind in definitions[name][0]:
        roll = rng.random()
        if roll < 0.10:
            continue
        if roll < 0.22:
            obj[field] = rng.choice(WRONG_VALUES)
        else:
            obj[field] = value_of(definitions, field_kind, rng, depth)
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


def inject_bad_utf8(rng: random.Random, payload: bytes) -> bytes:
    """Drop one invalid byte inside the first string literal that has room for it.

    Only used for actions whose corpus already carries invalid-UTF-8 cells (F063); everywhere else
    the oracle can CHECK-abort instead of answering.
    """
    quotes = [index for index, byte in enumerate(payload) if byte == 0x22]
    if len(quotes) < 2:
        return payload
    position = quotes[rng.randrange(len(quotes) - 1)] + 1
    return payload[:position] + bytes([rng.choice([0xFF, 0xFE, 0xC0, 0x80])]) + payload[position:]


def build_corpus(seed: int, cases: int) -> list[tuple[str, bytes]]:
    rng = random.Random(seed)
    definitions = gen_tl_schema.parse_schema()
    targets = sorted(set(synchronous_functions()) - STATEFUL)
    unsafe_ok = bytes_allowlist()
    corpus: list[tuple[str, bytes]] = []

    for index in range(cases * len(targets)):
        name = targets[index % len(targets)]
        request = function_call(definitions, name, rng)
        text = json.dumps(request, ensure_ascii=False, separators=(",", ":"))
        payload = text.encode()
        if name in unsafe_ok and rng.random() < 0.2:
            payload = inject_bad_utf8(rng, payload)
        corpus.append((name, payload))

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


LOG_TAGS = [
    "td_requests",
    "notifications",
    "proxy",
    "sqlite",
    "td_init",
    "update_file",
    "raw_mtproto",
    "no_such_tag",
]
LOG_DIR = "work/logs-fuzz"


def log_step(rng: random.Random, index: int) -> bytes:
    """One stateful request; paths stay inside work/logs-fuzz so nothing escapes the repo."""
    levels = [-1, 0, 1, 2, 3, 5, 7, 8]
    tag = rng.choice(LOG_TAGS)
    choice = rng.randrange(7)
    if choice == 0:
        request = {"@type": "getLogVerbosityLevel"}
    elif choice == 1:
        request = {"@type": "setLogVerbosityLevel", "new_verbosity_level": rng.choice(levels)}
    elif choice == 2:
        request = {"@type": "getLogTagVerbosityLevel", "tag": tag}
    elif choice == 3:
        request = {"@type": "setLogTagVerbosityLevel", "tag": tag, "new_verbosity_level": rng.choice(levels)}
    elif choice == 4:
        request = {
            "@type": "addLogMessage",
            # level 0 是 fatal：oracle 会直接 abort（F093），所以只用 >= 1
            "verbosity_level": rng.choice([1, 2, 3, 5, 7, 8]),
            "text": rng.choice(TEXT_SAMPLES),
        }
    elif choice == 5:
        request = {"@type": "getLogStream"}
    else:
        stream: dict[str, object] = rng.choice(
            [
                {"@type": "logStreamDefault"},
                {"@type": "logStreamEmpty"},
                {
                    "@type": "logStreamFile",
                    "path": f"{LOG_DIR}/seq-{index}.log",
                    "max_file_size": rng.choice([0, 1, 1024, 4096]),
                },
            ]
        )
        request = {"@type": "setLogStream", "log_stream": stream}
    return json.dumps(request, ensure_ascii=False, separators=(",", ":")).encode()


def build_log_sequences(seed: int, count: int) -> list[list[bytes]]:
    """Stateful actions are only comparable as ordered sequences, and every sequence restores state."""
    rng = random.Random(seed * 31 + 7)
    # 每个 tag 的默认级别是 oracle 的内置常量，读不到就先统一设成 3：这样任何一段的结果
    # 都不依赖它前面跑过什么（否则第一遍和第二遍会不同）
    reset = [
        json.dumps(
            {"@type": "setLogTagVerbosityLevel", "tag": tag, "new_verbosity_level": 3},
            separators=(",", ":"),
        ).encode()
        for tag in LOG_TAGS
        if tag != "no_such_tag"
    ]
    reset += [
        b'{"@type":"setLogVerbosityLevel","new_verbosity_level":5}',
        b'{"@type":"setLogStream","log_stream":{"@type":"logStreamDefault"}}',
    ]
    sequences: list[list[bytes]] = []
    for index in range(count):
        steps = [log_step(rng, index) for _ in range(rng.randrange(3, 7))]
        sequences.append(reset + steps + reset)
    return sequences


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

    print("\n== 有状态 action：随机序列（每段结束恢复 default / 5） ==")
    Path(LOG_DIR).mkdir(parents=True, exist_ok=True)
    sequences = build_log_sequences(args.seed, SEQUENCE_COUNT)

    def run_sequences(lib) -> list[list[str | None]]:
        return [[instrument.run(lib, step) for step in sequence] for sequence in sequences]

    cpp_sequences = run_sequences(cpp)
    cpp_sequences_again = run_sequences(cpp)
    rs_sequences = run_sequences(rs)
    sequence_steps = sum(len(sequence) for sequence in sequences)
    unstable = sum(1 for a, b in zip(cpp_sequences, cpp_sequences_again) if a != b)
    if unstable:
        failures.append(f"{unstable} of {len(sequences)} log sequences are not stable on the oracle")
    diverged_sequences = 0
    for index, (cpp_rows, rs_rows) in enumerate(zip(cpp_sequences, rs_sequences)):
        if cpp_rows == rs_rows:
            continue
        diverged_sequences += 1
        step_index = next(i for i, (a, b) in enumerate(zip(cpp_rows, rs_rows)) if a != b)
        failures.append(f"log sequence {index} step {step_index} differs")
        print(f"FAIL sequence {index} step {step_index}: {instrument.label(sequences[index][step_index])}")
        print(f"  cpp: {cpp_rows[step_index]}")
        print(f"  rs : {rs_rows[step_index]}")
    print(
        f"{'PASS' if not unstable and not diverged_sequences else 'FAIL'} "
        f"{len(sequences)} 段 / {sequence_steps} 步：unstable={unstable} diverged={diverged_sequences}"
    )

    print("\nper-action cases:")
    for name in targets:
        print(f"  {name}: {per_action.get(name, 0)}")
    print(
        f"\ncases={len(corpus)} sequences={len(sequences)}/{sequence_steps}steps diverged={differenced} "
        f"empty={empty} failures={len(failures)}"
    )
    for failure in failures[:20]:
        print(f"  - {failure}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))