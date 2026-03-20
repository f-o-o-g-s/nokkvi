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
sudo pacman -S alsa-lib fontconfig pkg-config
```

| Package | Purpose |
|---------|---------|
| `alsa-lib` | ALSA development headers (audio output via cpal) |
| `fontconfig` | Font discovery for the system font picker (used by `font-kit`) |
| `pkg-config` | Build-time dependency resolution for native libraries |

> **Note:** PipeWire users get audio routing automatically via PipeWire's ALSA compatibility layer (`pipewire-alsa`), which is installed by default on PipeWire systems. No PipeWire development headers are needed to build.

> **Troubleshooting:** If you have no audio but volume looks correct in pavucontrol, install `alsa-utils` (`sudo pacman -S alsa-utils`) and run `alsamixer` — hardware channels like `Master` or `Auto-Mute` may be muted at the ALSA layer beneath PipeWire.




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

Application logs are written to `~/.config/nokkvi/nokkvi.log`.

To customize the theme, copy an example theme over your config (your credentials will be preserved on next login):
```bash
cp example_themes/config_catppuccin.toml ~/.config/nokkvi/config.toml
```


## More Information

See [README.md](README.md) for:
- Feature overview
- Keyboard shortcuts
- MPRIS media controls
- Theme customization
