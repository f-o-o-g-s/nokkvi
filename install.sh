#!/bin/bash
# Install Nokkvi desktop entry + icon for the current user (no sudo needed)
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BINARY="$SCRIPT_DIR/target/release/nokkvi"

# Binary
if [ ! -f "$BINARY" ]; then
    echo "❌ Binary not found at $BINARY"
    echo "   Build first:  cargo build --release"
    exit 1
fi
install -Dm755 "$BINARY" ~/.local/bin/nokkvi

# Desktop entry
install -Dm644 "$SCRIPT_DIR/assets/org.nokkvi.nokkvi.desktop" \
    ~/.local/share/applications/org.nokkvi.nokkvi.desktop

# Patch Exec= to use absolute path (launchers may not inherit shell $PATH)
sed -i "s|^Exec=nokkvi|Exec=$HOME/.local/bin/nokkvi|" \
    ~/.local/share/applications/org.nokkvi.nokkvi.desktop

# SVG icon (hicolor scalable — picked up by most icon themes)
install -Dm644 "$SCRIPT_DIR/assets/org.nokkvi.nokkvi.svg" \
    ~/.local/share/icons/hicolor/scalable/apps/org.nokkvi.nokkvi.svg

# Refresh icon cache (harmless if gtk-update-icon-cache isn't installed)
gtk-update-icon-cache -f ~/.local/share/icons/hicolor/ 2>/dev/null || true

# Refresh desktop database so launchers pick up the entry immediately
update-desktop-database ~/.local/share/applications 2>/dev/null || true

echo "✅ Installed"
echo "   Binary:  ~/.local/bin/nokkvi"
echo "   Desktop: ~/.local/share/applications/org.nokkvi.nokkvi.desktop"
echo "   Icon:    ~/.local/share/icons/hicolor/scalable/apps/org.nokkvi.nokkvi.svg"
