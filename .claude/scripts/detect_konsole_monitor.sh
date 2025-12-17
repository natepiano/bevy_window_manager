#!/bin/bash
# Detects which monitor the XWayland Konsole is on
# Usage: detect_konsole_monitor.sh
# Outputs: "0" or "1" for the Bevy/winit monitor index, or exits with error
# Monitor indices match Bevy/winit enumeration order (NOT sorted by position)

set -e

# Bevy/winit monitor index to position mapping (winit enumeration order)
# Monitor 0: eDP at (1512, 2880)
# Monitor 1: HDMI at (0, 0)
declare -A BEVY_MONITORS
BEVY_MONITORS["1512,2880"]=0
BEVY_MONITORS["0,0"]=1

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

# Get monitor geometry from xrandr (unsorted)
MONITORS=$(WAYLAND_DISPLAY= xrandr --query 2>/dev/null | grep " connected" | grep -oP '\d+x\d+\+\d+\+\d+')

if [ -z "$MONITORS" ]; then
    echo "ERROR: Could not get monitor geometry from xrandr" >&2
    exit 1
fi

# Build arrays of monitor bounds
declare -a MON_X MON_Y MON_W MON_H
XRANDR_INDEX=0

while IFS= read -r mon; do
    # Parse WxH+X+Y format
    MON_W[$XRANDR_INDEX]=$(echo "$mon" | sed 's/\([0-9]*\)x.*/\1/')
    MON_H[$XRANDR_INDEX]=$(echo "$mon" | sed 's/[0-9]*x\([0-9]*\)+.*/\1/')
    MON_X[$XRANDR_INDEX]=$(echo "$mon" | sed 's/[0-9]*x[0-9]*+\([0-9]*\)+.*/\1/')
    MON_Y[$XRANDR_INDEX]=$(echo "$mon" | sed 's/[0-9]*x[0-9]*+[0-9]*+\([0-9]*\)/\1/')
    XRANDR_INDEX=$((XRANDR_INDEX + 1))
done < <(echo "$MONITORS")

# Check which monitor contains the window position, then map to Bevy index
for i in "${!MON_X[@]}"; do
    RIGHT=$((MON_X[$i] + MON_W[$i]))
    BOTTOM=$((MON_Y[$i] + MON_H[$i]))

    if [ "$WIN_X" -ge "${MON_X[$i]}" ] && [ "$WIN_X" -lt "$RIGHT" ] && \
       [ "$WIN_Y" -ge "${MON_Y[$i]}" ] && [ "$WIN_Y" -lt "$BOTTOM" ]; then
        # Found the xrandr monitor, now map to Bevy index using position
        POS_KEY="${MON_X[$i]},${MON_Y[$i]}"
        BEVY_INDEX=${BEVY_MONITORS[$POS_KEY]}
        if [ -z "$BEVY_INDEX" ]; then
            echo "ERROR: Unknown monitor position ($POS_KEY) - not in Bevy mapping" >&2
            exit 1
        fi
        echo "$BEVY_INDEX"
        exit 0
    fi
done

# If not found in any monitor, report error with debug info
echo "ERROR: Konsole at ($WIN_X, $WIN_Y) not within any monitor bounds" >&2
for i in "${!MON_X[@]}"; do
    RIGHT=$((MON_X[$i] + MON_W[$i]))
    BOTTOM=$((MON_Y[$i] + MON_H[$i]))
    POS_KEY="${MON_X[$i]},${MON_Y[$i]}"
    BEVY_INDEX=${BEVY_MONITORS[$POS_KEY]:-"?"}
    echo "xrandr $i -> Bevy $BEVY_INDEX: (${MON_X[$i]}, ${MON_Y[$i]}) - ($RIGHT, $BOTTOM)" >&2
done
exit 1
