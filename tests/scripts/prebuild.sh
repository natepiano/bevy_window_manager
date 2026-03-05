#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# prebuild.sh — Pre-build and setup for bevy_window_manager integration tests
#
# Usage:
#   tests/scripts/prebuild.sh
#
# Auto-detects platform and uses the corresponding config file.
#
# Outputs (one per line):
#   PLATFORM=macos|linux|windows
#   CONFIG=tests/config/<platform>.json
#   BUILD_DEFAULT=ok
#   BUILD_NODEFAULT=ok|skipped
#
# Exit codes: 0 = success, 1 = build failure, 2 = script error
# ============================================================================

die() {
    echo "ERROR $*" >&2
    exit 2
}

# Detect platform and config
case "$(uname -s)" in
    Darwin)
        echo "PLATFORM=macos"
        CONFIG_FILE="tests/config/macos.json"
        ;;
    Linux)
        echo "PLATFORM=linux"
        CONFIG_FILE="tests/config/linux.json"
        ;;
    MINGW*|MSYS*|CYGWIN*)
        echo "PLATFORM=windows"
        CONFIG_FILE="tests/config/windows.json"
        ;;
    *)
        die "Unsupported platform: $(uname -s)"
        ;;
esac

echo "CONFIG=${CONFIG_FILE}"
[[ -f "$CONFIG_FILE" ]] || die "Config file not found: ${CONFIG_FILE}"

# Ensure tmp directory exists
mkdir -p /tmp/claude

# Build default variant
if cargo build --example restore_window 2>&1; then
    echo "BUILD_DEFAULT=ok"
else
    echo "BUILD_DEFAULT=failed"
    exit 1
fi

# Build no-default-features variant if any workaround tests exist
has_workarounds=$(jq '[.tests[] | select(.workaround_validation != null)] | length' "$CONFIG_FILE")

if [[ "$has_workarounds" -gt 0 ]]; then
    if cargo build --example restore_window --no-default-features 2>&1; then
        echo "BUILD_NODEFAULT=ok"
    else
        echo "BUILD_NODEFAULT=failed"
        exit 1
    fi
else
    echo "BUILD_NODEFAULT=skipped"
fi
