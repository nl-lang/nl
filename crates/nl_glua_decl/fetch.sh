#!/bin/sh
# Downloads the wiki data of a glua-api-snippets release, for glua-decl:
#
#   crates/nl_glua_decl/fetch.sh [<release tag>]
#
# With no tag, the latest release. The JSON and the Lua annotations are
# unpacked into new, empty directories under target/glua/<tag>/; nothing
# in them is run. Then:
#
#   cargo run --release -p nl_glua_decl -- target/glua/<tag>/json \
#     --lua target/glua/<tag>/lua -o decl/glua.d.nl
set -eu
repo=luttje/glua-api-snippets
root=$(cd "$(dirname "$0")/../.." && pwd)
if [ $# -ge 1 ]; then
  tag=$1
else
  tag=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p')
fi
[ -n "$tag" ] || { echo "fetch.sh: no release tag" >&2; exit 1; }
dir=$root/target/glua/$tag
if [ -e "$dir" ]; then
  echo "fetch.sh: $dir exists; remove it to fetch again" >&2
  exit 1
fi
mkdir -p "$dir/json" "$dir/lua"
for kind in json lua; do
  curl -fsSL -o "$dir/$tag.$kind.zip" "https://github.com/$repo/releases/download/$tag/$tag.$kind.zip"
  unzip -q "$dir/$tag.$kind.zip" -d "$dir/$kind"
done
echo "$dir/json"
echo "$dir/lua"
