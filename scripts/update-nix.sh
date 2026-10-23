#!/usr/bin/env bash
# Bumps version + hash in packaging/nix/package.nix to a release.
#   ./scripts/update-nix.sh [version]   (defaults to the latest GitHub release)
set -euo pipefail
cd "$(dirname "$0")/.."

file=packaging/nix/package.nix
repo=Netflate/LumineCapture

cur=$(grep -oP '^\s*version = "\K[^"]+' "$file")
new=${1:-$(curl -fsSL ${GH_TOKEN:+-H "Authorization: Bearer $GH_TOKEN"} \
  "https://api.github.com/repos/$repo/releases/latest" | jq -r .tag_name)}
new=${new#v}

if [[ -z $new || $new == "null" ]]; then
  echo "couldn't work out the release version" >&2
  exit 1
fi
if [[ $new == "$cur" ]]; then
  echo "already at $cur"
  exit 0
fi

echo "$cur -> $new"
old_hash=$(grep -oP '^\s*hash = "\K[^"]+' "$file")
new_hash=$(nix store prefetch-file --json \
  "https://github.com/$repo/releases/download/v$new/lumine-capture_$new-1_amd64.deb" | jq -r .hash)

sed -i "s|version = \"$cur\";|version = \"$new\";|" "$file"
sed -i "s|hash = \"$old_hash\";|hash = \"$new_hash\";|" "$file"
