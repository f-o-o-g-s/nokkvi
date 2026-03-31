---
description: Audit and update .agent/rules/ and .agent/workflows/ to match the current codebase
---

# Sync Agent Rules

Audit all `.agent/rules/` and `.agent/workflows/` files against the current codebase and update any that are stale.

## 1. Enumerate files and check staleness

// turbo
For each `.md` file in `.agent/rules/` and `.agent/workflows/`, run:

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

## 2. Get commit history since each stale file

// turbo
For files with significant commits since their last update, get the commit summaries:

```bash
git --no-pager log --oneline <last_hash>..HEAD
```

Focus on commits that touch areas each file documents (use `-- <path>` filters as needed).

## 3. Read each stale file and research the codebase

For each file that needs updating:
- Read the current file contents
- Check the actual codebase structures it documents (list directories, grep for enums/structs/patterns)
- Identify concrete discrepancies: wrong counts, missing files/modules, outdated field lists, missing features

## 4. Update stale files

Rewrite each stale file with accurate content. Follow these principles:
- **Accuracy over completeness** — every claim must match the code
- **Conciseness** — these files populate LLM context; trim verbose explanations, keep actionable information
- **No redundancy** — don't duplicate information already in other rule files; cross-reference instead
- Skip files that are still accurate (e.g., simple workflow steps that haven't changed)

## 5. Commit

```bash
git add .agent/ && git commit -m "chore(rules): sync agent rules and workflows with current codebase"
```
