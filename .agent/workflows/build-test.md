---
description: Build, test, and lint the project
---

# Build & Test

// turbo-all

1. Check formatting (requires nightly):
```bash
cargo +nightly fmt --all -- --check
```

2. Run clippy for lint checks:
```bash
cargo clippy
```

3. Run the test suite:
```bash
cargo test
```

4. Verify release build compiles:
```bash
cargo build --release
```
