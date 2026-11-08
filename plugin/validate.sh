#!/usr/bin/env bash
# The checks of `omarchy plugin validate`, for machines without Omarchy (CI, dev boxes).
# Usage: plugin/validate.sh [plugin-folder]   (defaults to the repository root)
set -euo pipefail
dir="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
m="$dir/manifest.json"
fail() { echo "validate: $*" >&2; exit 1; }
command -v jq >/dev/null || fail "jq is required"
[ -f "$m" ] || fail "missing manifest.json"
jq -e . "$m" >/dev/null 2>&1 || fail "manifest.json is not valid JSON"
jq -e '.schemaVersion == 1' "$m" >/dev/null || fail "schemaVersion must be the number 1"
for f in id name version kinds entryPoints; do jq -e --arg f "$f" 'has($f)' "$m" >/dev/null || fail "missing field $f"; done
id=$(jq -r .id "$m")
[[ $id =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ && $id != *..* ]] || fail "invalid id $id"
[[ $id != omarchy.* ]] || fail "id uses the reserved omarchy.* namespace"
jq -e '(.kinds|type)=="array" and (.kinds|length)>0' "$m" >/dev/null || fail "kinds must be a non-empty array"
jq -e '(.entryPoints|type)=="object"' "$m" >/dev/null || fail "entryPoints must be an object"
jq -e 'if (.barWidget|type)=="object" and (.barWidget|has("defaultSection")) then (.barWidget.defaultSection as $s | (["left","centerBegin","center","centerEnd","tray","right"]|index($s)) != null) else true end' "$m" >/dev/null || fail "bad barWidget.defaultSection"
while IFS= read -r ep; do
  [[ -n $ep && $ep != /* && $ep != *..* ]] || fail "entry point must be a safe relative path: $ep"
  [ -f "$dir/$ep" ] || fail "entry point file not found: $ep"
done < <(jq -r '.entryPoints | to_entries[] | .value' "$m")
for pair in bar:bar bar-widget:barWidget menu:menu overlay:overlay panel:panel service:service; do
  kind=${pair%%:*}; ep=${pair##*:}
  if jq -e --arg k "$kind" '(.kinds|index($k)) != null' "$m" >/dev/null; then
    jq -e --arg e "$ep" '.entryPoints|has($e)' "$m" >/dev/null || fail "kind $kind needs entryPoints.$ep"
  fi
done
link=$(find "$dir" -name .git -prune -o -name target -prune -o -type l -print -quit)
[ -z "$link" ] || fail "symlinks are not allowed: $link"
echo "valid: $id $(jq -r .version "$m")"
