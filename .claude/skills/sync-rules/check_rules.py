#!/usr/bin/env python3
"""Structural checks for .claude/rules/: glob liveness, citation anchors, sizes.

Run from the repo root: `python3 .claude/skills/sync-rules/check_rules.py`.
Exits 1 on a dead glob, a broken citation, or a bare `gotchas.md` reference;
size findings are warnings only.
"""

import pathlib
import re
import subprocess
import sys

RULES = pathlib.Path(".claude/rules")
# Sizes are bytes. A path-scoped file costs this much in every session that
# touches one of its globs; the harness's own size warning only looks at the
# always-loaded files, so nothing else flags a rule file that keeps growing.
GOTCHAS_FILE_BUDGET = 10_000
TOPICAL_FILE_BUDGET = 50_000
ALWAYS_ON_BUDGET = 40_000

tracked = subprocess.run(
    ["git", "ls-files"], capture_output=True, text=True, check=True
).stdout.split()
failed = False


def fail(msg):
    global failed
    failed = True
    print(f"FAIL  {msg}")


def expand_braces(pattern):
    m = re.search(r"\{([^{}]*)\}", pattern)
    if not m:
        return [pattern]
    head, tail = pattern[: m.start()], pattern[m.end():]
    return [x for alt in m.group(1).split(",") for x in expand_braces(head + alt + tail)]


def glob_regex(pattern):
    out, i = "", 0
    while i < len(pattern):
        if pattern.startswith("**/", i):
            out, i = out + "(?:.*/)?", i + 3
        elif pattern.startswith("**", i):
            out, i = out + ".*", i + 2
        elif pattern[i] == "*":
            out, i = out + "[^/]*", i + 1
        elif pattern[i] == "?":
            out, i = out + "[^/]", i + 1
        else:
            out, i = out + re.escape(pattern[i]), i + 1
    return re.compile(f"^{out}$")


def frontmatter_paths(text):
    m = re.match(r"---\n(.*?)\n---\n", text, re.S)
    if not m:
        return None
    return [
        g.strip().strip("'\"")
        for g in re.findall(r"^\s*-\s*(.+?)\s*$", m.group(1), re.M)
    ]


def anchors(path):
    """Headings and bold spans of a markdown file, backticks stripped."""
    text = path.read_text()
    names = {h.strip() for h in re.findall(r"^#+\s+(.+)$", text, re.M)}
    names |= set(re.findall(r"\*\*(.+?)\*\*", text))
    return {n.replace("`", "") for n in names}


# 1. Glob liveness.
rule_files = sorted(RULES.rglob("*.md"))
always_on = [pathlib.Path("CLAUDE.md")]
for rule in rule_files:
    globs = frontmatter_paths(rule.read_text())
    if globs is None:
        always_on.append(rule)
        continue
    for g in globs:
        for exp in expand_braces(g):
            if not any(glob_regex(exp).match(f) for f in tracked):
                fail(f"dead glob in {rule}: {exp}" + (f" (from {g})" if exp != g else ""))

# 2. Citation anchors: `<file>.md "Name"` must name a heading or bold span in
# that file. Comment prefixes are folded away first so a citation that wraps
# onto the next comment line still parses.
cite = re.compile(r"((?:[\w-]+/)?[\w.-]+\.md)`?\)?,?\s+(?:§\s*)?\"([^\"]+)\"")
bare = re.compile(r"(?<![\w/.-])gotchas\.md")
sources = [
    f
    for f in tracked
    if f.endswith(".rs") or f == "CLAUDE.md" or f.startswith(".claude/")
]
sources += [str(p) for p in RULES.rglob("*.md") if str(p) not in sources]


def resolve(name, cited_from):
    if name == "CLAUDE.md":
        return pathlib.Path("CLAUDE.md")
    for cand in (RULES / name, pathlib.Path(cited_from).parent / name):
        if cand.is_file() and cand.resolve().is_relative_to(RULES.resolve()):
            return cand
    return None


checked = 0
for src in sources:
    if src == ".claude/skills/sync-rules/check_rules.py" or not src.endswith((".rs", ".md")):
        continue
    raw = pathlib.Path(src).read_text()
    if bare.search(raw) and src != ".claude/skills/sync-rules/SKILL.md":
        line = raw.count("\n", 0, bare.search(raw).start()) + 1
        fail(f"{src}:{line}: bare `gotchas.md` reference (the file is split into gotchas/<area>.md)")
    flat = re.sub(r"\n\s*(?://[!/]?|\*)?\s*", " ", raw)
    for m in cite.finditer(flat):
        target = resolve(m.group(1), src)
        if target is None:
            continue  # not a rule/CLAUDE.md citation (README.md, CHANGELOG.md, ...)
        checked += 1
        quoted = m.group(2).replace("`", "")
        body = target.read_text().replace("`", "").lower()
        named = any(a.startswith(quoted) for a in anchors(target))
        if not named and quoted.lower() not in body:
            fail(f"{src}: cites {m.group(1)} \"{m.group(2)}\", which matches no heading, bold span or text there")

print(f"citations checked: {checked}")

# 3. Sizes (warnings).
total_on = sum(p.stat().st_size for p in always_on)
print(f"always-on: {total_on} bytes ({', '.join(str(p) for p in always_on)})")
if total_on > ALWAYS_ON_BUDGET:
    print(f"WARN  always-on instructions exceed {ALWAYS_ON_BUDGET} bytes")
for rule in rule_files:
    size = rule.stat().st_size
    budget = GOTCHAS_FILE_BUDGET if rule.parent.name == "gotchas" else TOPICAL_FILE_BUDGET
    flag = "  WARN over budget" if size > budget and rule not in always_on else ""
    print(f"{size:7d}  {rule}{flag}")

sys.exit(1 if failed else 0)
