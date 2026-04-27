---
description: Bump version, build, commit, push, and tag a release
---

# Release a new version

// turbo-all

The release workflow (`.github/workflows/release.yml`) does the binary build and tarball packaging on tag push. This workflow is what gets you to that tag.

## 1. Generate changelog from git history

Review commits since the last version bump:

```bash
last_version_commit=$(git log --oneline --all --grep='bump version' -1 --format='%H')
git log --oneline "${last_version_commit}..HEAD" --no-merges
```

Categorize changes into: **Features**, **Fixes**, **Improvements**, **Internal**. Promote the existing `## [Unreleased]` section in `CHANGELOG.md` to `## vX.Y.Z — YYYY-MM-DD` and seed a fresh empty `## [Unreleased]` block above it.

The release workflow extracts the section matching the pushed tag verbatim into the GitHub Release body, so write it for that audience.

## 2. Update README.md

Read `README.md` and verify the feature list, dependencies, and build instructions match the current codebase. Update any stale sections. Don't touch screenshots or media.

## 3. Bump version

Determine the new version number:
- **Bugfix only** → bump patch (e.g. `0.4.0` → `0.4.1`)
- **New features** → bump minor (e.g. `0.4.1` → `0.5.0`)

Update `version = "X.Y.Z"` in `Cargo.toml` (root, first occurrence under `[package]`).

## 4. Run the CI gate locally

```bash
cargo +nightly fmt --all -- --check
```

```bash
cargo clippy --all-targets -- -D warnings
```

```bash
cargo test
```

```bash
cargo build --release
```

Fix anything that fails before continuing.

## 5. Commit and push

```bash
git add -A && git commit -m "chore: bump version to X.Y.Z, update changelog and readme"
```

```bash
git push
```

## 6. Tag and trigger the release workflow

```bash
git tag "vX.Y.Z" && git push origin "vX.Y.Z"
```

The tag push fires `.github/workflows/release.yml`, which builds the x86_64 Linux binary, packages it into `nokkvi-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` with a `.sha256` companion, and creates a **draft** GitHub Release with the matching CHANGELOG section as the body.

## 7. Watch the workflow and publish the draft

Watch the run until it succeeds:

```bash
gh run watch --exit-status
```

Open the draft release and review:

```bash
gh release view "vX.Y.Z" --web
```

Confirm the tarball + sha256 are attached and the release notes match the CHANGELOG section. When everything looks right, publish:

```bash
gh release edit "vX.Y.Z" --draft=false
```

## 8. Report

Print the release URL (`gh release view vX.Y.Z --json url -q .url`) and the published artifact filenames.
