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

Promote the existing `## [Unreleased]` section in `CHANGELOG.md` to `## vX.Y.Z — YYYY-MM-DD` and seed a fresh empty `## [Unreleased]` block above it (with empty `### Added` / `### Changed` / `### Fixed` / `### Removed` sub-headings ready to fill in).

**Style rubric** (matches the rmpc-style format the repo settled on):

- Categories: **Added** (new features), **Changed** (visible behavior changes that aren't fixes), **Fixed** (bug fixes), **Removed** (removed features). Omit any category with no entries.
- One bullet per change, one line if at all possible.
- Frame by user-visible effect, not internal mechanism. No internal type names, file paths, or PR numbers — those live in the commit body / git log.
- Drop CI, workflow, lockfile, and other internal-only churn entirely. If a CI change matters to users, it belongs under **Changed** phrased as user effect; otherwise let the commit message carry it.
- Keep the version-header format exactly `## vX.Y.Z — YYYY-MM-DD` — the release workflow's awk extractor matches on it.

The release workflow extracts the section matching the pushed tag verbatim into the GitHub Release body, so write it for end users.

## 2. Update README.md

Read `README.md` and verify the feature list, dependencies, and build instructions match the current codebase. Update any stale sections. Don't touch screenshots or media.

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
