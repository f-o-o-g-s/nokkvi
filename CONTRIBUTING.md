# Contributing to Nokkvi

This is a personal project I use daily. Contributions are welcome, but review might be slow since it's just me.

Please read the [AI Disclosure](#ai-disclosure) section before diving in.

## Expectations

- **I only merge what I use:** Because I maintain this codebase through prompts and testing, every new feature is something I have to maintain long-term. I generally only merge changes that fit my daily workflow. If you want to build something huge or very specific (like full Windows support or a totally new layout), it might be better to fork the project!
- **Bug reports are as good as code:** Not a programmer? Neither am I. If you find a bug, just opening an issue with clear steps to reproduce it is incredibly helpful.
- **Things might break:** This is my daily driver, but it's a hobby project. I might overhaul the UI or break features on a whim. There are no stability guarantees here.

## Getting Started

### Prerequisites

- Rust toolchain via [rustup](https://rustup.rs/) (stable + nightly)
- System dependencies (Arch Linux): `pacman -S pipewire fontconfig pkg-config`

### Build & Test

```bash
cargo build                   # Debug build
cargo build --release         # Release build
cargo test                    # Run tests
cargo clippy                  # Lint (fix all warnings)
cargo +nightly fmt --all      # Format (nightly required)
```

All four need to pass before submitting a PR.

Formatting uses **nightly rustfmt** with a custom `rustfmt.toml` (100-char lines, crate-level import merging). Install nightly with `rustup toolchain install nightly` if you don't have it.

## Good Contributions

- Bug fixes, especially edge cases, panics, or rendering issues
- Platform support for non-Arch distros or Wayland/X11 quirks
- Performance improvements (this is an audio app, latency matters)
- Documentation, screenshots, user guides
- New themes in `themes/`

## Things to Avoid

- Major architectural changes without opening an issue first
- Adding new dependencies unless truly necessary
- Breaking the TEA pattern (every view uses The Elm Architecture)
- `.unwrap()` in production code

## Submitting Changes

1. Fork the repo and create a feature branch
2. Make your changes
3. Make sure `cargo test`, `cargo clippy`, and `cargo +nightly fmt --all -- --check` pass
4. Open a PR clearly explaining **what** you did and **why**. Since I rely on AI to help me review code, good comments and a clear PR description make things way easier for me.

## AI Disclosure

**This project is entirely AI-generated.** All the code was written by AI (primarily Claude) with my direction. I'm not a developer. I come up with the ideas, test things, and steer the project, but I don't write code.

What that means in practice:

- There are probably patterns in here that experienced developers would do differently. That's fine.
- If you spot something that could be better, PRs or issues explaining the "why" are appreciated.
- I use this as my daily music player and it works great for me, but no professional developer has reviewed the codebase.
- AI-generated contributions are fine too. Use whatever tools you want.

## Code of Conduct

Be kind, be constructive, be patient. This is a hobby project. Life comes first.
