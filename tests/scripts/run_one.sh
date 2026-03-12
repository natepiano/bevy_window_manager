#!/bin/bash
if [ -z "$1" ]; then
    echo "Usage: ./tests/scripts/run_one.sh <test-id> [--no-shutdown]"
    exit 1
fi
python3 tests/scripts/run_test.py --config tests/config/linux.json --test-id "$1" --env-file /tmp/claude/discovery.env "${@:2}"
