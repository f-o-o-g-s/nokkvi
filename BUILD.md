# Building Nokkvi

Quick start guide for compiling this application from source.

## Prerequisites

### 1. Rust Toolchain

Install Rust via [rustup](https://rustup.rs/) or the Arch `rust` package.

If you plan to contribute, also install the nightly toolchain for formatting:
```bash
rustup toolchain install nightly   # only needed for cargo +nightly fmt
```

### 2. System Dependencies (Arch Linux)

```bash
sudo pacman -S pipewire fontconfig pkgconf
```

| Package | Purpose |
|---------|---------|
| `pipewire` | PipeWire development headers (native audio output via `libpipewire-0.3`) |
| `fontconfig` | Font discovery for the system font picker (used by `font-kit`) |
| `pkgconf` | Build-time dependency resolution for native libraries (provides `pkg-config`) |

> **Note:** Nokkvi uses a native PipeWire audio backend — it links directly against `libpipewire-0.3` at build time. A running PipeWire daemon is required for audio output.

## Building

```bash
# Build release version (optimized)
cargo build --release

# Binary will be at:
# target/release/nokkvi
```

For development/testing:
```bash
# Build debug version (faster compile, slower runtime)
cargo build

# Binary will be at:
# target/debug/nokkvi
```

## First Run

```bash
./target/release/nokkvi
```

The app will show a login screen on first launch. Enter your Navidrome server URL, username, and password. The config directory and `config.toml` are created automatically.

Application logs are written to `~/.local/state/nokkvi/nokkvi.log` (truncated on every launch). See [debug logging](CONTRIBUTING.md#debug-logging) in CONTRIBUTING.md for the `RUST_LOG` escape hatch when filing bug reports.

Built-in themes are automatically seeded to `~/.config/nokkvi/themes/` on first launch. To change themes, open **Settings → Theme** and pick from the list.

## More Information

See [README.md](README.md) for:
- Feature overview
- Keyboard shortcuts
- MPRIS media controls
- Theme customization
