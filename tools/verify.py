"""Single gate for the Rust rewrite: build, differential harness, generated tables.

命令:
    python tools/verify.py            # 人读的 PASS/FAIL 汇总
    python tools/verify.py --json     # 同一结果的机器记录（写进冻结记录用）

判据:
    退出码 0 = 全部门禁全绿：`cargo build`（0 warning）、`tools/tdjson_diff.py`
    （failures=0）、`tools/tdjson_fuzz.py`（随机语料零分歧）、`tools/client_diff.py`
    （首启 lane 对齐）、六个生成器 `--check` 与仓库内容逐字节相同。

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

GATES = [
    (
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
    ),
    ("instrument", [PYTHON, "tools/tdjson_diff.py"]),
    ("fuzz", [PYTHON, "tools/tdjson_fuzz.py", "--seeds", "20261126,7,99"]),
    ("client_options", [PYTHON, "tools/gen_client_options.py", "--check"]),
    ("client_lane", [PYTHON, "tools/client_diff.py"]),
    ("tl_schema", [PYTHON, "tools/gen_tl_schema.py", "--check"]),
    ("mime_tables", [PYTHON, "tools/gen_mime_tables.py", "--check"]),
    ("emoji_table", [PYTHON, "tools/gen_emoji_table.py", "--check"]),
    ("country_table", [PYTHON, "tools/gen_country_table.py", "--check"]),
    ("ton_address", [PYTHON, "tools/gen_ton_address.py", "--check"]),
    ("api_functions", [PYTHON, "tools/gen_api_functions.py", "--check"]),
]


def run(name: str, command: list[str]) -> dict:
    env = dict(os.environ)
    env["PATH"] = UCrt + ";" + env.get("PATH", "")
    # TDLib 的日志会把请求里的原始字节写进 stderr（corpus 有非法 UTF-8 的 cell），
    # 所以按字节收再宽松解码，别让 gate 自己因为解码炸掉
    result = subprocess.run(command, cwd=ROOT, env=env, capture_output=True)
    output = ((result.stdout or b"") + (result.stderr or b"")).decode("utf-8", "replace")
    warnings = sum(1 for line in output.splitlines() if line.startswith("warning:"))
    return {
        "gate": name,
        "command": " ".join(command),
        "exit_code": result.returncode,
        "warnings": warnings,
        "tail": [line for line in output.strip().splitlines()[-3:]],
    }


def main(argv: list[str]) -> int:
    records = [run(name, command) for name, command in GATES]
    failures = [record for record in records if record["exit_code"] != 0]
    if "--json" in argv:
        print(json.dumps({"gates": records, "failures": len(failures)}, indent=2, ensure_ascii=False))
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