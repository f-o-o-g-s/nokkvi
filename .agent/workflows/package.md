---
description: Bump version, build, commit, push, and tag a release
---

# Release a new version

// turbo-all

The release workflow (`.github/workflows/release.yml`) does the binary build and tarball packaging on tag push. This workflow is what gets you to that tag.

## 0. Bootstrap git hooks

Run this once per clone or worktree. It is idempotent — safe to re-run every release:

```bash
git config --local core.hooksPath .githooks
```

The pre-commit hook (`.githooks/pre-commit`) auto-updates Navidrome/PipeWire pins in `README.md` and refuses a minor/major `Cargo.toml` bump if the previous minor's entries are still in `CHANGELOG.md`. Without `core.hooksPath` set, both checks silently no-op and the bump can land unchecked — the CI gate in step 6 is the backstop, but it fails late (after the tag push). Bootstrap once and the hook catches the mistake locally instead.

## 1. Generate changelog from git history

Review commits since the last version bump:

```bash
last_version_commit=$(git log --oneline --all --grep='bump version' -1 --format='%H')
git log --oneline "${last_version_commit}..HEAD" --no-merges
```

Promote the existing `## [Unreleased]` section in `CHANGELOG.md` to `## vX.Y.Z — YYYY-MM-DD` and seed a fresh empty `## [Unreleased]` block above it (with empty `### Added` / `### Changed` / `### Fixed` / `### Removed` sub-headings ready to fill in).

**Style rubric** (matches the rmpc-style format the repo settled on):

- **Categories**: **Added** / **Changed** / **Fixed** / **Removed**. Omit any with no entries.
- **One bullet = one sentence = one user-visible effect.** Aim for ≤ 25 words. Lead with what the user perceives now ("Volume changes via the wheel now persist past the 500ms throttle.") and stop.
- **Root-cause prose stays in the commit body.** When a fix has an interesting "why" worth keeping (incidents, races, surprising mechanisms), put it in the commit message body — `git log` is the engineering record; CHANGELOG is the user-facing summary. If a single change needs more than one sentence to describe its effect, that usually means two changes — split into two bullets.
- **Frame by user-visible effect**, not internal mechanism. Skip internal type names, file paths, function names, PR numbers.
- **Drop internal-only churn**: CI, workflow, lockfile, dep bumps with no runtime effect, refactors with no behavior change. If a refactor produces a perceptible effect (memory, startup time, fewer crashes), record the effect, not the refactor.
- **Version-header format**: keep `## vX.Y.Z — YYYY-MM-DD` exact — `.github/workflows/release.yml`'s awk extractor matches on it character-by-character.

The release workflow extracts the section matching the pushed tag verbatim into the GitHub Release body — write for someone deciding whether to upgrade.

**Archive boundary**: only the current minor series (e.g. `0.4.x`) plus `## [Unreleased]` lives in `CHANGELOG.md`. Older minors are archived under `changelog-archive/CHANGELOG-X.Y.md` (e.g. `changelog-archive/CHANGELOG-0.3.md`). When promoting `## [Unreleased]` to a release, you only ever touch `CHANGELOG.md`; do not edit archive files. When a new minor opens (e.g. `0.4.x` → `0.5.x`), close out the old minor by moving its block into a new `changelog-archive/CHANGELOG-0.4.md` and refresh the "Older releases" footer link in `CHANGELOG.md`.

## 2. Update README.md

Read `README.md` and verify the feature list, dependencies, and build instructions match the current codebase. Update any stale sections. Don't touch screenshots or media.

**Logo assets:** if the logo artwork changed this release, regenerate the committed derived assets from the canonical master before tagging: `sh scripts/gen-logo-assets.sh <new-art.svg>` rebuilds `assets/logo/nokkvi_master.svg` and re-derives `assets/nokkvi_logo.svg`, `assets/nokkvi_logo_readme.svg`, `assets/org.nokkvi.nokkvi.svg`, and `assets/org.nokkvi.nokkvi.png` (pinned scour 0.38.2 / rsvg-convert 2.62.x). Stage the master plus all four derived outputs in the same commit, and run `sh scripts/gen-logo-assets.sh --check` to confirm they are in sync. The tarball and AUR builds consume these committed assets and never regenerate them (no rasterizer in their deps).

## 3. Bump version

**Default to a patch bump** regardless of whether the changes are fixes or features (e.g. `0.3.7` → `0.3.8`). Patch numbers are allowed to grow arbitrarily — `0.3.99`, `0.3.128` are all fine. Early-iteration honesty over semver purity.

Bump the **minor** (`0.3.x` → `0.4.0`) or **major** (`0.x.x` → `1.0.0`) **only when the user has explicitly authorized it in this conversation.** If you think the changeset warrants a minor bump, ask first — do not assume.

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

**Patch bump (default):** lightweight tag is fine.

```bash
git tag "vX.Y.Z" && git push origin "vX.Y.Z"
```

**Minor or major bump (only with explicit user authorization):** the tag must be annotated and the body must contain `allow-minor-bump: <reason>` or `allow-major-bump: <reason>`. The release workflow has a hard gate that fails the build otherwise.

```bash
# minor
git tag -a "vX.Y.Z" -m "allow-minor-bump: <reason from user>" && git push origin "vX.Y.Z"

# major
git tag -a "vX.Y.Z" -m "allow-major-bump: <reason from user>" && git push origin "vX.Y.Z"
```

If you pushed a non-conforming tag and the gate fails, recover with:

```bash
git push --delete origin "vX.Y.Z" && git tag -d "vX.Y.Z"
# then re-tag annotated and push again
```

The tag push fires `.github/workflows/release.yml`, which validates the bump policy, builds the x86_64 Linux binary, packages it into `nokkvi-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` with a `.sha256` companion, and creates a **draft** GitHub Release with the matching CHANGELOG section as the body.

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

## 8. Sync AUR packages

After the GitHub release is published, propagate the new version to both AUR packages.

**`nokkvi-bin`** — bump `pkgver`, refresh sha256 from the just-published `.sha256` artifact, regenerate `.SRCINFO`, push:

```bash
if [ -d ~/aur/nokkvi-bin/.git ]; then
    cd ~/aur/nokkvi-bin
    current=$(grep -oP '(?<=^pkgver=)\S+' PKGBUILD)
    if [ "$current" = "X.Y.Z" ]; then
        echo "nokkvi-bin already at vX.Y.Z; skipping (would otherwise regress pkgrel)"
    else
        sed -i "s/^pkgver=.*/pkgver=X.Y.Z/" PKGBUILD
        sed -i "s/^pkgrel=.*/pkgrel=1/" PKGBUILD
        updpkgsums                          # auto-fetches sha256 from the source URL
        makepkg --printsrcinfo > .SRCINFO
        git add PKGBUILD .SRCINFO
        git commit -m "Update to vX.Y.Z"
        git push
    fi
fi
```

The same-version guard matters because `pkgrel` is reset to `1` on every fresh `pkgver`. If Step 8 ran twice for the same release, the second run would push `pkgrel=1` over an already-incremented `pkgrel=N>1` (e.g. a packaging-only fix between releases), regressing the AUR's view of that release. The guard makes the step idempotent.

**`nokkvi-git`** — no `pkgver` bump needed (it's auto-derived from `git describe` at install time). Refresh `.SRCINFO` so the AUR's "Last Updated" timestamp reflects the new release:

**Before running this block:** if this release introduced a new `-sys` / build-script crate that pulls in a system build tool (`cmake`, `autoconf`, `pkg-config`, etc.), add it to `nokkvi-git/PKGBUILD`'s `makedepends` list manually first. `makepkg -od` skips the build stage, so this step won't catch a stale `makedepends` list on its own.

```bash
if [ -d ~/aur/nokkvi-git/.git ]; then
    cd ~/aur/nokkvi-git
    makepkg -od --noconfirm                 # download + extract + run pkgver() against new HEAD
    makepkg --printsrcinfo > .SRCINFO
    if ! git diff --quiet PKGBUILD .SRCINFO; then
        git add PKGBUILD .SRCINFO
        git commit -m "Sync with vX.Y.Z release"
        git push
    fi
fi
```

Both blocks are guarded — they no-op cleanly if the AUR repos aren't cloned locally (e.g. for contributors running `/package` who don't maintain AUR packages).

If either push fails due to remote-ahead (someone else pushed first), recover with `git pull --rebase && git push`.

## 9. Report

Print the GitHub release URL (`gh release view vX.Y.Z --json url -q .url`), the published artifact filenames, and the AUR package URLs (`https://aur.archlinux.org/packages/nokkvi-bin`, `https://aur.archlinux.org/packages/nokkvi-git`).
