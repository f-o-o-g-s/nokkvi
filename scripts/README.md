# Theme Showcase Scripts

Scripts for creating video demonstrations of the Nokkvi themes.

## Scripts

### `theme_showcase.sh`
Basic theme showcase that cycles through all theme configs.

**Usage:**
```bash
./scripts/theme_showcase.sh
```

**Features:**
- Cycles through all theme configs in `~/.config/nokkvi/`
- Each theme displays for 3 seconds (configurable)
- Automatically backs up and restores your original config
- Safe interrupt handling (Ctrl+C restores original config)

### `theme_showcase_with_modes.sh`
Enhanced theme showcase that demonstrates both light and dark modes for each theme.

**Usage:**
```bash
./scripts/theme_showcase_with_modes.sh
```

**Features:**
- Shows both dark and light mode for each theme
- Prompts you to manually toggle light mode at the right time
- Each mode displays for 3 seconds (configurable)
- Automatically backs up and restores your original config
- Safe interrupt handling (Ctrl+C restores original config)

**How it works:**
The script swaps theme config files and prompts you to click the sun/moon button in the player bar when it's time to switch between light and dark modes. Just follow the on-screen prompts!

**Configuration:**
Edit the `THEMES` array in the script to customize:
- Which themes to show
- Duration for each theme
- Mode setting: `"dark"`, `"light"`, or `"both"`

Example:
```bash
THEMES=(
    "gruvbox_dark_hard_blue.toml:5:both"    # 5 seconds each for dark and light
    "config_catppuccin.toml:2:dark"         # 2 seconds, dark mode only
    "config_dracula.toml:4:light"           # 4 seconds, light mode only
)
```

### `toggle_light_mode_util`
Rust utility for toggling light/dark mode by modifying the `app.redb` database.

**Usage:**
```bash
./scripts/toggle_light_mode_util <true|false>
```

**Building:**
The utility is automatically built when you run `theme_showcase_with_modes.sh`, but you can also build it manually:
```bash
./scripts/build_toggle_util.sh
```

## Tips for Recording

1. **Start the client first**: Make sure Nokkvi is running before starting the showcase script
2. **Window size**: Set your window to the desired size before recording
3. **Timing adjustments**: If themes switch too quickly or slowly, adjust the duration values in the `THEMES` array
4. **Selective showcase**: Comment out themes you don't want to show in the video
5. **Test run**: Do a test run before recording to ensure everything works smoothly

## Example Workflow

```bash
# 1. Start the Navidrome client
./target/release/nokkvi

# 2. In another terminal, start recording (e.g., with OBS or similar)

# 3. Run the showcase script
./scripts/theme_showcase_with_modes.sh

# 4. Stop recording when the script completes
```
