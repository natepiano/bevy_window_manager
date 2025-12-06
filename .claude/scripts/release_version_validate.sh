#!/bin/bash
# Validate version argument for release_version command

set -e

VERSION_ARG="$1"

# Check if version argument is provided
if [ -z "$VERSION_ARG" ]; then
    echo "Error: Version argument required"
    echo "Usage: /release_version X.Y.Z or /release_version X.Y.Z-rc.N"
    echo "Examples:"
    echo "  /release_version 0.17.0"
    echo "  /release_version 0.17.0-rc.1"
    exit 1
fi

# Validate version format using regex
# Supports: X.Y.Z, X.Y.Z-rc.N, X.Y.Z-alpha.N, X.Y.Z-beta.N
if ! echo "$VERSION_ARG" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-(rc|alpha|beta)\.[0-9]+)?$'; then
    echo "Error: Invalid version format '$VERSION_ARG'"
    echo "Expected format: X.Y.Z or X.Y.Z-rc.N"
    echo "Valid examples:"
    echo "  0.17.0"
    echo "  1.2.3-rc.1"
    echo "  2.0.0-beta.2"
    exit 1
fi

echo "✓ Version format valid: $VERSION_ARG"
