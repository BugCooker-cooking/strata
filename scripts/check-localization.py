#!/usr/bin/env python3
"""Check the macOS English catalog and its interpolation placeholders."""
import json
import re
from pathlib import Path

root = Path(__file__).resolve().parent.parent / "apps" / "macos" / "Strata"
catalog = json.loads((root / "English.json").read_text())
literal = re.compile(r'\bL10n\.(?:text|format)\("((?:[^"\\]|\\.)*)"')
placeholder = re.compile(r"\{[0-9]+\}")
issues = []
checked = set()
for path in sorted(root.rglob("*.swift")):
    source = path.read_text()
    for match in literal.finditer(source):
        key = json.loads('"' + match[1] + '"')
        checked.add(key)
        translated = catalog.get(key, "")
        if not translated or sorted(placeholder.findall(key)) != sorted(placeholder.findall(translated)):
            line = source.count("\n", 0, match.start()) + 1
            issues.append(f"{path.relative_to(root)}:{line}: missing or invalid translation for {key!r}")
if issues:
    raise SystemExit("\n".join(issues))
print(f"Checked {len(checked)} translated macOS interface strings.")
