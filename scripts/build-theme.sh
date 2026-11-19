#!/usr/bin/env bash
# Build the theme into build/theme/.pod and the splash bitmap into the
# patched source tree. Needs the devenv shell and a built tools/convbdf.
#   scripts/build-theme.sh <rockbox-src> <target> [splash.bmp usb.bmp podlogo.h]
# The target is ipodmini2g, ipodmini1g or ipod6g. The Classic gets the
# autoreverse look (mktheme6g.py) and the Terminus fonts as "Tape": OFL
# reserves the name.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
src=$(cd "$1" && pwd)
target=$2
work=$root/build/theme-work
out=$root/build/theme/.pod
rm -rf "$work" "$out"
mkdir -p "$work" "$out/fonts" "$out/themes"
cp -R "$root/firmware/theme/." "$out/"

ui_h=12   # JetBrains Mono 11px, ascent/descent trimmed to 10+2

fonts_jb=${JETBRAINS_MONO_DIR:?}; fonts_or=${ORBITRON_DIR:?}
otf2bdf -p 11 -r 72 "$fonts_jb/JetBrainsMono-Regular.ttf" -o "$work/jb11.bdf" || true
otf2bdf -p 10 -r 72 "$fonts_or/Orbitron Medium.ttf"      -o "$work/or10.bdf" || true
otf2bdf -p 14 -r 72 "$fonts_or/Orbitron Bold.ttf"        -o "$work/or14b.bdf" || true
otf2bdf -p 40 -r 72 -l '35 65_90' "$fonts_or/Orbitron Bold.ttf" -o "$work/or40b.bdf" || true
cp "$src/fonts/06-Tiny.bdf" "$work/tiny6.bdf"

# big splash glyph: a geometric Δ drawn at 8x and downsampled, so its edges
# antialias into the 4 LCD grey levels
magick -size 512x480 xc:black -fill none -stroke white -strokewidth 48 \
  -draw 'stroke-linejoin miter polygon 256,84 460,436 52,436' \
  -resize 64x60 -depth 8 "$work/delta.pgm"

if [ "$target" = ipod6g ]; then
  splash=${3:-$src/apps/bitmaps/native/rockboxlogo.320x240x16.bmp}
  usb=${4:-$src/apps/bitmaps/native/usblogo.320x240x16.bmp}
  podlogo=${5:-$src/bootloader/podlogo.h}
  mkdir -p "$work/terminus"
  tar xzf "${TERMINUS_SRC:?}" -C "$work/terminus" --strip-components=1
  for f in 12n:12 14b:14b 16n:16 18b:18b 32b:32b; do
    sed -E '/^(FONT|FAMILY_NAME) /s/Terminus/Tape/' "$work/terminus/ter-u${f%:*}.bdf" >"$work/ter${f#*:}.bdf"
  done
  # the UI font is a row (24 px) tall, glyphs centred: a list's selection bar
  # is as tall as its text line
  sed -i.bak -E 's/^FONT_ASCENT 12/FONT_ASCENT 16/; s/^FONT_DESCENT 4/FONT_DESCENT 8/' "$work/ter16.bdf"
  python3 "$root/firmware/tools/mktheme6g.py" "$work" "$out" "$splash" "$usb" "$podlogo"
else
  splash=${3:-$src/apps/bitmaps/native/rockboxlogo.138x110x2.bmp}
  usb=${4:-$src/apps/bitmaps/native/usblogo.138x110x2.bmp}
  python3 "$root/firmware/tools/mktheme.py" "$work" "$work/delta.pgm" "$out" "$splash" "$ui_h" "$usb"
fi

cvt() { "$src/tools/convbdf" -f "$@" >/dev/null; }
cvt -a '-1!' -d '-1!' -o "$out/fonts/11-JetBrainsMono.fnt" "$work/jb11.bdf"
cvt -o "$out/fonts/10-Deck-Medium.fnt" "$work/or10.bdf"
cvt -o "$out/fonts/14-Deck-Bold.fnt" "$work/or14b.bdf"
cvt -o "$out/fonts/40-Deck-Letter.fnt" "$work/or40b.bdf"
cvt -o "$out/fonts/06-Tiny.fnt" "$work/tiny6.bdf"


if [ "$target" = ipod6g ]; then
  cvt -o "$out/fonts/12-Tape.fnt" "$work/ter12.bdf"
  cvt -o "$out/fonts/14-Tape-Bold.fnt" "$work/ter14b.bdf"
  cvt -o "$out/fonts/16-Tape.fnt" "$work/ter16.bdf"
  cvt -o "$out/fonts/18-Tape-Bold.fnt" "$work/ter18b.bdf"
  cvt -o "$out/fonts/32-Tape-Bold.fnt" "$work/ter32b.bdf"
  # config.cfg: the generated theme, then the defaults from the first blank line.
  # The Classic's LCD is dark without the backlight: it stays on twice as long.
  { head -n1 "$root/firmware/theme/config.cfg"
    grep -v '^#' "$out/themes/pod.cfg"
    sed -n '/^$/,$p' "$root/firmware/theme/config.cfg" \
      | sed 's/^backlight timeout: .*/backlight timeout: 30/'; } >"$out/config.cfg"
fi
