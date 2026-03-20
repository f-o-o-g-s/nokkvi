#!/bin/bash
# Build the light mode toggle utility

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

echo "Building toggle_light_mode utility..."
rustc --edition 2021 \
    -L target/debug/deps \
    -L target/release/deps \
    --extern redb \
    --extern serde \
    --extern serde_json \
    --extern anyhow \
    --extern dirs \
    scripts/toggle_light_mode_util.rs \
    -o scripts/toggle_light_mode_util 2>/dev/null

if [ $? -eq 0 ]; then
    echo "✓ Build successful: scripts/toggle_light_mode_util"
else
    echo "⚠ Direct rustc build failed, trying with cargo..."
    
    # Create a temporary Cargo project
    TEMP_DIR=$(mktemp -d)
    cd "$TEMP_DIR"
    cargo init --name toggle_light_mode_util --bin
    
    # Copy dependencies from main Cargo.toml
    cat > Cargo.toml << 'EOF'
[package]
name = "toggle_light_mode_util"
version = "0.1.0"
edition = "2021"

[dependencies]
redb = "3.1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
anyhow = "1.0"
dirs = "5.0"
EOF
    
    # Copy the source
    cp "$SCRIPT_DIR/toggle_light_mode_util.rs" src/main.rs
    
    # Build
    cargo build --release
    
    # Copy binary back
    cp target/release/toggle_light_mode_util "$SCRIPT_DIR/"
    
    # Cleanup
    cd "$SCRIPT_DIR/.."
    rm -rf "$TEMP_DIR"
    
    echo "✓ Build successful: scripts/toggle_light_mode_util"
fi
