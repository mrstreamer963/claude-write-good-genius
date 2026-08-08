#!/usr/bin/env python3
"""Транскрипт сессии Claude Code (.jsonl) → Markdown.

Использование: python3 jsonl2md.py <файл.jsonl> [выход.md]
Кладёт реплики человека и ассистента; вызовы инструментов сворачивает в одну строку.
"""
import json
import sys
from pathlib import Path

src = Path(sys.argv[1])
dst = Path(sys.argv[2]) if len(sys.argv) > 2 else src.with_suffix(".md")

out = [f"# Диалог — {src.stem}\n"]
for line in src.read_text(encoding="utf-8").splitlines():
    try:
        rec = json.loads(line)
    except json.JSONDecodeError:
        continue
    msg = rec.get("message")
    if not isinstance(msg, dict) or rec.get("type") not in ("user", "assistant"):
        continue
    role = "Я" if msg.get("role") == "user" else "Claude"
    content = msg.get("content")
    if isinstance(content, str):
        parts = [content]
    else:
        parts = []
        for b in content or []:
            kind = b.get("type")
            if kind == "text":
                parts.append(b["text"])
            elif kind == "thinking":
                continue
            elif kind == "tool_use":
                parts.append(f"_[инструмент: {b.get('name')}]_")
            elif kind == "tool_result":
                parts.append("_[результат инструмента]_")
    body = "\n\n".join(p.strip() for p in parts if p and p.strip())
    if not body or body.startswith("<system-reminder>"):
        continue
    out.append(f"## {role}\n\n{body}\n")

dst.write_text("\n".join(out), encoding="utf-8")
print(f"{dst}  ({dst.stat().st_size} байт)")
