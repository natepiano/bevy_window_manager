#!/bin/bash
# Detects which monitor the XWayland Konsole is on
# Usage: detect_konsole_monitor.sh
# Outputs: "0" or "1" for the monitor index, or exits with error
# Note: xrandr enumeration order matches winit/Bevy on X11

set -e

# Get Konsole window geometry via xdotool (must run without Wayland display)
GEOMETRY=$(WAYLAND_DISPLAY= xdotool search --class "konsole" getwindowgeometry 2>/dev/null | head -4)

if [ -z "$GEOMETRY" ]; then
    echo "ERROR: No XWayland Konsole found. Run from .claude/scripts/linux_test.sh" >&2
    exit 1
fi

# Parse position from "Position: X,Y (screen: 0)"
POSITION_LINE=$(echo "$GEOMETRY" | grep "Position:")
if [ -z "$POSITION_LINE" ]; then
    echo "ERROR: Could not parse Konsole position" >&2
    exit 1
fi

# Extract X and Y coordinates
WIN_X=$(echo "$POSITION_LINE" | sed 's/.*Position: \([0-9]*\),.*/\1/')
WIN_Y=$(echo "$POSITION_LINE" | sed 's/.*Position: [0-9]*,\([0-9]*\).*/\1/')

# Get monitor geometry from xrandr (order matches winit/Bevy)
MONITORS=$(WAYLAND_DISPLAY= xrandr --query 2>/dev/null | grep " connected" | grep -oP '\d+x\d+\+\d+\+\d+')

if [ -z "$MONITORS" ]; then
    echo "ERROR: Could not get monitor geometry from xrandr" >&2
    exit 1
fi

# Build arrays of monitor bounds
declare -a MON_X MON_Y MON_W MON_H
INDEX=0

while IFS= read -r mon; do
    # Parse WxH+X+Y format
    MON_W[$INDEX]=$(echo "$mon" | sed 's/\([0-9]*\)x.*/\1/')
    MON_H[$INDEX]=$(echo "$mon" | sed 's/[0-9]*x\([0-9]*\)+.*/\1/')
    MON_X[$INDEX]=$(echo "$mon" | sed 's/[0-9]*x[0-9]*+\([0-9]*\)+.*/\1/')
    MON_Y[$INDEX]=$(echo "$mon" | sed 's/[0-9]*x[0-9]*+[0-9]*+\([0-9]*\)/\1/')
    INDEX=$((INDEX + 1))
done < <(echo "$MONITORS")

# Check which monitor contains the window position
for i in "${!MON_X[@]}"; do
    RIGHT=$((MON_X[$i] + MON_W[$i]))
    BOTTOM=$((MON_Y[$i] + MON_H[$i]))

    if [ "$WIN_X" -ge "${MON_X[$i]}" ] && [ "$WIN_X" -lt "$RIGHT" ] && \
       [ "$WIN_Y" -ge "${MON_Y[$i]}" ] && [ "$WIN_Y" -lt "$BOTTOM" ]; then
        echo "$i"
        exit 0
    fi
done

# If not found in any monitor, report error with debug info
echo "ERROR: Konsole at ($WIN_X, $WIN_Y) not within any monitor bounds" >&2
for i in "${!MON_X[@]}"; do
    RIGHT=$((MON_X[$i] + MON_W[$i]))
    BOTTOM=$((MON_Y[$i] + MON_H[$i]))
    echo "Monitor $i: (${MON_X[$i]}, ${MON_Y[$i]}) - ($RIGHT, $BOTTOM)" >&2
done
exit 1
