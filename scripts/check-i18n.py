#!/usr/bin/env python3
"""Catalog checks for CI: every language has exactly the keys of en.json,
placeholders survive translation, and the bar widget catalogs agree too."""
import glob, json, os, re, sys

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
failed = False

def check(folder):
    global failed
    en = json.load(open(os.path.join(folder, "en.json"), encoding="utf-8"))
    for path in sorted(glob.glob(os.path.join(folder, "*.json"))):
        code = os.path.basename(path)[:-5]
        cat = json.load(open(path, encoding="utf-8"))
        missing, extra = sorted(set(en) - set(cat)), sorted(set(cat) - set(en))
        bad = [k for k in cat if k in en and set(re.findall(r"\{\w+\}", k)) != set(re.findall(r"\{\w+\}", cat[k]))]
        empty = [k for k, v in cat.items() if not str(v).strip() and k.strip()]
        if missing or extra or bad or empty:
            failed = True
            print(f"{os.path.relpath(path, root)}: {len(missing)} missing, {len(extra)} extra, {len(bad)} placeholder mismatches, {len(empty)} empty")
            for label, items in (("missing", missing), ("extra", extra), ("placeholders", bad), ("empty", empty)):
                for k in items[:10]:
                    print(f"  {label}: {k!r}")
        else:
            print(f"{os.path.relpath(path, root)}: ok ({len(cat)} strings, {code})")

check(os.path.join(root, "data", "i18n"))
check(os.path.join(root, "plugin", "i18n"))
sys.exit(1 if failed else 0)
