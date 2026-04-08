---
description: Build, test, and lint the project
---

# Build & Test

// turbo-all

1. Check formatting (requires nightly):
```bash
cargo +nightly fmt --all -- --check
```

2. Run clippy to enforce zero warnings (matches CI strictness):
```bash
cargo clippy --all-targets -- -D warnings
```

3. Run the test suite:
```bash
cargo test
```

4. Verify release build compiles:
```bash
cargo build --release
```
