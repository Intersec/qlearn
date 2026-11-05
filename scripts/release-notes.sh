#!/usr/bin/env bash
# Print the CHANGELOG section of one version.
#
# The release job attaches this text to the tag, so a reader sees the same
# notes on the release page and in the file.
#
#   scripts/release-notes.sh v0.3.1

set -euo pipefail

tag=${1:?usage: release-notes.sh <tag>}
version=${tag#v}
root=$(cd "$(dirname "$0")/.." && pwd)

notes=$(awk -v want="## [$version]" '
    $0 == want { on = 1; next }
    on && /^## \[/ { exit }
    on { print }
' "$root/CHANGELOG.md")

if [ -z "$(printf '%s' "$notes" | tr -d '[:space:]')" ]; then
    echo "release-notes.sh: CHANGELOG.md has no section for $version" >&2
    exit 1
fi

printf '%s\n' "$notes"
