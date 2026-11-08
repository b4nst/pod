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

# Rescan Library keeps the play stats: the first track plays to the end (20 s),
# then the rescan exports its count to the changelog and imports it after the
# rebuild, now or at the next boot (stats still queued when the simulator
# exits are lost, so this is one run)
cl=$root/build/sim/simdisk/.pod/database_changelog.txt
"$root/scripts/sim-shot.sh" "$out/rescan" "W300 F1 S W100 F2 S W100 S W3500 MM W100 F4 W50 D S W1500 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/rescan-boot" "W1500" >/dev/null
grep -q 'playcount="[1-9]' "$cl" || { echo "the changelog has no play count" >&2; exit 1; }
cat "$out"/rescan/sim.log "$out"/rescan-boot/sim.log | grep -q '^pod: stats imported for [1-9]' || { echo "the stats were not imported" >&2; exit 1; }
echo "stats ok"

# a finished play is a line of /.pod/plays.log, written at the USB connect
# (the storage idle flush): the header, then a 5 s play of a 20 s track as S
# and a 12 s one as L, with a Unix timestamp
log=$root/build/sim/simdisk/.pod/plays.log
rm -f "$log"
"$root/scripts/sim-shot.sh" "$out/playlog" "W300 F2 S W100 S W800 R W1800 R W300 U W200 D" >/dev/null
[ -f "$log" ] || { echo "no play log" >&2; exit 1; }
[ "$(grep -c '^#' "$log")" = 3 ] && [ "$(grep -vc '^#' "$log")" = 2 ] || { echo "the play log has the wrong lines" >&2; exit 1; }
[ "$(grep -v '^#' "$log" | cut -f6 | tr -d '\n')" = SL ] || { echo "the plays are not S then L" >&2; exit 1; }
[ "$(grep -v '^#' "$log" | awk -F'\t' 'NF == 8 && $7 > 1000000000' | wc -l | tr -d ' ')" = 2 ] || { echo "the play log lines are malformed" >&2; exit 1; }
echo "play log ok"

# the Mixes row (root row 4) lists /Mixes/*.m3u8 by name and SELECT plays the
# mix and opens the player; with none it says to sync. The test makes its own
# Mixes and removes it.
mixes=$root/build/sim/simdisk/Mixes
rm -rf "$mixes"
"$root/scripts/sim-shot.sh" "$out/mixes-empty" "W300 F3 S W100 D M W100 D" >/dev/null
mkdir -p "$mixes"
printf '/Music/Unit Δelta/Signal - Noise/01 Carrier.flac\n/Music/Unit Δelta/Signal - Noise/02 Low Orbit.flac\n' >"$mixes/Ambient.m3u8"
printf '/Music/Neon Cassette/Magnetic Dawn/03 Night Drive.flac\n' >"$mixes/Night Drive.m3u8"
"$root/scripts/sim-shot.sh" "$out/mixes" "W300 F3 S W100 D S W200 D" >/dev/null
rm -rf "$mixes"
grep -q '^pod: mixes 0' "$out/mixes-empty/sim.log" || { echo "the empty Mixes was not detected" >&2; exit 1; }
grep -q '^pod: mixes 2' "$out/mixes/sim.log" || { echo "the mixes were not listed" >&2; exit 1; }
grep -q '^pod: mix /Mixes/Ambient.m3u8' "$out/mixes/sim.log" || { echo "SELECT did not play the mix" >&2; exit 1; }
for i in 01 02; do
  [ -f "$out/mixes-empty/$i.png" ] && [ -f "$out/mixes/$i.png" ] || { echo "mixes dump $i is missing" >&2; exit 1; }
done
[ "$(areadiff mixes-empty/01 mixes/01)" != 0 ] || { echo "the mix list looks like the empty one" >&2; exit 1; }
echo "mixes ok"

# PLAY plays the highlighted row in order and opens the player; a double tap
# plays it shuffled, and Shuffle All (root row 6) shuffles on a single tap.
# The dumps are the player, which differs from the library; PLAY on Artists
# (a menu row) plays nothing.
"$root/scripts/sim-shot.sh" "$out/play" "W300 D S W100 P W200 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/play-double" "W300 S W100 2P W200 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/play-all" "W300 F5 P W200 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/play-root" "W300 P W200 D" >/dev/null
! grep -q '^pod: play shuffled' "$out/play/sim.log" || { echo "PLAY shuffled" >&2; exit 1; }
for d in play-double play-all; do
  grep -q '^pod: play shuffled' "$out/$d/sim.log" || { echo "$d did not shuffle" >&2; exit 1; }
done
for d in play play-double play-all play-root; do
  [ -f "$out/$d/01.png" ] || { echo "$d dump is missing" >&2; exit 1; }
done
[ "$(areadiff play/01 play/02)" != 0 ] || { echo "PLAY did not open the player" >&2; exit 1; }
[ "$(magick "$out/play/02.png" -format '%k' info:)" -gt 1 ] || { echo "play/02.png is blank" >&2; exit 1; }
[ "$(areadiff play/01 play-double/01)" != 0 ] || { echo "a double tap did not open the player" >&2; exit 1; }
[ "$(areadiff play/01 play-all/01)" != 0 ] || { echo "Shuffle All did not open the player" >&2; exit 1; }
! grep -q '^pod: play shuffled' "$out/play-root/sim.log" || { echo "PLAY on Artists played" >&2; exit 1; }
echo "play ok"

# double SELECT in the player loves the playing track (rating 10) and does it
# again to unlove (0). A loved track has a small heart by TRK, also after
# skipping away and back, so the rating went through the tagcache. The heart
# is the 21x15 box at 3x, clear of the counter and the reels.
"$root/scripts/sim-shot.sh" "$out/love" "W300 F2 S W100 S W100 2S W20 D W200 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/unlove" "W300 F2 S W100 S W100 2S W200 2S W20 D W200 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/love-keep" "W300 F2 S W100 S W100 2S W100 R W100 L W100 D" >/dev/null
grep -q '^pod: love 10' "$out/love/sim.log" || { echo "double SELECT did not love" >&2; exit 1; }
grep -q '^pod: love 0' "$out/unlove/sim.log" || { echo "double SELECT did not unlove" >&2; exit 1; }
markdiff() {
  magick compare -metric AE "$out/$1.png[21x15+393+132]" "$out/$2.png[21x15+393+132]" null: 2>&1 | cut -d' ' -f1
}
for f in love/01 love/02 unlove/01 unlove/02 love-keep/01; do
  [ -f "$out/$f.png" ] || { echo "$f.png is missing" >&2; exit 1; }
done
[ "$(markdiff love/02 unlove/02)" != 0 ] || { echo "the loved mark is not shown" >&2; exit 1; }
[ "$(markdiff love/01 love/02)" = 0 ] || { echo "the mark went away" >&2; exit 1; }
[ "$(markdiff love/02 love-keep/01)" = 0 ] || { echo "the love was lost on a skip" >&2; exit 1; }
[ "$(areadiff love/01 love/02)" != 0 ] || { echo "the heart flash is not shown" >&2; exit 1; }
echo "love ok"
