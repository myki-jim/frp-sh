#!/usr/bin/env python3
"""移除 web/docs 下所有 markdown 中的 emoji 字符，保留 → ← ↔ 等文本符号。"""
import os
import re

ROOT = r"C:\Users\15447\Desktop\harness\frpsh\web\docs"

# 需要移除的 unicode 区间（emoji 块）：
# 1F000-1FAFF: 各类 emoji 符号块
# FE0F: 变体选择符（emoji 修饰）
# 200D: ZWJ
# 2600-27BF: 杂项符号/dingbat（含 ✅⚠️⚡ 等，但保留 →←↑↓ 等箭头在 2190-21FF，不在此区间）
# 2B00-2BFF: 杂项符号（如 ⭐ 星形）
EMOJI_RE = re.compile(
    "[\U0001F000-\U0001FAFF\U00002600-\U000027BF\U00002B00-\U00002BFF\U0000FE0F\U0000200D]"
)

# 特殊替换：任务列表风格（roadmap 的 ✅）
CHECK_RE = re.compile(r"^- ✅\s*", re.MULTILINE)

total_removed = 0
for root, dirs, files in os.walk(ROOT):
    for name in files:
        if not name.endswith(".md"):
            continue
        path = os.path.join(root, name)
        with open(path, encoding="utf-8") as f:
            text = f.read()
        removed = len(EMOJI_RE.findall(text))
        new_text = EMOJI_RE.sub("", text)
        new_text = CHECK_RE.sub("- [x] ", new_text)
        # 清理 emoji 移除后留下的多余空格
        new_text = new_text.replace(">  ", "> ").replace("|  ", "| ").replace("  |", " |")
        if new_text != text:
            with open(path, "w", encoding="utf-8", newline="\n") as f:
                f.write(new_text)
        total_removed += removed
        if removed:
            print(f"{path}: removed {removed} emoji chars")
print("total removed:", total_removed)
