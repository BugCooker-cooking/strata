#!/usr/bin/env python3
"""Track every inventory row; implementation and acceptance are separate states."""
import argparse
import hashlib
import json
import re
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "docs/research/photoshop-feature-inventory.md"
STATE = ROOT / "docs/research/photoshop-feature-status.json"
OUTPUT = ROOT / "docs/plans/photoshop-feature-coverage.md"


def inventory():
    section = "0"
    title = ""
    for number, line in enumerate(SOURCE.read_text().splitlines(), 1):
        heading = re.match(r"## (\d+)\.\s+(.*)", line)
        if heading:
            section, title = heading.groups()
        if not line.startswith("|") or int(section) > 19:
            continue
        cells = [cell.strip() for cell in line.strip("|").split("|")]
        priority = next((cell for cell in cells if re.fullmatch(r"P[0-3]|✗", cell)), None)
        if priority is None:
            continue
        name = cells[0]
        identity = f"PS-{int(section):02d}-{hashlib.sha256(name.encode()).hexdigest()[:10]}"
        yield {"id": identity, "section": int(section), "category": title,
               "name": name, "priority": priority, "source_line": number}


def render():
    states = json.loads(STATE.read_text()) if STATE.exists() else {}
    rows = list(inventory())
    unknown = set(states) - {row["id"] for row in rows}
    if unknown:
        raise SystemExit(f"Status refers to unknown inventory rows: {sorted(unknown)}")
    counts = Counter()
    lines = [
        "# Photoshop 功能覆盖跟踪", "",
        "由 scripts/feature-coverage.py 从完整功能清单和独立证据状态生成。"
        "一个条目可能包含多个动作；全部动作达到标准才算验收。"
        "优先级决定顺序，不代表本任务可以省略后续功能。", "",
        "旧清单的 ✗ 项保持“重新核对”，包含历史移除和外部服务依赖；"
        "没有自动当作已完成。实现记录与验收记录独立。", "",
        "| ID | 类别 | 功能 | 顺序 | 实现 | 验收/证据 |", "|---|---|---|---|---|---|"
    ]
    for row in rows:
        state = states.get(row["id"], {})
        implementation = state.get("implementation", "重新核对" if row["priority"] == "✗" else "待实现")
        acceptance = state.get("acceptance", "待验收")
        counts[implementation] += 1
        links = " · ".join(f"[{item['label']}]({item['path']})" for item in state.get("evidence", []))
        name = row["name"].replace("|", r"\|")
        category = row["category"].partition(chr(96))[0].strip()
        lines.append(f"| {row['id']} | {category} | {name} | {row['priority']} | {implementation} | {acceptance}{' · ' + links if links else ''} |")
    lines.extend(["", f"综合功能条目：{len(rows)}。实现状态：" + "；".join(f"{name} {count}" for name, count in sorted(counts.items())) + "。", "",
                  "源清单行号可由 --json 查询。该数量是综合条目数，不能当作独立子功能总数或完成百分比。", ""])
    return rows, "\n".join(lines)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    rows, text = render()
    if args.json:
        print(json.dumps(rows, ensure_ascii=False, indent=2))
    elif args.check:
        if not OUTPUT.exists() or OUTPUT.read_text() != text:
            raise SystemExit("Coverage document is stale; run python3 scripts/feature-coverage.py")
    else:
        OUTPUT.write_text(text)
        print(f"Tracked {len(rows)} composite inventory rows.")


if __name__ == "__main__":
    main()
