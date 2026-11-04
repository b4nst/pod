#!/usr/bin/env bash
# Build the theme into build/theme/.rockbox and the splash bitmap into the
# patched source tree. Needs the devenv shell and a built tools/convbdf.
#   scripts/build-theme.sh <rockbox-src> [splash.bmp]
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
src=$(cd "$1" && pwd)
work=$root/build/theme-work
out=$root/build/theme/.rockbox
rm -rf "$work" "$out"
mkdir -p "$work" "$out/fonts" "$out/themes"

ui_h=12   # JetBrains Mono 11px, ascent/descent trimmed to 10+2

fonts_jb=${JETBRAINS_MONO_DIR:?}; fonts_or=${ORBITRON_DIR:?}
otf2bdf -p 11 -r 72 "$fonts_jb/JetBrainsMono-Regular.ttf" -o "$work/jb11.bdf" || true
otf2bdf -p 10 -r 72 "$fonts_or/Orbitron Medium.ttf"      -o "$work/or10.bdf" || true
otf2bdf -p 14 -r 72 "$fonts_or/Orbitron Bold.ttf"        -o "$work/or14b.bdf" || true
cp "$src/fonts/06-Tiny.bdf" "$work/tiny6.bdf"

# big splash glyph: a geometric Δ drawn at 8x and downsampled, so its edges
# antialias into the 4 LCD grey levels
magick -size 512x480 xc:black -fill none -stroke white -strokewidth 48 \
  -draw 'stroke-linejoin miter polygon 256,84 460,436 52,436' \
  -resize 64x60 -depth 8 "$work/delta.pgm"

splash=${2:-$src/apps/bitmaps/native/rockboxlogo.138x110x2.bmp}
python3 "$root/tools/mktheme.py" "$work" "$work/delta.pgm" "$out" "$splash" "$ui_h"

cvt() { "$src/tools/convbdf" -f "$@" >/dev/null; }
cvt -a '-1!' -d '-1!' -o "$out/fonts/11-JetBrainsMono.fnt" "$work/jb11.bdf"
cvt -o "$out/fonts/10-Orbitron-Medium.fnt" "$work/or10.bdf"
cvt -o "$out/fonts/14-Orbitron-Bold.fnt" "$work/or14b.bdf"
cvt -o "$out/fonts/06-Tiny.fnt" "$work/tiny6.bdf"

cp -R "$root/theme/." "$out/"
