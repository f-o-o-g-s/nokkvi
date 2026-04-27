---
description: Bump version, build, commit, push, and tag a release
---

Follow the procedure in `.agent/workflows/package.md` exactly. Read that file first, then execute its steps in order:

1. Generate the changelog: find the last `bump version` commit, list everything since with `git log --oneline --no-merges`, and promote the `## [Unreleased]` section in `CHANGELOG.md` to `## vX.Y.Z — YYYY-MM-DD` (categories: Features / Fixes / Improvements / Internal). Add a fresh empty `## [Unreleased]` above it.
2. Read `README.md` and refresh any stale feature/dependency/build sections. Skip screenshots and media.
3. Bump `version = "X.Y.Z"` in the root `Cargo.toml` — patch bump for bugfix-only, minor bump when features are present.
4. Run the CI gate: `cargo +nightly fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build --release`. Fix anything that fails before continuing.
5. Stage everything, commit with `chore: bump version to X.Y.Z, update changelog and readme`, and push.
6. `git tag "vX.Y.Z" && git push origin "vX.Y.Z"` — this fires `.github/workflows/release.yml`, which builds the binary, packages a tarball + sha256, and creates a draft GitHub Release with the matching CHANGELOG section as the body.
7. `gh run watch --exit-status` to follow the workflow, then `gh release view "vX.Y.Z" --web` to inspect the draft. Once the tarball + sha256 + notes look right, publish with `gh release edit "vX.Y.Z" --draft=false`.
8. Report the release URL and published artifact filenames.
