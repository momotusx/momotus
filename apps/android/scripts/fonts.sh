#!/bin/bash
# Makes the bundled fonts: static TTF instances (Latin and Latin-extended) cut from the Google
# Fonts variable files, pinned by commit. Variable axes are not used at runtime because
# Android only supports them from API 26 and our floor is 24.
#
#   Inter 400, 500, 600, 700 (text, opsz 14)   Inter 800 (display titles and headings, opsz 32)
#
# SIL OFL 1.1; the license text goes into design/licenses. Needs uv and network.
# Usage: scripts/fonts.sh   (writes design/src/main/res/font and design/licenses)
set -euo pipefail

HERE=$(cd "$(dirname "$0")/.." && pwd)
OUT=$HERE/design/src/main/res/font
LIC=$HERE/design/licenses
WORK=${TMPDIR:-/tmp}/marquee-fonts
INTER=0b58fb370093f9a9f4ff785d94405710b79de67c
RAW=https://raw.githubusercontent.com/google/fonts

mkdir -p "$OUT" "$LIC" "$WORK"
curl -fsSL -o "$WORK/inter.ttf" "$RAW/$INTER/ofl/inter/Inter%5Bopsz%2Cwght%5D.ttf"
curl -fsSL -o "$LIC/Inter-OFL.txt" "$RAW/$INTER/ofl/inter/OFL.txt"

UNICODES="U+0000-00FF,U+0100-024F,U+1E00-1EFF,U+2000-206F,U+20A0-20CF,U+2190-21FF,U+2212,U+2022"
cut() { # <source> <axes...> -- <out>
  src=$1; shift
  axes=()
  while [ "$1" != "--" ]; do axes+=("$1"); shift; done
  shift
  out=$1
  uv run --quiet --with fonttools --with brotli python -m fontTools.varLib.instancer "$src" "${axes[@]}" -o "$WORK/tmp.ttf"
  uv run --quiet --with fonttools --with brotli pyftsubset "$WORK/tmp.ttf" --unicodes="$UNICODES" \
    --layout-features='*' --no-hinting --output-file="$out"
}

cut "$WORK/inter.ttf" wght=400 opsz=14 -- "$OUT/inter_regular.ttf"
cut "$WORK/inter.ttf" wght=500 opsz=14 -- "$OUT/inter_medium.ttf"
cut "$WORK/inter.ttf" wght=600 opsz=14 -- "$OUT/inter_semibold.ttf"
cut "$WORK/inter.ttf" wght=700 opsz=14 -- "$OUT/inter_bold.ttf"
cut "$WORK/inter.ttf" wght=800 opsz=32 -- "$OUT/inter_display_extrabold.ttf"
rm -rf "$WORK"
ls -l "$OUT"
