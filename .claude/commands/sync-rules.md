---
description: Audit and update .claude/rules/, .claude/skills/, .agent/workflows/, and CLAUDE.md to match the current codebase
---

Follow the procedure in `.agent/workflows/sync-rules.md` exactly. Read that file first, then execute its steps in order:

1. Enumerate every `.md` file under `.claude/rules/`, `.claude/skills/`, `.claude/commands/`, and `.agent/workflows/`, plus `CLAUDE.md`, and report each file's last-touch commit and how many commits have landed since.
2. For files with meaningful commit activity since their last update, pull the commit summaries scoped to the paths each rule documents.
3. For each stale file, read the current contents and verify claims against the live codebase (directory listings, struct/enum definitions, feature presence).
4. Run the structural audits from the workflow every time: every `paths:` glob in `.claude/rules/` frontmatter still matches at least one file (a stale glob is a silently dead rule); every source comment citing a `gotchas.md` section name still resolves; `git grep "\.agent/rules" -- ':!*sync-rules.md'` returns nothing.
5. Rewrite stale files. Rules AUTO-INJECT into matching sessions, so optimize hard for token economy: keep invariants, contracts, and whys; delete enumerated variant lists, field catalogs, and churning counts — in-code doc-comments are canonical for those. Bullets cited by source comments survive any cut (anchors win).
6. Audit `CLAUDE.md` under the stricter rubric in the workflow: verify, don't expand; the inline "Gotchas (the silent ones)" block stays (it is the post-/compact safety net).
7. Skip files that are still accurate.
8. Stage and commit only the audited paths in one shot:

   ```bash
   git add .claude/rules .claude/skills .claude/commands .agent/workflows CLAUDE.md && git commit -m "chore(rules): sync agent rules and workflows with current codebase"
   ```

   Never `git add .claude/` wholesale (it would catch `settings.local.json`). Omit untouched paths. End the commit message at the subject; no `Co-Authored-By` trailer. If nothing changed, skip the commit and say so.
