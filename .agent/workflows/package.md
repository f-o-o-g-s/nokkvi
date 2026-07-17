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

**Style rubric** — the canonical changelog style for the repo (rmpc-style); `/commit` and `/unreleased` keep a one-line summary and defer here for the full rules + example:

- **Categories**: **Added** / **Changed** / **Fixed** / **Removed**. Omit any with no entries.
- **≤ 25 words — a hard cap, no exception, no band.** The cap does not yield for scene work, for narrative or aesthetic features, or for a change the author finds unusually characterful. **Importance buys a feature more bullets, never longer ones.** Lead with what the user perceives now ("Volume changes via the wheel now persist past the 500ms throttle.") and stop.
- **One claim per bullet — binding, not advisory.** A claim is one assertion the reader can confirm or refute against one lane of code. Two draw blocks with independent constants are two claims, even when they land in the same corner and read as one idea. Where a feature has N perceivable effects, it gets N adjacent bullets. The word count is a floor on auditability, not a guarantee of it: a bullet braiding four claims is unauditable even at 24 words. A bullet that resists splitting is not coherent, it is unaudited — split it and see which half was false. **Do not decide where a bullet ends by asking whether the reader perceives "one thing"** — that judgment is made in the same chair where two clauses with different truth conditions get read as one idea, and it is how *"the moon (and its day sun) now rests as a plain disc"* shipped (one true clause about the moon laundering a false one about the twelve-ray sun). Different gate, lane, or before-state means a different bullet.
- **One sentence, full stop.** A second sentence or a semicolon means two things — split or cut.
- **Cut clauses that EXPLAIN; keep clauses that BOUND.** The connective never decides it — a "because / since / so that / which means" clause is *usually* mechanism (commit-body material), but the test is the **over-promise test**, run before cutting any clause:
  > Read the bullet with the clause removed. **Does the sentence now promise more than the user will actually perceive?** No → the clause was explaining; cut it. Yes → the clause is doing effect work; do not cut it *and* do not keep it — rewrite the sentence so the bound is unnecessary.

  Existence is not the question; **perception is**. A draw call can exist (the moonbeam shafts really are drawn) while the user who resolves it does not (their alphas are pinned under the banding floor on purpose).
- **Failing the test means claim LESS, never say more.** The bound is almost always a single verb or adjective, and those are free. Restraint bounds in one word: an *invisible* black hole, *untranslated* verses. Reach for the bounding word before the retraction clause.
  - ❌ *Three whisper-quiet shafts of starlight fan from the moon — you notice the water feels lit rather than seeing rays.* (asserts three shafts, then retracts in an appositive; self-contradictory at any length)
  - ❌ *Three faint shafts of moonlight now fan down through the night water, giving the dark sea a light source instead of flat ink.* (24 words, under cap, and false twice: the shafts are sub-perceptual by const-assert, and the night sea was never "flat ink")
  - ✅ *The Trawl scene's night water now feels lit by the moon.* (the fix cost negative words)
- **Split, don't stretch.** A large scene feature landing as a dozen short bullets is the expected shape, not a warning sign. But the split hatch peels *two effects* apart — it cannot peel a *bound* off a single effect ("you don't see rays" is the absence of a percept, not its own effect). One effect plus a bound is governed by the over-promise test above.
- **Accuracy gate — every bullet, before saving.** Name the `file:line` that makes each claim true and check the claim's *shape* against it, not just its gist: **motion** needs a phase term; **"at night"** needs a `!day` gate on *the thing being described*, not a detail of it; **"plain" / "bare"** needs nothing else drawn around it; a **count** needs the user to actually resolve that count; **"now"** needs a before-state that genuinely existed.
- **Frame by user-visible effect**, not internal mechanism. Skip internal type names, file paths, function names, PR numbers.
- **Drop internal-only churn**: CI, workflow, lockfile, dep bumps with no runtime effect, refactors with no behavior change. If a refactor produces a perceptible effect (memory, startup time, fewer crashes), record the effect, not the refactor.
- **Version-header format**: use `## vX.Y.Z — YYYY-MM-DD` — `.github/workflows/release.yml`'s awk extractor keys on the exact `## vX.Y.Z` prefix at line start (followed by a space); the em-dash date suffix is repo convention.

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
cargo test --workspace
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

**Recovery — `makepkg -od` fails with `unable to find all commit-graph files` → `fatal: bad object refs/heads/main`:** the cause is a stale split commit-graph in makepkg's local cache, not the AUR or GitHub repo (`git fsck` on the mirror passes clean). Clean the bare mirror, disable graph writes there, drop the cached working copy, then re-run the block:

```bash
cd ~/aur/nokkvi-git/nokkvi                    # makepkg's bare mirror
rm -rf objects/info/commit-graphs objects/info/commit-graph
git config core.commitGraph false
git config fetch.writeCommitGraph false
git config gc.writeCommitGraph false
cd ~/aur/nokkvi-git && rm -rf src pkg         # force a fresh re-clone from the mirror
```

Both blocks are guarded — they no-op cleanly if the AUR repos aren't cloned locally (e.g. for contributors running `/package` who don't maintain AUR packages).

If either push fails due to remote-ahead (someone else pushed first), recover with `git pull --rebase && git push`.

## 9. Report

Print the GitHub release URL (`gh release view vX.Y.Z --json url -q .url`), the published artifact filenames, and the AUR package URLs (`https://aur.archlinux.org/packages/nokkvi-bin`, `https://aur.archlinux.org/packages/nokkvi-git`).
