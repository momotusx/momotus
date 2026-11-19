#!/bin/bash
# Writes the apps' update feed (plans/auto-update.md, "Discovery") to stdout: the previous feed with this release's
# assets merged in, signed.
#   update-feed.sh <tag> <assets dir> <previous latest.json, or "" for none> <P-256 private key PEM>
# The feed is an envelope {"signed": "<JSON text>", "sig": "<base64 DER ECDSA P-256 SHA-256 over signed>"}, so the body
# and its signature change in one upload. The body maps a role to {version, url, sha256, size}:
#   momotus-android-arm64-phone.apk -> android-arm64    momotus-webos-*.ipk -> webos
#   momotus-android-armv7-firetv.apk -> android-armv7   momotus-macos-*.zip -> macos
# A role is replaced only when this release's version is higher than the one in the feed, so rebuilding an older tag
# never lowers it, and a release without an asset leaves that role as it was. REPO (default momotusx/momotus) names
# the repository the asset URLs point at.
set -euo pipefail

tag=$1
dir=$2
old=$3
key=$4
repo=${REPO:-momotusx/momotus}
version=${tag#v}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "update-feed.sh: $tag is not vX.Y.Z" >&2; exit 1; }

feed='{}'
if [ -n "$old" ]; then
  # The previous feed must carry our own signature: otherwise anyone who could replace the asset would have their
  # entries signed again here.
  work=$(mktemp -d)
  trap 'rm -rf "${work:?}"' EXIT
  openssl pkey -in "$key" -pubout -out "$work/pub.pem"
  jq -r '.sig' "$old" | openssl base64 -d -A >"$work/sig"
  if ! jq -j '.signed' "$old" | openssl dgst -sha256 -verify "$work/pub.pem" -signature "$work/sig" >/dev/null 2>&1; then
    echo "update-feed.sh: $old is not signed by this key; not merging into it" >&2
    exit 1
  fi
  feed=$(jq -c '.signed | fromjson' "$old")
fi

found=0
for path in "$dir"/momotus-*; do
  [ -f "$path" ] || continue
  name=$(basename "$path")
  case $name in
    momotus-android-arm64-phone.apk) role=android-arm64 ;;
    momotus-android-armv7-firetv.apk) role=android-armv7 ;;
    momotus-webos-*.ipk) role=webos ;;
    momotus-macos-*.zip) role=macos ;;
    *) continue ;;
  esac
  found=1
  sha=$(openssl dgst -sha256 -r "$path" | cut -d' ' -f1)
  size=$(wc -c <"$path" | tr -d ' ')
  feed=$(jq -c --arg role "$role" --arg v "$version" --arg sha "$sha" --argjson size "$size" \
    --arg url "https://github.com/$repo/releases/download/$tag/$name" '
    def parts: split(".") | map(tonumber);
    if (.[$role] == null) or (($v | parts) > (.[$role].version | parts))
    then .[$role] = {version: $v, url: $url, sha256: $sha, size: $size}
    else . end' <<<"$feed")
done
[ "$found" = 1 ] || { echo "update-feed.sh: no momotus-* asset in $dir" >&2; exit 1; }

signed=$(jq -cS . <<<"$feed")
sig=$(printf '%s' "$signed" | openssl dgst -sha256 -sign "$key" | openssl base64 -A)
jq -n --arg signed "$signed" --arg sig "$sig" '{signed: $signed, sig: $sig}'
