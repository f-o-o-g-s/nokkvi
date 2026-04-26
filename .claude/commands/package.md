---
description: Bump version, build, commit, push, and package for distribution
---

Follow the procedure in `.agent/workflows/package.md` exactly. Read that file first, then execute its steps in order:

1. Generate the changelog: find the last `bump version` commit, list everything since with `git log --oneline --no-merges`, and add a new section to `CHANGELOG.md` (categories: Features / Fixes / Improvements / Internal).
2. Read `README.md` and refresh any stale feature/dependency/build sections. Skip screenshots and media.
3. Bump `version = "X.Y.Z"` in the root `Cargo.toml` — patch bump for bugfix-only, minor bump when features are present.
4. Run the CI gate: `cargo +nightly fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build --release`. Fix anything that fails before continuing.
5. Stage everything, commit with `chore: bump version to X.Y.Z, update changelog and readme`, and push.
6. Run `./package.sh` from the repo root.
7. Report the final zip path, size, and confirm the version in `BUILD_INFO` matches the bump.
