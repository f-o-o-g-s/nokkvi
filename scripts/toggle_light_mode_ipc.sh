#!/bin/bash
# Simple IPC script to toggle light mode via file-based signaling

IPC_FILE="$HOME/.config/nokkvi/ipc_command"

# Write the toggle command
echo "toggle_light_mode" > "$IPC_FILE"

# Wait a moment for the app to process it
sleep 0.2

# Clean up
rm -f "$IPC_FILE"
