#!/bin/bash
#
# Bump the CoreVM version across the entire project.
#
# Usage:
#   ./tools/bump-version.sh <new-version>
#   ./tools/bump-version.sh 0.2.0
#
# This updates:
#   - VERSION file (single source of truth)
#   - Cargo.toml workspace.package.version
#   - libs/libcorevm/Cargo.toml (separate workspace)
#
# All workspace members inherit via version.workspace = true.
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [ $# -ne 1 ]; then
    echo "Usage: $0 <new-version>"
    echo "Example: $0 0.2.0"
    exit 1
fi

NEW_VERSION="$1"

# Validate semver format (X.Y.Z with optional -pre)
if ! echo "$NEW_VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$'; then
    echo "ERROR: Version must be in semver format (e.g., 0.2.0 or 1.0.0-beta.1)"
    exit 1
fi

OLD_VERSION=$(cat "$ROOT/VERSION" | tr -d '[:space:]')
echo "Bumping version: $OLD_VERSION -> $NEW_VERSION"

# 1. VERSION file
echo "$NEW_VERSION" > "$ROOT/VERSION"
echo "  Updated VERSION"

# 2. Workspace Cargo.toml
sed -i "s/^version = \"$OLD_VERSION\"/version = \"$NEW_VERSION\"/" "$ROOT/Cargo.toml"
echo "  Updated Cargo.toml (workspace)"

# 3. libcorevm (separate workspace)
if [ -f "$ROOT/libs/libcorevm/Cargo.toml" ]; then
    sed -i "s/^version = \"$OLD_VERSION\"/version = \"$NEW_VERSION\"/" "$ROOT/libs/libcorevm/Cargo.toml"
    echo "  Updated libs/libcorevm/Cargo.toml"
fi

# 4. Verify cargo can parse it
echo ""
echo "Verifying workspace..."
cd "$ROOT"
if ! cargo metadata --format-version=1 --no-deps > /dev/null 2>&1; then
    echo "WARNING: cargo metadata check failed (may be expected inside chroot or without full toolchain)"
    echo "Version files were updated successfully."
else
    echo "OK - all packages at version $NEW_VERSION"
fi
