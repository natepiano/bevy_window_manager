#!/bin/bash
# Moves XWayland Konsole to a target monitor and tiles it to the left half
# Usage: move_konsole_to_monitor.sh <monitor_index>
# Uses KWin's DBus interface for reliable window management on Wayland

set -e

if [ $# -ne 1 ]; then
    echo "Usage: $0 <monitor_index>" >&2
    exit 1
fi

TARGET_INDEX=$1

# Validate monitor index
case "$TARGET_INDEX" in
    0|1) ;;
    *) echo "ERROR: Unknown monitor index $TARGET_INDEX (only 0 and 1 supported)" >&2; exit 1 ;;
esac

# Get Konsole window ID and activate it (KWin shortcuts work on active window)
WIN_ID=$(WAYLAND_DISPLAY= xdotool search --class "konsole" 2>/dev/null | head -1)

if [ -z "$WIN_ID" ]; then
    echo "ERROR: No XWayland Konsole found. Run from .claude/scripts/linux_test.sh" >&2
    exit 1
fi

# Activate the window so KWin shortcuts affect it
WAYLAND_DISPLAY= xdotool windowactivate "$WIN_ID"
sleep 0.1

# Use KWin's DBus interface to move window to target screen
# This works reliably even when window is tiled/snapped
busctl --user call org.kde.kglobalaccel /component/kwin org.kde.kglobalaccel.Component invokeShortcut s "Window to Screen $TARGET_INDEX"
sleep 0.2

# Tile to left half of the screen
busctl --user call org.kde.kglobalaccel /component/kwin org.kde.kglobalaccel.Component invokeShortcut s "Window Quick Tile Left"
sleep 0.1

# Maximize vertically to get full screen height
busctl --user call org.kde.kglobalaccel /component/kwin org.kde.kglobalaccel.Component invokeShortcut s "Window Maximize Vertical"
sleep 0.1

echo "Moved Konsole to monitor $TARGET_INDEX (left half, full height)"
