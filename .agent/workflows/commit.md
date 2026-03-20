---
description: Stage, analyze, and commit changes using conventional commits
---

// turbo-all

# /commit Workflow

## Conventional Commit Format

```
type(scope): description
```

### Types

| Type | When |
|------|------|
| `feat` | New feature or capability |
| `fix` | Bug fix |
| `refactor` | Code restructuring (no behavior change) |
| `perf` | Performance improvement |
| `style` | Formatting, whitespace, cosmetic (no logic change) |
| `chore` | Build scripts, config, deps, tooling |
| `docs` | Documentation only |
| `test` | Adding or updating tests |
| `ci` | CI/CD changes |

### Scope

Use the primary area affected: `audio`, `queue`, `ui`, `api`, `settings`, `theme`, `visualizer`, `playback`, `scrobble`, `widgets`, `views`, `hotkeys`, `mpris`, `artwork`, `deps`, etc. Omit scope if the change is truly cross-cutting.

## Steps

1. Check for staged changes: `git -C /home/foogs/nokkvi diff --cached --stat`

2. If nothing is staged, show unstaged changes: `git -C /home/foogs/nokkvi diff --stat` and `git -C /home/foogs/nokkvi status --short`
   - Stage everything with `git -C /home/foogs/nokkvi add -A` unless the unstaged changes clearly contain unrelated work, in which case ask the user what to stage.

3. Review the staged diff to understand the changes: `git -C /home/foogs/nokkvi diff --cached`

4. Determine the commit type, scope, and description:
   - Analyze the diff for what changed
   - Pick the most appropriate type from the table above
   - If multiple types apply, use the most significant one (e.g. `feat` over `refactor`)
   - Keep the description lowercase, imperative mood, no period at end
   - If the change is breaking, add `!` after the scope: `type(scope)!: description`

5. Commit directly: `git -C /home/foogs/nokkvi commit -m "type(scope): description"`
   - Add a body `-m "body text"` only if the change is complex or non-obvious
   - Do NOT ask for approval — just commit
