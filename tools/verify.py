"""Single gate for the Rust rewrite: build, differential harness, generated tables.

命令:
    python tools/verify.py                # 人读的 PASS/FAIL 汇总
    python tools/verify.py --json         # 同一结果 + 基座自证（commit / worktree / oracle md5）
    python tools/verify.py --list         # 只打印门禁名，一行一个
    python tools/verify.py --only a,b     # 只跑指定门禁（单 lane 迭代用；与 --json 同用时是局部快照）

判据:
    退出码 0 = 全部门禁全绿。门禁 = `build` + `tools/*_diff.py`（**自动发现**：每个脚本
    默认跑 `--selftest`，`tdjson_diff.py` / `client_diff.py` / `binlog_diff.py` 三个例外
    写在 `DIFF_OVERRIDES` 里）+ `tdjson_fuzz.py` + 七个 `gen_*.py --check`。加一条 lane
    只需新增 `tools/<name>_diff.py`，不用改这个文件；名单以 `--list` 为准。

覆盖边界:
    覆盖 `tdjson` C ABI 的 `td_execute` 面（29 个同步 action + 全部 1114 个函数的参数解析，
    含 `getLanguagePackString` 的成功路径）、客户端 lane（一个 client、固定
    `setTdlibParameters`：首次运行逐行、同一 `database_directory` 的第二次运行按集合、
    `waitPhoneNumber` 下的未授权状态机逐行）与生成表的一致性。
    覆盖不到：第二次运行的 option 顺序（不可判，F078）、真实登录（需实时码 + 2FA，F079）、
    非空号码的 `setAuthenticationPhoneNumber`、8 个有状态日志 action 的随机输入（它们只能
    按有序序列判，见 SEQUENCE 段）。

失效条件:
    oracle 换了构建（md5 变）、Rust 换了 target/linker、或 `td/` 的 schema/数据表更新——
    任一条都要重判 gate 并更新 docs/facts.md。
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
UCrt = "C:/msys64/ucrt64/bin"
PYTHON = sys.executable

# 门禁 = build + `tools/*_diff.py` 自动发现 + fuzz + 七个生成器。
# 加一条 lane 只需新增 `tools/<name>_diff.py`（默认带 `--selftest`），不用改这个文件。

BUILD_GATE = (
    "build",
    [
        "cargo",
        "build",
        "--release",
        "--target",
        "x86_64-pc-windows-gnu",
        "--manifest-path",
        "rust/Cargo.toml",
    ],
)

# 与 lane 同形、但参数不是「--selftest」的三个 diff 脚本
DIFF_OVERRIDES = {
    "tdjson_diff.py": ("instrument", []),
    "client_diff.py": ("client_lane", []),
    "binlog_diff.py": ("binlog", ["--all", "--selftest"]),
}

TAIL_GATES = [
    ("fuzz", [PYTHON, "tools/tdjson_fuzz.py", "--seeds", "20261126,7,99,2027"]),
    ("client_options", [PYTHON, "tools/gen_client_options.py", "--check"]),
    ("tl_schema", [PYTHON, "tools/gen_tl_schema.py", "--check"]),
    ("mime_tables", [PYTHON, "tools/gen_mime_tables.py", "--check"]),
    ("emoji_table", [PYTHON, "tools/gen_emoji_table.py", "--check"]),
    ("country_table", [PYTHON, "tools/gen_country_table.py", "--check"]),
    ("ton_address", [PYTHON, "tools/gen_ton_address.py", "--check"]),
    ("api_functions", [PYTHON, "tools/gen_api_functions.py", "--check"]),
]


def discover_diffs() -> list[tuple[str, list[str]]]:
    """`tools/*_diff.py` 全部入列：门禁名取自文件名，默认参数 `--selftest`。"""
    gates = []
    for path in sorted((ROOT / "tools").glob("*_diff.py")):
        name, args = DIFF_OVERRIDES.get(path.name, (path.name[: -len("_diff.py")], ["--selftest"]))
        gates.append((name, [PYTHON, f"tools/{path.name}", *args]))
    return gates


GATES = [BUILD_GATE, *discover_diffs(), *TAIL_GATES]


def run(name: str, command: list[str]) -> dict:
    env = dict(os.environ)
    env["PATH"] = UCrt + ";" + env.get("PATH", "")
    # TDLib 的日志会把请求里的原始字节写进 stderr（corpus 有非法 UTF-8 的 cell），
    # 所以按字节收再宽松解码，别让 gate 自己因为解码炸掉
    try:
        result = subprocess.run(command, cwd=ROOT, env=env, capture_output=True)
    except OSError as error:
        # 命令本身不在 PATH 上（典型：shell 里没有 cargo）时给一条 FAIL，别让整个门禁崩在这里
        return {
            "gate": name,
            "command": " ".join(command),
            "exit_code": 127,
            "warnings": 0,
            "tail": [f"{type(error).__name__}: {error}"],
        }
    output = ((result.stdout or b"") + (result.stderr or b"")).decode("utf-8", "replace")
    warnings = sum(1 for line in output.splitlines() if line.startswith("warning:"))
    return {
        "gate": name,
        "command": " ".join(command),
        "exit_code": result.returncode,
        "warnings": warnings,
        "tail": [line for line in output.strip().splitlines()[-3:]],
    }


def snapshot_meta() -> dict:
    """快照自证：这批 gate 跑在哪个 commit、哪个 oracle 二进制上。"""
    sys.path.insert(0, str(ROOT / "tools"))
    import tdjson_diff  # oracle 路径与 md5 pin 只此一份

    def git(*args: str) -> str | None:
        try:
            done = subprocess.run(
                ["git", *args], cwd=ROOT, capture_output=True, encoding="utf-8", errors="replace"
            )
        except OSError:
            return None
        return done.stdout.strip() if done.returncode == 0 else None

    oracle = Path(tdjson_diff.CPP)
    return {
        "commit": git("rev-parse", "HEAD"),
        # 工作树脏时 commit 不代表被测源码，快照不能自证
        "worktree_dirty": bool(git("status", "--porcelain")),
        "cpp_oracle": str(oracle),
        "cpp_oracle_md5": tdjson_diff.md5(oracle) if oracle.is_file() else None,
        "cpp_oracle_expected_md5": tdjson_diff.EXPECTED_CPP_MD5,
    }


def parse_selection(argv: list[str]) -> tuple[list[str], list[str]]:
    only: list[str] = []
    flags: list[str] = []
    index = 0
    while index < len(argv):
        if argv[index] == "--only":
            if index + 1 >= len(argv):
                raise SystemExit("--only 需要逗号分隔的门禁名")
            only.extend(name for name in argv[index + 1].split(",") if name)
            index += 2
        else:
            flags.append(argv[index])
            index += 1
    return only, flags


def main(argv: list[str]) -> int:
    only, flags = parse_selection(argv)
    if "--list" in flags:
        for name, _ in GATES:
            print(name)
        return 0
    selected = GATES
    if only:
        known = {name for name, _ in GATES}
        unknown = [name for name in only if name not in known]
        if unknown:
            print(f"unknown gate(s): {', '.join(unknown)}（已知门禁见 --list）")
            return 2
        wanted = set(only)
        selected = [(name, command) for name, command in GATES if name in wanted]
    records = [run(name, command) for name, command in selected]
    failures = [record for record in records if record["exit_code"] != 0]
    if "--json" in flags:
        snapshot = snapshot_meta()
        snapshot["gates"] = records
        snapshot["failures"] = len(failures)
        print(json.dumps(snapshot, indent=2, ensure_ascii=False))
    else:
        for record in records:
            mark = "PASS" if record["exit_code"] == 0 else "FAIL"
            extra = f" warnings={record['warnings']}" if record["gate"] == "build" else ""
            print(f"{mark} {record['gate']}{extra}")
            if record["exit_code"] != 0:
                for line in record["tail"]:
                    print(f"    {line}")
        print(f"\nfailures={len(failures)}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))