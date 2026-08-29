#!/usr/bin/env python3
"""把 docs/src/*.md 复制为 VitePress 中文内容（根区域），mermaid 代码块原样保留。"""
import os
import shutil

SRC = r"C:\Users\15447\Desktop\harness\frpsh\docs\src"
DST = r"C:\Users\15447\Desktop\harness\frpsh\web\docs"

CHAPTERS = [
    "intro.md", "quickstart.md", "install.md", "server.md", "cli.md",
    "config.md", "architecture.md", "advanced.md", "faq.md",
    "protocol.md", "develop.md", "roadmap.md",
]

os.makedirs(DST, exist_ok=True)
for name in CHAPTERS:
    shutil.copyfile(os.path.join(SRC, name), os.path.join(DST, name))
    print("copied", name)
print("zh content copied")
