---
description: Audit and update .agent/rules/ and .agent/workflows/ to match the current codebase
---

Follow the procedure in `.agent/workflows/sync-rules.md` exactly. Read that file first, then execute its steps in order:

1. Enumerate every `.md` file under `.agent/rules/` and `.agent/workflows/` and report each file's last-touch commit and how many commits have landed since.
2. For files with meaningful commit activity since their last update, pull the commit summaries scoped to the paths each rule documents.
3. For each stale file, read the current contents and verify claims against the live codebase (directory listings, struct/enum definitions, field lists, feature presence).
4. Rewrite stale files to match reality. Optimize for accuracy and conciseness — these files populate LLM context, so trim filler and cross-reference rather than duplicate.
5. Skip files that are still accurate.
6. Stage and commit the updated rule/workflow files in one shot:

   ```bash
   git add .agent/ && git commit -m "chore(rules): sync agent rules and workflows with current codebase"
   ```

   Commit only `.agent/` paths — leave any unrelated working-tree changes alone. End the commit message at the subject; no `Co-Authored-By` trailer. If nothing changed, skip the commit and say so.
