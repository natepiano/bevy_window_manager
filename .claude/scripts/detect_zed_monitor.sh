#!/bin/bash
# Detects which monitor the Zed window is on (macOS)
# Usage: detect_zed_monitor.sh
# Outputs: "0" or "1" for the monitor index, or exits with error
#
# Finds the Zed window titled "bevy_window_manager" (handles multiple Zed windows)
# Queries NSScreen directly for current monitor geometry (no temp file needed)

set -e

# Get position of the Zed window titled "bevy_window_manager"
POS=$(osascript -e 'tell application "System Events" to get position of (first window of process "Zed" whose name is "bevy_window_manager")' 2>/dev/null)

if [ -z "$POS" ]; then
    echo "ERROR: Could not find Zed window titled 'bevy_window_manager'" >&2
    exit 1
fi

# Parse "X, Y" format
WIN_X=$(echo "$POS" | cut -d',' -f1 | tr -d ' ')
WIN_Y=$(echo "$POS" | cut -d',' -f2 | tr -d ' ')

# Get monitor frames from NSScreen and convert to top-left coordinates
# NSScreen uses bottom-left origin (Cocoa), but System Events uses top-left
# NSRect format: {{origin.x, origin.y}, {size.width, size.height}}
MONITORS=$(osascript <<'EOF'
use framework "AppKit"
use scripting additions

set screenList to current application's NSScreen's screens()

-- Get the main screen (index 0) to find the total height for coordinate conversion
set mainScreen to item 1 of screenList
set mainFrame to mainScreen's frame()
-- NSRect is {{x, y}, {w, h}} - access as list items
set mainHeight to item 2 of item 2 of mainFrame as integer

set output to ""
repeat with i from 1 to count of screenList
    set scr to item i of screenList
    set frm to scr's frame()

    -- Access NSRect components: {{x, y}, {w, h}}
    set origin to item 1 of frm
    set sz to item 2 of frm

    set scrX to (item 1 of origin) as integer
    set scrY to (item 2 of origin) as integer
    set scrW to (item 1 of sz) as integer
    set scrH to (item 2 of sz) as integer

    -- Convert Y to top-left origin: new_y = mainHeight - (cocoa_y + height)
    set topLeftY to mainHeight - (scrY + scrH)

    -- Output: index,x,y,width,height (y is now in top-left coordinates)
    set output to output & (i - 1) & "," & scrX & "," & topLeftY & "," & scrW & "," & scrH & linefeed
end repeat

return output
EOF
)

if [ -z "$MONITORS" ]; then
    echo "ERROR: Could not get monitor geometry from NSScreen" >&2
    exit 1
fi

# Check which monitor contains the window position
while IFS=',' read -r idx mon_x mon_y mon_w mon_h; do
    [ -z "$idx" ] && continue

    right=$((mon_x + mon_w))
    bottom=$((mon_y + mon_h))

    if [ "$WIN_X" -ge "$mon_x" ] && [ "$WIN_X" -lt "$right" ] && \
       [ "$WIN_Y" -ge "$mon_y" ] && [ "$WIN_Y" -lt "$bottom" ]; then
        echo "$idx"
        exit 0
    fi
done <<< "$MONITORS"

# If not found in any monitor, report error with debug info
echo "ERROR: Window at ($WIN_X, $WIN_Y) not within any monitor bounds" >&2
while IFS=',' read -r idx mon_x mon_y mon_w mon_h; do
    [ -z "$idx" ] && continue
    right=$((mon_x + mon_w))
    bottom=$((mon_y + mon_h))
    echo "Monitor $idx: ($mon_x, $mon_y) - ($right, $bottom)" >&2
done <<< "$MONITORS"
exit 1
