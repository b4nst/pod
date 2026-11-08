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

# a second press within 300 ms of a short tap is a double tap, a slower one
# or a hold is not (the simulator logs it, whether or not a keymap maps it)
"$root/scripts/sim-shot.sh" "$out/double" "W300 2M W100 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/slow" "W300 M W60 M W100 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/hold" "W300 MM W20 M W100 D" >/dev/null
grep -q '^pod: double 0x2 unmapped' "$out/double/sim.log" || { echo "double tap not detected" >&2; exit 1; }
for d in slow hold; do
  ! grep -q '^pod: double' "$out/$d/sim.log" || { echo "$d counted as a double tap" >&2; exit 1; }
done
echo "double tap ok"

# an artist's albums come by year: the first one under Neon Cassette (1984) is
# Magnetic Dawn, the second one in Albums (by title). Opened, they show the
# same tracks, which they would not if the first row were Afterglow (1991) or
# one of the [All tracks] rows.
"$root/scripts/sim-shot.sh" "$out/artist" "W300 S W100 F1 S W100 S W100 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/album" "W300 F1 S W100 F1 S W100 D" >/dev/null
for d in artist album; do
  [ -f "$out/$d/01.png" ] || { echo "$d/01.png is missing" >&2; exit 1; }
done
d=$(magick compare -metric AE "$out/artist/01.png[2000x2000+0+45]" "$out/album/01.png[2000x2000+0+45]" null: 2>&1 | cut -d' ' -f1)
[ "$d" = 0 ] || { echo "the first album of an artist is not the earliest one" >&2; exit 1; }
echo "albums ok"

# MENU in the player opens the playing track in its album (Albums, level 2,
# the second track), and MENU backs out of there one level at a time up to
# the library root, where one more goes to the player
"$root/scripts/sim-shot.sh" "$out/goto" "W300 F1 S W100 F2 S W100 F1 S W200 D M W100 D M W100 D M W100 D M W200 D" >/dev/null
grep -q '^pod: current track level 2 item 1' "$out/goto/sim.log" || { echo "MENU did not open the playing track" >&2; exit 1; }
for i in 01 02 03 04 05; do
  [ -f "$out/goto/$i.png" ] || { echo "goto/$i.png is missing" >&2; exit 1; }
done
echo "goto ok"

# SELECT in the player scrubs: the wheel moves the position (the counter
# changes) and no longer the volume
"$root/scripts/sim-shot.sh" "$out/scrub" "W300 F2 S W100 S W200 S W20 D F8 W20 D S W20 D" >/dev/null
for i in 01 02 03; do
  [ -f "$out/scrub/$i.png" ] || { echo "scrub/$i.png is missing" >&2; exit 1; }
done
[ "$(magick compare -metric AE "$out/scrub/01.png[2000x2000+0+45]" "$out/scrub/02.png[2000x2000+0+45]" null: 2>&1 | cut -d' ' -f1)" != 0 ] || { echo "the wheel did not scrub" >&2; exit 1; }
# SELECT again ends the scrub: SEEK is gone from the status row
[ "$(magick compare -metric AE "$out/scrub/02.png[2000x2000+0+45]" "$out/scrub/03.png[2000x2000+0+45]" null: 2>&1 | cut -d' ' -f1)" != 0 ] || { echo "SELECT did not end the scrub" >&2; exit 1; }
echo "scrub ok"

# << and >> jump by initial in the library, and a fast wheel shows the letter
# box over the list until 600 ms after it stops. In Songs (Analog Heart,
# Carrier, Chrome Tape, Concrete, Dropout, Dusk, ...) >> >> lands on Dropout
# like four clicks do, and << << from the third row is back on the first.
"$root/scripts/sim-shot.sh" "$out/letters-next" "W300 F2 S W100 R W100 R W100 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/letters-click" "W300 F2 S W100 F4 W100 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/letters-prev" "W300 F2 S W100 F2 L W100 D L W100 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/letters-top" "W300 F2 S W100 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/letters-fast" "W300 F2 S W100 f1 D W100 D" >/dev/null
for d in letters-next letters-click letters-prev letters-top letters-fast; do
  [ -f "$out/$d/01.png" ] || { echo "$d/01.png is missing" >&2; exit 1; }
done
areadiff() {
  magick compare -metric AE "$out/$1.png[2000x2000+0+45]" "$out/$2.png[2000x2000+0+45]" null: 2>&1 | cut -d' ' -f1
}
[ "$(areadiff letters-next/01 letters-click/01)" = 0 ] || { echo ">> did not jump by initial" >&2; exit 1; }
[ "$(areadiff letters-prev/02 letters-top/01)" = 0 ] || { echo "<< did not jump back by initial" >&2; exit 1; }
[ -f "$out/letters-fast/02.png" ] || { echo "letters-fast/02.png is missing" >&2; exit 1; }
[ "$(areadiff letters-fast/01 letters-fast/02)" != 0 ] || { echo "a fast wheel did not show the letter" >&2; exit 1; }
echo "letters ok"
