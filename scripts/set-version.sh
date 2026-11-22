#!/usr/bin/env bash
# Sets the version for a release build. Used by the release workflow only.
#
# The source keeps the placeholder version 0.0.0 on purpose: main is protected and nothing is committed
# for a release, the git tag is the source of truth. A local or development build therefore shows
# "v0.0.0". Do not commit a version bump.
#
#   scripts/set-version.sh 1.2.3
set -euo pipefail
cd "$(dirname "$0")/.."

version="${1:?usage: $0 MAJOR.MINOR.PATCH}"
if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "error: '$version' is not a MAJOR.MINOR.PATCH version" >&2
    exit 1
fi

# the first `version = "..."` line is [workspace.package]; Cargo.lock follows on the next build
sed -i '0,/^version = "[^"]*"/s//version = "'"$version"'"/' Cargo.toml
grep -q "^version = \"$version\"" Cargo.toml || { echo "error: version line not found in Cargo.toml" >&2; exit 1; }
echo "workspace version set to $version"
