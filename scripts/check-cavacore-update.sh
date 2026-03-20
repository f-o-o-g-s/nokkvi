#!/usr/bin/env bash
# Check if vendored cavacore files are up to date with upstream karlstav/cava.
# Compares local files against upstream master by content hash — no tracking file needed.
#
# Usage: ./scripts/check-cavacore-update.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CAVA_DIR="$REPO_ROOT/deps/libcava-rs/cava-sys/libcava"
UPSTREAM_FILE="$CAVA_DIR/UPSTREAM"

UPSTREAM_BASE="https://raw.githubusercontent.com/karlstav/cava/master"
LOCAL_C="$CAVA_DIR/src/cavacore.c"
LOCAL_H="$CAVA_DIR/include/cava/cavacore.h"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BOLD='\033[1m'
NC='\033[0m'

echo -e "${BOLD}Checking cavacore upstream status...${NC}"
echo

# Show current synced version if tracking file exists
if [[ -f "$UPSTREAM_FILE" ]]; then
    synced_commit=$(grep '^commit:' "$UPSTREAM_FILE" | awk '{print $2}')
    synced_date=$(grep '^synced:' "$UPSTREAM_FILE" | awk '{print $2}')
    echo -e "  Local version:  ${BOLD}${synced_commit:0:12}${NC} (synced $synced_date)"
else
    echo -e "  Local version:  ${YELLOW}unknown (no UPSTREAM file)${NC}"
fi

# Fetch upstream files to temp dir
tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT

if ! curl -sfL "$UPSTREAM_BASE/cavacore.c" -o "$tmp_dir/cavacore.c" 2>/dev/null; then
    echo -e "  ${RED}Failed to fetch upstream cavacore.c${NC}"
    exit 1
fi
if ! curl -sfL "$UPSTREAM_BASE/cavacore.h" -o "$tmp_dir/cavacore.h" 2>/dev/null; then
    echo -e "  ${RED}Failed to fetch upstream cavacore.h${NC}"
    exit 1
fi

# Get upstream HEAD commit for display
upstream_sha="unknown"
if command -v gh &>/dev/null; then
    upstream_sha=$(gh api repos/karlstav/cava/commits/master --jq '.sha' 2>/dev/null || echo "unknown")
fi
echo -e "  Upstream HEAD:  ${BOLD}${upstream_sha:0:12}${NC}"
echo

# Compare by content hash
local_c_hash=$(sha256sum "$LOCAL_C" | awk '{print $1}')
local_h_hash=$(sha256sum "$LOCAL_H" | awk '{print $1}')
upstream_c_hash=$(sha256sum "$tmp_dir/cavacore.c" | awk '{print $1}')
upstream_h_hash=$(sha256sum "$tmp_dir/cavacore.h" | awk '{print $1}')

c_match=true
h_match=true

if [[ "$local_c_hash" != "$upstream_c_hash" ]]; then
    c_match=false
fi
if [[ "$local_h_hash" != "$upstream_h_hash" ]]; then
    h_match=false
fi

if $c_match && $h_match; then
    echo -e "  ${GREEN}✓ cavacore.c  up to date${NC}"
    echo -e "  ${GREEN}✓ cavacore.h  up to date${NC}"
    echo
    echo -e "${GREEN}${BOLD}All cavacore files are current with upstream.${NC}"
else
    if ! $c_match; then
        echo -e "  ${YELLOW}✗ cavacore.c  differs from upstream${NC}"
    else
        echo -e "  ${GREEN}✓ cavacore.c  up to date${NC}"
    fi
    if ! $h_match; then
        echo -e "  ${YELLOW}✗ cavacore.h  differs from upstream${NC}"
    else
        echo -e "  ${GREEN}✓ cavacore.h  up to date${NC}"
    fi

    echo
    echo -e "${YELLOW}${BOLD}Updates available!${NC} To see what changed:"
    echo
    echo "  diff $LOCAL_C $tmp_dir/cavacore.c"
    echo "  diff $LOCAL_H $tmp_dir/cavacore.h"

    # Show recent upstream commits touching these files
    if command -v gh &>/dev/null && [[ -f "$UPSTREAM_FILE" ]]; then
        synced_date=$(grep '^synced:' "$UPSTREAM_FILE" | awk '{print $2}')
        echo
        echo -e "${BOLD}Recent upstream commits:${NC}"
        gh api "repos/karlstav/cava/commits?path=cavacore.c&since=${synced_date}T00:00:00Z&per_page=5" \
            --jq '.[] | "  \(.sha[0:12]) \(.commit.message | split("\n")[0])"' 2>/dev/null || true
    fi
fi
