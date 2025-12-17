#!/bin/bash
# Launch Linux integration tests in XWayland Konsole
#
# Usage: ./linux_test.sh
#
# This script:
# 1. Launches Konsole in XWayland mode (required for xdotool position detection)
# 2. Runs Claude with the /test linux command
# 3. Claude will auto-move the terminal between monitors and run all tests

set -e

# Check if xdotool is available
if ! command -v xdotool &> /dev/null; then
    echo "Error: xdotool is required but not installed"
    echo "Install with: sudo dnf install xdotool"
    exit 1
fi

# Get the project directory (two levels up from this script)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Launch Konsole in XWayland mode with Claude running the test
# Using nohup and & to fully detach from parent shell
# --add-dir allows Claude to access the RON config directory
QT_QPA_PLATFORM=xcb nohup konsole -e bash -c "cd '$PROJECT_DIR' && claude '/test linux' --add-dir ~/.config/restore_window" &>/dev/null &

echo "Launched Linux test runner in XWayland Konsole"
echo "The test will run autonomously - check the new Konsole window"
