---
description: Audit and update .agent/rules/, .agent/workflows/, and CLAUDE.md to match the current codebase
---

Follow the procedure in `.agent/workflows/sync-rules.md` exactly. Read that file first, then execute its steps in order:

1. Enumerate every `.md` file under `.agent/rules/` and `.agent/workflows/`, plus `CLAUDE.md`, and report each file's last-touch commit and how many commits have landed since.
2. For files with meaningful commit activity since their last update, pull the commit summaries scoped to the paths each rule documents.
3. For each stale file, read the current contents and verify claims against the live codebase (directory listings, struct/enum definitions, field lists, feature presence).
4. Rewrite stale rule/workflow files to match reality. Optimize for accuracy and conciseness — these files populate LLM context, so trim filler and cross-reference rather than duplicate.
5. Audit `CLAUDE.md` under a stricter rubric: it is auto-injected every session, so verify catalog accuracy, architecture summaries, inline gotchas, commands, and conventions — but do **not** expand it. New detail belongs in a rule file; CLAUDE.md only points. Resolved gotchas get deleted, not updated. Net additions require an explicit reason.
6. Skip files that are still accurate.
7. Stage and commit the updated files in one shot:

   ```bash
   git add .agent/ CLAUDE.md && git commit -m "chore(rules): sync agent rules and workflows with current codebase"
   ```

   If `CLAUDE.md` was untouched, omit it from the `git add`. Stage only the audited paths — leave any unrelated working-tree changes alone. End the commit message at the subject; no `Co-Authored-By` trailer. If nothing changed, skip the commit and say so.
