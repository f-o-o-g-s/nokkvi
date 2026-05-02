---
description: Refresh the [Unreleased] section of CHANGELOG.md from commits since the last release
argument-hint: [optional hint about scope or framing]
---

Propose entries for the `## [Unreleased]` block of `CHANGELOG.md` from commits made since the last release, show the diff, and apply on confirmation. Does not touch versioning, tags, README, or any release machinery — that's `/package`'s job.

## Steps

1. Find the boundary commit: `last_version_commit=$(git log --oneline --all --grep='bump version' -1 --format='%H')`. List commits since with `git log --no-merges --format='%h %s%n%b%n---' "${last_version_commit}..HEAD"`. Read bodies — subjects alone often miss the user-visible angle. If the range is empty, say so and stop.

2. Read the current `## [Unreleased]` block in `CHANGELOG.md`. Existing bullets are authoritative — do not rewrite, reorder, or merge them. Your job is to *add* what's missing.

3. Categorize each commit by conventional-commit type:
   - `feat:` → **Added**
   - `fix:` → **Fixed**
   - `refactor:` / `perf:` / UI-affecting `style:` → **Changed** *only if* the change is user-visible. Otherwise drop.
   - Removed features or `BREAKING CHANGE:` → **Removed**
   - `chore:` / `ci:` / `build:` / `test:` / `docs:` → drop unless the effect is user-visible (e.g. a runtime dep bump users will notice → **Changed**; a docs-only commit → drop, that's the docs site's concern).

4. Apply the style rubric from `.agent/workflows/package.md` § 1: one line per bullet, framed by user-visible effect, no internal type names / file paths / PR numbers, no CI/workflow/lockfile churn. Collapse multi-commit features into a single bullet.

5. Skip any commit whose user-visible effect is already covered by an existing bullet under `## [Unreleased]`. When in doubt, ask the user rather than duplicate.

6. Show a proposed diff of the `## [Unreleased]` block (existing bullets preserved verbatim + new bullets inserted under the right sub-heading, in commit order oldest→newest within each sub-heading). Wait for approval. Do **not** silently edit `CHANGELOG.md`.

7. On approval, edit `CHANGELOG.md` and stop. Do not stage, commit, bump versions, or touch any other file. The user runs `/commit` next if they want to land it.

## Arguments

If `$ARGUMENTS` is provided, treat it as guidance on framing, scope, or which commits to emphasize or skip. It is a hint, not a literal entry.
