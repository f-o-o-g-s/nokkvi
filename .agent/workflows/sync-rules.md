---
description: Audit and update .agent/rules/, .agent/workflows/, and CLAUDE.md to match the current codebase
---

# Sync Agent Rules

Audit every `.agent/rules/` and `.agent/workflows/` `.md` file against the live codebase and rewrite the stale ones. Then audit `CLAUDE.md` separately under a stricter rubric.

## 1. Enumerate files and check staleness

// turbo
```bash
for f in $(find .agent -type f -name '*.md' | sort) CLAUDE.md; do
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
- Note concrete discrepancies — wrong counts, missing modules, renamed types, dropped features

## 4. Rewrite stale rule/workflow files

Rule and workflow files are read on demand, so verbosity is acceptable when accuracy demands it.

- **Accuracy** — every claim must match current code
- **Brevity** — trim filler, prefer cross-references over duplication
- **Skip** files that are still accurate

## 5. Audit CLAUDE.md (different rubric — verify, don't expand)

CLAUDE.md is auto-injected every session, so every line costs tokens × every session. Treat it as an index + load-bearing rules, not a knowledge base.

Verify only:
- **Catalog accuracy** — the `.agent/rules/` and `.agent/workflows/` lists at the bottom point to files that still exist with descriptions that still match.
- **Architecture summaries** — the workspace table, TEA pattern, `AppService` tree, and `CustomAudioEngine` diagram still describe reality at a coarse level. Don't enumerate every service; capture shape.
- **Inline gotchas** — each one still applies. Resolved gotchas get deleted, not updated. Do not import new gotchas from `gotchas.md`; new detail goes in the rule file with a pointer (already present) from CLAUDE.md.
- **Commands** — `cargo build`, test invocations, system deps still correct.
- **Conventions** — error/logging/cloning/threading/dependency rules still hold.

Hard rule: **do not expand CLAUDE.md.** If new content seems necessary, put it in a rule file and rely on the existing catalog pointer. Net additions to CLAUDE.md require an explicit reason ("the existing pointer is insufficient because…").

## 6. Commit

```bash
git add .agent/ CLAUDE.md && git commit -m "chore(rules): sync agent rules and workflows with current codebase"
```

If only `.agent/` changed, omit `CLAUDE.md` from the `git add`. End the commit message at the subject — no `Co-Authored-By` trailer. Stage only the audited paths so unrelated working-tree changes are left alone. If nothing changed, skip the commit.
