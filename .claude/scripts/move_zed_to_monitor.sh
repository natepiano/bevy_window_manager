#!/bin/bash
# Moves Zed window to a target monitor (macOS)
# Usage: move_zed_to_monitor.sh <monitor_index>
#
# Finds the Zed window titled "bevy_window_manager" (handles multiple Zed windows)
# Positions it in the left half of the target monitor
# Queries NSScreen directly for current monitor geometry (no temp file needed)

set -e

if [ $# -ne 1 ]; then
    echo "Usage: $0 <monitor_index>" >&2
    exit 1
fi

TARGET_INDEX=$1

# Get monitor geometry from NSScreen (converted to top-left coordinates)
# NSRect format: {{origin.x, origin.y}, {size.width, size.height}}
MONITOR_INFO=$(osascript <<EOF
use framework "AppKit"
use scripting additions

set screenList to current application's NSScreen's screens()
set targetIdx to $TARGET_INDEX

if targetIdx >= (count of screenList) then
    return "ERROR: Monitor index out of range"
end if

-- Get the main screen to find total height for coordinate conversion
set mainScreen to item 1 of screenList
set mainFrame to mainScreen's frame()
set mainHeight to item 2 of item 2 of mainFrame as integer

-- Get target screen (1-indexed in AppleScript)
set targetScreen to item (targetIdx + 1) of screenList
set frm to targetScreen's frame()

-- Access NSRect components: {{x, y}, {w, h}}
set origin to item 1 of frm
set sz to item 2 of frm

set scrX to (item 1 of origin) as integer
set scrY to (item 2 of origin) as integer
set scrW to (item 1 of sz) as integer
set scrH to (item 2 of sz) as integer

-- Convert Y to top-left origin
set topLeftY to mainHeight - (scrY + scrH)

-- Return: x,y,width,height
return (scrX as text) & "," & (topLeftY as text) & "," & (scrW as text) & "," & (scrH as text)
EOF
)

if [[ "$MONITOR_INFO" == ERROR* ]]; then
    echo "$MONITOR_INFO" >&2
    exit 1
fi

# Parse monitor geometry
IFS=',' read -r MON_X MON_Y MON_W MON_H <<< "$MONITOR_INFO"

# Calculate position: left half of monitor, with offset for menu bar
# Menu bar is ~25px, add small margin
MENU_BAR_HEIGHT=25
MARGIN=20

TARGET_X=$((MON_X + MARGIN))
TARGET_Y=$((MON_Y + MENU_BAR_HEIGHT + MARGIN))

# Calculate size: left half width, most of screen height
TARGET_W=$((MON_W / 2 - MARGIN * 2))
TARGET_H=$((MON_H - MENU_BAR_HEIGHT - MARGIN * 2))

# Move and resize the Zed window titled "bevy_window_manager"
osascript <<EOF
tell application "System Events"
    tell process "Zed"
        set targetWindow to (first window whose name is "bevy_window_manager")
        set frontmost to true
        delay 0.1
        set position of targetWindow to {$TARGET_X, $TARGET_Y}
        set size of targetWindow to {$TARGET_W, $TARGET_H}
    end tell
end tell
EOF

echo "Moved Zed to monitor $TARGET_INDEX at ($TARGET_X, $TARGET_Y) size ${TARGET_W}x${TARGET_H}"

# Verify the move worked
sleep 0.2
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DETECTED=$("$SCRIPT_DIR/detect_zed_monitor.sh")

if [ "$DETECTED" != "$TARGET_INDEX" ]; then
    echo "WARNING: Zed detected on monitor $DETECTED, expected $TARGET_INDEX" >&2
    exit 1
fi
