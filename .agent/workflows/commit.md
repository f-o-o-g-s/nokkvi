---
description: Commit session-authored files with a conventional message, refreshing the changelog in the same commit
---

// turbo-all

# Commit Workflow

The canonical procedure is the `/commit` slash command (`.claude/commands/commit.md`); this workflow summarizes it for agents committing outside that skill.

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

1. Inspect the working tree: `git -C /home/foogs/nokkvi status --short` and `git -C /home/foogs/nokkvi diff --stat`

2. Stage only the files you authored this session, explicitly by name: `git -C /home/foogs/nokkvi add <file1> <file2> ...`
   - Concurrent Claude sessions share this working tree, so leave everything else alone: pre-existing modifications, files the user or other agents changed, untracked files you didn't create
   - List the skipped dirty files in your response so the user knows they were left out on purpose

3. Review the staged diff to understand the changes: `git -C /home/foogs/nokkvi diff --cached`

4. Run the quality gate (fix any issues before committing):
   ```bash
   cargo +nightly fmt --all
   ```
   ```bash
   cargo clippy --all-targets -- -D warnings
   ```
   ```bash
   cargo test --workspace
   ```
   - `--workspace` is required: bare `cargo test` runs only the root crate and skips the `data` and `nokkvi-ipc` members. CI runs `cargo test --workspace` (`.github/workflows/ci.yml`)
   - If fmt reformatted any of your staged files, re-stage them by name
   - If clippy or tests fail, fix the issues and re-run before proceeding

5. Determine the commit type, scope, and description:
   - Analyze the diff for what changed
   - Pick the most appropriate type from the table above
   - If multiple types apply, use the most significant one (e.g. `feat` over `refactor`)
   - Keep the description lowercase, imperative mood, no period at end
   - If the change is breaking, add `!` after the scope: `type(scope)!: description`

6. For user-visible changes, refresh `## [Unreleased]` in `CHANGELOG.md` as part of the same commit:
   - Describe the change from the staged diff and your drafted message — it isn't in `git log` yet
   - Preserve existing bullets verbatim; only add what's missing, routed `feat` → **Added**, `fix` → **Fixed**, user-visible `refactor`/`perf`/`style` → **Changed**, removals/breaking → **Removed**
   - Follow the style rubric in `.agent/workflows/package.md` § 1: one bullet = one sentence = one user-visible effect, ≤ 25 words
   - Stage `CHANGELOG.md` with the rest of your set; internal-only changes (pure refactor, tests, CI, docs) leave it untouched

7. Commit directly: `git -C /home/foogs/nokkvi commit -m "type(scope): description"`
   - Add a body `-m "body text"` only if the change is complex or non-obvious; end the message at the body (no `Co-Authored-By` or other trailer)
   - Use plain `git commit`; leave `--no-verify`, `--amend`, and other rewrite flags out
   - Do NOT ask for approval — just commit

8. Expect the pre-commit hook (`.githooks/pre-commit`) to act during the commit:
   - It refreshes the Navidrome/PipeWire version pins in `README.md` and self-stages `README.md`, so the landed file set can grow by one file
   - It rejects (exit 1) a minor/major `Cargo.toml` version bump while `CHANGELOG.md` still contains the previous minor's entries — archive those to `changelog-archive/CHANGELOG-X.Y.md` first (see `.agent/workflows/package.md` § 1, "Archive boundary")

9. Confirm the result with `git -C /home/foogs/nokkvi show --stat HEAD`: report the commit hash and landed files, including `README.md`/`CHANGELOG.md` when the hook or step 6 touched them
