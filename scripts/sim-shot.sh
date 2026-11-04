#!/usr/bin/env bash
# Run the headless simulator with a POD_SCRIPT and collect screendumps as PNG.
#   scripts/sim-shot.sh <outdir> "<script>"
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
out=$(mkdir -p "$1" && cd "$1" && pwd); script=$2
sim=$root/build/sim
mkdir -p "$out"
rm -f "$sim"/simdisk/dump*.bmp
cd "$sim"
SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy POD_SCRIPT="$script X" \
  perl -e 'alarm 180; exec @ARGV' ./rockboxui >"$out/sim.log" 2>&1 || true
i=0
shopt -s nullglob
for f in "$sim"/simdisk/dump*.bmp; do
  i=$((i+1))
  magick "$f" -filter point -resize 300% "$out/$(printf %02d $i).png"
done
echo "$i screenshots in $out"
