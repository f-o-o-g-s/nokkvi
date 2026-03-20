---
description: Bump version, build, commit, push, and package for distribution
---

# Package for Distribution

// turbo-all

## 1. Generate changelog from git history

Review commits since the last version bump to build an accurate changelog:

```bash
last_version_commit=$(git log --oneline --all --grep='bump version' -1 --format='%H')
git log --oneline "${last_version_commit}..HEAD" --no-merges
```

Categorize changes into: **Features**, **Fixes**, **Improvements**, **Internal**. Write a new section in `CHANGELOG.md` with the new version and date.

## 2. Update README.md

Read `README.md` and verify the feature list, dependencies, and build instructions match the current codebase. Update any stale sections. Don't touch screenshots or media.

## 3. Bump version

- Determine the new version number:
  - **Bugfix only** → bump patch (e.g. `0.4.0` → `0.4.1`)
  - **New features** → bump minor (e.g. `0.4.1` → `0.5.0`)
- Update `version = "X.Y.Z"` in `Cargo.toml` (root, first occurrence)

## 4. Run build CI

```bash
cargo +nightly fmt --all -- --check
```

```bash
cargo clippy
```

```bash
cargo test
```

```bash
cargo build --release
```

## 5. Commit and push

```bash
git add -A && git commit -m "chore: bump version to X.Y.Z, update changelog and readme"
```

```bash
git push
```

## 6. Package

```bash
./package.sh
```

## 7. Report

Print the final zip path and size, and confirm the version in `BUILD_INFO` matches.
