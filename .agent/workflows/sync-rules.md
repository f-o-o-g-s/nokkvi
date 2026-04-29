---
description: Audit and update .agent/rules/ and .agent/workflows/ to match the current codebase
---

# Sync Agent Rules

Audit every `.agent/rules/` and `.agent/workflows/` `.md` file against the live codebase and rewrite the stale ones.

## 1. Enumerate files and check staleness

// turbo
```bash
for f in $(find .agent -type f -name '*.md' | sort); do
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

## 4. Rewrite stale files

- **Accuracy** — every claim must match current code
- **Brevity** — these files populate LLM context; trim filler, keep cross-references over duplication
- **Skip** files that are still accurate

## 5. Commit

```bash
git add .agent/ && git commit -m "chore(rules): sync agent rules and workflows with current codebase"
```

End the commit message at the subject — no `Co-Authored-By` trailer. Stage only `.agent/` paths so unrelated working-tree changes are left alone. If nothing changed, skip the commit.
