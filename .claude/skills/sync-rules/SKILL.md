---
description: Audit and update .claude/rules/, .claude/skills/, and CLAUDE.md to match the current codebase
---

# Sync Agent Rules

Audit every `.claude/rules/**/*.md` rule and every `.md` file under `.claude/skills/` (each `SKILL.md` plus its supporting files) against the live codebase and rewrite the stale ones. Then audit `CLAUDE.md` separately under a stricter rubric.

**Economics (this inverts the old rubric):** `.claude/rules/**/*.md` files AUTO-INJECT. `code-standards.md` (no `paths:`) loads in every session, like CLAUDE.md. A path-scoped rule injects once per session on the first Read/Edit/Write (or single-file `cat`/`head`) of a file matching its globs, in subagents too; after /compact it returns only once a matching file is touched again. The built-in Explore and Plan agents skip CLAUDE.md and `code-standards.md` but still get path-scoped rules. Claude Code's own size warning only measures the always-loaded files, so nothing else flags a path-scoped file that keeps growing: `check_rules.py` (below) reports sizes against a budget. Verbosity is no longer free. Rules hold **invariants, contracts, and whys** — never enumerated variant lists, field catalogs, file inventories, or churning counts (in-code doc-comments are canonical for those). Treat re-introduction of a catalog into a rule file as a sync FAILURE: point the author at the code instead. Skill **bodies** and supporting files load only when the skill runs, so verbosity there is still acceptable when accuracy demands it — but every model-invocable skill's `description` frontmatter loads in every session, so keep descriptions to one or two sentences.

## 1. Enumerate files and check staleness

```bash
for f in $(find .claude/rules .claude/skills -type f -name '*.md' | sort) CLAUDE.md; do
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

- **Rules checker** — `python3 .claude/skills/sync-rules/check_rules.py` must exit 0. It fails on a `paths:` glob (brace groups expanded) that matches no tracked file, which is a silently dead rule; on a citation `<file>.md "Name"` in a rule, skill, CLAUDE.md or source comment (wrapped comment lines included) whose quoted name matches no heading, bold span or text in the file it names; and on any bare `gotchas.md` reference. It also prints each rule file's size against its budget (10 KB per `gotchas/` file, 50 KB per topical file, 40 KB always-on). Source comments cite rules by SECTION or bullet NAME, never by line number.
- **No stale paths** — `git grep -nE '\.agent/|\.claude/commands' -- ':!.claude/skills/sync-rules/' ':!CHANGELOG.md' ':!changelog-archive/'` must return nothing. Both directories are gone: rules live in `.claude/rules/`, and procedures (the old commands + workflows) live as skills in `.claude/skills/`. The exclusions skip this procedure's own text and the immutable release history.

## 5. Rewrite stale files

- **Accuracy** — every claim must match current code
- **Brevity** — rules auto-inject, so every line costs tokens in matching sessions; keep invariants, cut derivable content
- **One home per fact** — a fact CLAUDE.md or `code-standards.md` already states (always loaded) gets no path-scoped copy; between two path-scoped files, keep it in the one whose globs cover the code it guards
- **Gotchas placement** — a gotcha lives in the `gotchas/<area>.md` whose `paths:` cover the files where it can be violated: the code it names, its callers, and where new code of that kind gets written. Widen that file's globs when a new site appears; never fall back to a catch-all like `src/**`. A file past its budget splits by area
- **Anchors win** — a bullet cited by a source comment survives any dedupe, with its section heading intact
- **Skip** files that are still accurate

## 6. Audit CLAUDE.md (different rubric — verify, don't expand)

CLAUDE.md is auto-injected every session. Treat it as an index + load-bearing rules, not a knowledge base.

Verify only:
- **Catalog accuracy** — the `.claude/rules/` description block and the skills list point to files that still exist with descriptions that still match.
- **Architecture summaries** — the workspace table, TEA pattern, `AppService` tree, and `CustomAudioEngine` diagram still describe reality at a coarse level.
- **Inline gotchas** — each one still applies. Resolved gotchas get deleted, not updated. The inline "Gotchas (the silent ones)" block STAYS even though the `gotchas/` files auto-inject — path-scoped rules are lost after /compact until a matching file is re-read; the inline block is the compaction-resilient safety net.
- **Commands** — build/test invocations, system deps still correct.
- **Conventions** — error/logging/cloning/threading/dependency rules still hold.

Hard rule: **do not expand CLAUDE.md.** New detail goes in a rule file; CLAUDE.md only points.

## 7. Commit

```bash
git add .claude/rules .claude/skills CLAUDE.md && git commit -m "chore(rules): sync agent rules and skills with current codebase"
```

Stage only the audited paths — never `git add .claude/` wholesale (it would catch the tracked `.claude/settings.json`). Omit untouched paths from the `git add`. End the commit message at the subject. If nothing changed, skip the commit and say so.
