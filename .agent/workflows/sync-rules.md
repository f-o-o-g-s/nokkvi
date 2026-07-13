---
description: Audit and update .claude/rules/, .claude/skills/, .agent/workflows/, and CLAUDE.md to match the current codebase
---

# Sync Agent Rules

Audit every `.claude/rules/*.md` rule, `.claude/skills/*/SKILL.md` skill, and `.agent/workflows/*.md` workflow against the live codebase and rewrite the stale ones. Then audit `CLAUDE.md` separately under a stricter rubric.

**Economics (this inverts the old rubric):** `.claude/rules/` files AUTO-INJECT — `code-standards.md` (no `paths:`) loads in every session; path-scoped rules inject on the first Read of a matching file, including in subagents. Verbosity is no longer free. Rules hold **invariants, contracts, and whys** — never enumerated variant lists, field catalogs, file inventories, or churning counts (in-code doc-comments are canonical for those). Treat re-introduction of a catalog into a rule file as a sync FAILURE: point the author at the code instead. `.agent/workflows/` files are read on demand by slash commands, so verbosity there is still acceptable when accuracy demands it.

## 1. Enumerate files and check staleness

// turbo
```bash
for f in $(find .claude/rules .claude/skills .claude/commands .agent/workflows -type f -name '*.md' | sort) CLAUDE.md; do
  last_commit=$(git log -1 --format='%H %ai %s' -- "$f")
  last_hash=$(echo "$last_commit" | awk '{print $1}')
  commits_since=$(git rev-list --count "${last_hash}..HEAD" 2>/dev/null || echo "N/A")
  echo "=== $f ==="
  echo "  Last modified: $last_commit"
  echo "  Commits since: $commits_since"
  echo ""
done
```

## 2. Pull commit summaries scoped to each stale file

// turbo
For files with meaningful drift, scope `git log` to the paths each rule documents:

```bash
git --no-pager log --oneline <last_hash>..HEAD -- <path1> <path2>
```

## 3. Verify claims against the codebase

For each stale file:
- Read the current contents
- Check the structures it documents (`ls` directories, `grep` for enums/structs/fields/constants)
- Note concrete discrepancies — wrong names, missing modules, renamed types, dropped features

## 4. Structural audits (every run, cheap)

// turbo
- **Glob liveness** — every `paths:` glob in `.claude/rules/*.md` frontmatter must still match at least one existing file; a stale glob is a silently dead rule:
  ```bash
  python3 - << 'EOF'
  import glob, re, sys, pathlib
  ok = True
  for rule in sorted(glob.glob('.claude/rules/*.md')):
      text = pathlib.Path(rule).read_text()
      m = re.match(r'^---\n(.*?)\n---', text, re.S)
      if not m: continue
      for g in re.findall(r'-\s*"([^"]+)"', m.group(1)):
          if not glob.glob(g, recursive=True):
              print(f'DEAD GLOB in {rule}: {g}'); ok = False
  sys.exit(0 if ok else 1)
  EOF
  ```
- **Anchor integrity** — source comments cite `gotchas.md` by SECTION NAME, never by line number. Every cited section/bullet must still exist verbatim:
  ```bash
  grep -rn "gotchas.md" src/ data/ | grep -o '"[^"]*"' | sort -u
  # assert each quoted name appears in .claude/rules/gotchas.md
  ```
- **No stale paths** — `git grep -n "\.agent/rules" -- ':!*sync-rules.md'` must return nothing (that directory is gone; rules live in `.claude/rules/`; the exclusion skips this procedure's own text).

## 5. Rewrite stale files

- **Accuracy** — every claim must match current code
- **Brevity** — rules auto-inject, so every line costs tokens in matching sessions; keep invariants, cut derivable content
- **Anchors win** — a bullet cited by a source comment survives any dedupe, with its section heading intact
- **Skip** files that are still accurate

## 6. Audit CLAUDE.md (different rubric — verify, don't expand)

CLAUDE.md is auto-injected every session. Treat it as an index + load-bearing rules, not a knowledge base.

Verify only:
- **Catalog accuracy** — the `.claude/rules/` description block and `.agent/workflows/` list point to files that still exist with descriptions that still match.
- **Architecture summaries** — the workspace table, TEA pattern, `AppService` tree, and `CustomAudioEngine` diagram still describe reality at a coarse level.
- **Inline gotchas** — each one still applies. Resolved gotchas get deleted, not updated. The inline "Gotchas (the silent ones)" block STAYS even though `gotchas.md` auto-injects — path-scoped rules are lost after /compact until a matching file is re-read; the inline block is the compaction-resilient safety net.
- **Commands** — build/test invocations, system deps still correct.
- **Conventions** — error/logging/cloning/threading/dependency rules still hold.

Hard rule: **do not expand CLAUDE.md.** New detail goes in a rule file; CLAUDE.md only points.

## 7. Commit

```bash
git add .claude/rules .claude/skills .claude/commands .agent/workflows CLAUDE.md && git commit -m "chore(rules): sync agent rules and workflows with current codebase"
```

Stage only the audited paths — never `git add .claude/` wholesale (it would catch `settings.local.json`). Omit untouched paths from the `git add`. End the commit message at the subject — no `Co-Authored-By` trailer. If nothing changed, skip the commit.
