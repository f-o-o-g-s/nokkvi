---
description: Bump version, build, commit, push, and tag a release
---

Follow the procedure in `.agent/workflows/package.md` exactly. Read that file first, then execute its steps in order:

1. Generate the changelog: find the last `bump version` commit, list everything since with `git log --oneline --no-merges`, and promote the `## [Unreleased]` section in `CHANGELOG.md` to `## vX.Y.Z — YYYY-MM-DD` (categories: Features / Fixes / Improvements / Internal). Add a fresh empty `## [Unreleased]` above it.
2. Read `README.md` and refresh any stale feature/dependency/build sections. Skip screenshots and media.
3. Bump `version = "X.Y.Z"` in the root `Cargo.toml`. **Default to a patch bump** regardless of whether the changes are fixes or features — patch numbers may grow arbitrarily large (e.g. `0.3.99`). Bump the minor or major **only when the user has explicitly authorized it in this conversation**; if unsure, ask first.
4. Run the CI gate: `cargo +nightly fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build --release`. Fix anything that fails before continuing.
5. Stage everything, commit with `chore: bump version to X.Y.Z, update changelog and readme`, and push.
6. Tag and push. **Patch bump (default):** `git tag "vX.Y.Z" && git push origin "vX.Y.Z"`. **Minor or major bump (only with explicit user authorization):** `git tag -a "vX.Y.Z" -m "allow-minor-bump: <reason>"` (or `allow-major-bump`) then `git push origin "vX.Y.Z"`. The release workflow has a hard gate that rejects unauthorized minor/major bumps. The tag push fires `.github/workflows/release.yml`, which validates the bump, builds the binary, packages a tarball + sha256, and creates a draft GitHub Release with the matching CHANGELOG section as the body.
7. `gh run watch --exit-status` to follow the workflow, then `gh release view "vX.Y.Z" --web` to inspect the draft. Once the tarball + sha256 + notes look right, publish with `gh release edit "vX.Y.Z" --draft=false`.
8. Sync AUR packages. Both blocks are guarded — they no-op cleanly if the AUR repos aren't cloned locally. **`nokkvi-bin`**: `cd ~/aur/nokkvi-bin`, sed-bump `pkgver=X.Y.Z` and `pkgrel=1` in `PKGBUILD`, run `updpkgsums` to refresh sha256, `makepkg --printsrcinfo > .SRCINFO`, commit `Update to vX.Y.Z`, push. **`nokkvi-git`**: `cd ~/aur/nokkvi-git`, `makepkg -od --noconfirm` to refresh git checkout and rerun `pkgver()`, `makepkg --printsrcinfo > .SRCINFO`, only commit/push if `git diff` shows a change, message `Sync with vX.Y.Z release`. If either push fails due to remote-ahead, `git pull --rebase && git push`.
9. Report the GitHub release URL, the published artifact filenames, and both AUR package URLs (`https://aur.archlinux.org/packages/nokkvi-bin`, `https://aur.archlinux.org/packages/nokkvi-git`).
