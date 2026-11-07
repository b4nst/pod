#!/usr/bin/env bash
# Smoke test: boot the simulator, walk library -> folder -> settings and check
# that every screendump was taken. Run after scripts/sim-setup.sh.
#   scripts/sim-test.sh [outdir]
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-$root/build/shots/test}
script="W300 D S W100 D MM W100 D M W100 D"
want=$(tr ' ' '\n' <<<"$script" | grep -c '^D$')
rm -rf "$out"
# the first boot scans the library and reboots to commit it, which ends the
# simulator: get that out of the way
"$root/scripts/sim-shot.sh" "$out/first-boot" "W1500" >/dev/null
"$root/scripts/sim-shot.sh" "$out" "$script"
got=$(find "$out" -maxdepth 1 -name '*.png' | wc -l | tr -d ' ')
if [ "$got" != "$want" ]; then
  echo "expected $want screenshots, got $got" >&2
  tail -30 "$out/sim.log" >&2
  exit 1
fi
# a screen that is all one colour means nothing was drawn
for f in "$out"/*.png; do
  n=$(magick "$f" -format '%k' info:)
  [ "$n" -gt 1 ] || { echo "$f is blank" >&2; exit 1; }
done
echo "sim ok: $got screens"

# unplugging the headphones pauses and replugging leaves it paused: the time
# and the reels stop moving, so the last three dumps are identical below the
# status bar (its clock and battery tick on their own)
"$root/scripts/sim-shot.sh" "$out/unplug" "W300 F2 S W100 S W300 D H W200 D W300 D H W300 D" >/dev/null
diffpx() {
  magick compare -metric AE "$out/unplug/$1.png[2000x2000+0+45]" "$out/unplug/$2.png[2000x2000+0+45]" null: 2>&1 | cut -d' ' -f1
}
for i in 01 02 03 04; do
  [ -f "$out/unplug/$i.png" ] || { echo "unplug/$i.png is missing" >&2; exit 1; }
done
[ "$(diffpx 01 02)" != 0 ] || { echo "unplug did not change the screen" >&2; exit 1; }
for i in 03 04; do
  [ "$(diffpx 02 $i)" = 0 ] || { echo "playback moved after the unplug ($i.png)" >&2; exit 1; }
done
echo "unplug ok"
