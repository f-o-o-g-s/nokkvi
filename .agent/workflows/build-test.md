---
description: Build, test, and lint the project
---

# Build & Test

// turbo-all

1. Check formatting (requires nightly):
```bash
cargo +nightly fmt --all -- --check
```

CI runs this against the *latest* nightly rather than a pinned one, so a
months-stale local nightly can pass here and fail there. No need to update on
every build — `/package` refreshes both channels (`rustup update stable
nightly`, no root) as part of the release gate.

2. Run clippy to enforce zero warnings (matches CI strictness):
```bash
cargo clippy --all-targets -- -D warnings
```

3. Run the full test suite (matches CI; bare `cargo test` runs only the root crate and skips nokkvi-data and nokkvi-ipc):
```bash
cargo test --workspace
```

4. Verify release build compiles:
```bash
cargo build --release
```
