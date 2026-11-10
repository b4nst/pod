#!/usr/bin/env bash
# Smoke test: boot the simulator, walk deck -> player -> settings and check
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
# a playlist left by an earlier run would be the queue tape of the deck
rm -f "$root"/build/sim/simdisk/.pod/.playlist_control* "$root"/build/sim/simdisk/.pod/.resume.cfg*
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
"$root/scripts/sim-shot.sh" "$out/unplug" "W300 F1 S W300 D H W200 D W300 D H W300 D" >/dev/null
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
# or a hold is not (the simulator logs it, whether or not a keymap maps it).
# A double MENU is mapped on the deck: it opens the search.
"$root/scripts/sim-shot.sh" "$out/double" "W300 2M W100 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/slow" "W300 M W60 M W100 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/hold" "W300 MM W20 M W100 D" >/dev/null
grep -q '^pod: double 0x2 mapped' "$out/double/sim.log" || { echo "double tap not detected" >&2; exit 1; }
for d in slow hold; do
  ! grep -q '^pod: double' "$out/$d/sim.log" || { echo "$d counted as a double tap" >&2; exit 1; }
  ! grep -q '^pod: search' "$out/$d/sim.log" || { echo "$d opened the search" >&2; exit 1; }
done
echo "double tap ok"

# The deck is home: the wheel moves one album at a time (Afterglow, Magnetic
# Dawn, Signal / Noise, Tape Loop, ...), >> and << jump by initial (S, T, back
# to S), SELECT plays the album from its first track and opens the player, and
# MENU in the player comes back to the deck on the playing album. A single
# MENU on the deck does nothing.
areadiff() {
  magick compare -metric AE "$out/$1.png[2000x2000+0+45]" "$out/$2.png[2000x2000+0+45]" null: 2>&1 | cut -d' ' -f1
}
"$root/scripts/sim-shot.sh" "$out/deck" "W300 D F2 D R D L D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/deck-play" "W300 F2 S W200 D M W100 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/deck-shuffle" "W300 F2 2P W200 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/deck-home" "W300 D M W100 D" >/dev/null
for f in deck/01 deck/04 deck-play/01 deck-play/02 deck-home/01 deck-home/02; do
  [ -f "$out/$f.png" ] || { echo "$f.png is missing" >&2; exit 1; }
done
grep -q '^pod: deck 1/5 Afterglow' "$out/deck/sim.log" || { echo "the deck did not open on the first album" >&2; exit 1; }
grep -q '^pod: deck 3/5 Signal / Noise' "$out/deck/sim.log" || { echo "the wheel did not move the deck" >&2; exit 1; }
grep -q '^pod: deck 4/5 Tape Loop' "$out/deck/sim.log" || { echo ">> did not jump to the next initial" >&2; exit 1; }
[ "$(grep '^pod: deck' "$out/deck/sim.log" | tail -1)" = 'pod: deck 3/5 Signal / Noise' ] || { echo "<< did not jump back" >&2; exit 1; }
[ "$(areadiff deck/01 deck/04)" != 0 ] || { echo "the deck did not change" >&2; exit 1; }
grep -q '^pod: deck play Signal / Noise' "$out/deck-play/sim.log" || { echo "SELECT did not play the album" >&2; exit 1; }
grep -q '^pod: deck 4/6 Signal / Noise' "$out/deck-play/sim.log" || { echo "MENU did not go back to the deck" >&2; exit 1; }
[ "$(areadiff deck-play/01 deck-play/02)" != 0 ] || { echo "the player looks like the deck" >&2; exit 1; }
grep -q '^pod: play shuffled' "$out/deck-shuffle/sim.log" || { echo "a double PLAY did not shuffle the album" >&2; exit 1; }
! grep -q '^pod: play shuffled' "$out/deck-play/sim.log" || { echo "SELECT shuffled the album" >&2; exit 1; }
[ "$(areadiff deck-home/01 deck-home/02)" = 0 ] || { echo "MENU left the deck" >&2; exit 1; }
echo "deck ok"

# The J-card shows the cover of the album, from /.pod/covers.bin: Signal /
# Noise has one (a black square), Afterglow keeps the generated hatch, and
# without the file both do
covers=$root/build/sim/simdisk/.pod/covers.bin
cardiff() {
  magick compare -metric AE "$out/$1.png[114x177+51+57]" "$out/$2.png[114x177+51+57]" null: 2>&1 | cut -d' ' -f1
}
"$root/scripts/sim-shot.sh" "$out/covers" "W300 D F2 D" >/dev/null
mv "$covers" "$covers.off"
"$root/scripts/sim-shot.sh" "$out/nocovers" "W300 D F2 D" >/dev/null
mv "$covers.off" "$covers"
grep -q '^pod: covers 1' "$out/covers/sim.log" || { echo "the deck did not read covers.bin" >&2; exit 1; }
grep -q '^pod: covers 0' "$out/nocovers/sim.log" || { echo "the deck found covers without covers.bin" >&2; exit 1; }
[ "$(cardiff covers/01 covers/02)" != 0 ] || { echo "the covers did not draw" >&2; exit 1; }
[ "$(cardiff covers/01 nocovers/01)" = 0 ] || { echo "an album without a cover changed" >&2; exit 1; }
[ "$(cardiff covers/02 nocovers/02)" != 0 ] || { echo "the cover did not replace the hatch" >&2; exit 1; }
echo "covers ok"

# SELECT in the player scrubs: the wheel moves the position (the counter
# changes) and no longer the volume
"$root/scripts/sim-shot.sh" "$out/scrub" "W300 F1 S W200 S W20 D F8 W20 D S W20 D" >/dev/null
for i in 01 02 03; do
  [ -f "$out/scrub/$i.png" ] || { echo "scrub/$i.png is missing" >&2; exit 1; }
done
[ "$(magick compare -metric AE "$out/scrub/01.png[2000x2000+0+45]" "$out/scrub/02.png[2000x2000+0+45]" null: 2>&1 | cut -d' ' -f1)" != 0 ] || { echo "the wheel did not scrub" >&2; exit 1; }
# SELECT again ends the scrub: SEEK is gone from the status row
[ "$(magick compare -metric AE "$out/scrub/02.png[2000x2000+0+45]" "$out/scrub/03.png[2000x2000+0+45]" null: 2>&1 | cut -d' ' -f1)" != 0 ] || { echo "SELECT did not end the scrub" >&2; exit 1; }
echo "scrub ok"

# Rescan Library keeps the play stats: the first track plays to the end (20 s),
# then the rescan exports its count to the changelog and imports it after the
# rebuild, now or at the next boot (stats still queued when the simulator
# exits are lost, so this is one run)
cl=$root/build/sim/simdisk/.pod/database_changelog.txt
"$root/scripts/sim-shot.sh" "$out/rescan" "W300 F2 S W3500 M W100 MM W100 F6 W50 D S W1500 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/rescan-boot" "W1500" >/dev/null
grep -q 'playcount="[1-9]' "$cl" || { echo "the changelog has no play count" >&2; exit 1; }
cat "$out"/rescan/sim.log "$out"/rescan-boot/sim.log | grep -q '^pod: stats imported for [1-9]' || { echo "the stats were not imported" >&2; exit 1; }
echo "stats ok"

# a finished play is a line of /.pod/plays.log, written at the USB connect
# (the storage idle flush): the header, then a 5 s play of a 20 s track as S
# and a 12 s one as L, with a Unix timestamp
log=$root/build/sim/simdisk/.pod/plays.log
rm -f "$log"
"$root/scripts/sim-shot.sh" "$out/playlog" "W300 F1 S W800 R W1800 R W300 U W200 D" >/dev/null
[ -f "$log" ] || { echo "no play log" >&2; exit 1; }
[ "$(grep -c '^#' "$log")" = 3 ] && [ "$(grep -vc '^#' "$log")" = 2 ] || { echo "the play log has the wrong lines" >&2; exit 1; }
[ "$(grep -v '^#' "$log" | cut -f6 | tr -d '\n')" = SL ] || { echo "the plays are not S then L" >&2; exit 1; }
[ "$(grep -v '^#' "$log" | awk -F'\t' 'NF == 8 && $7 > 1000000000' | wc -l | tr -d ' ')" = 2 ] || { echo "the play log lines are malformed" >&2; exit 1; }
echo "play log ok"

# a mix in /Mixes is a tape in the deck, ahead of the albums under #: SELECT
# plays it and opens the player, MENU comes back to it, and >> jumps over the
# mixes to the first album. The rule line is on its tape, which the dumps show.
# The test makes its own Mixes and removes it.
mixes=$root/build/sim/simdisk/Mixes
rm -rf "$mixes"
mkdir -p "$mixes"
printf '#RULE:test rule\n/Music/Unit Δelta/Signal - Noise/01 Carrier.flac\n/Music/Unit Δelta/Signal - Noise/02 Low Orbit.flac\n' >"$mixes/Ambient.m3u8"
printf '/Music/Neon Cassette/Magnetic Dawn/03 Night Drive.flac\n' >"$mixes/Night Drive.m3u8"
"$root/scripts/sim-shot.sh" "$out/deck-mixes" "W300 D S W200 D M W100 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/deck-mixes-jump" "W300 R D L D" >/dev/null
rm -rf "$mixes"
for f in deck-mixes/01 deck-mixes/02 deck-mixes/03 deck-mixes-jump/01; do
  [ -f "$out/$f.png" ] || { echo "$f.png is missing" >&2; exit 1; }
done
grep -q '^pod: deck 1/7 Ambient' "$out/deck-mixes/sim.log" || { echo "the mixes are not first in the deck" >&2; exit 1; }
grep -q '^pod: deck play mix /Mixes/Ambient.m3u8' "$out/deck-mixes/sim.log" || { echo "SELECT did not play the mix" >&2; exit 1; }
grep -q '^pod: deck 2/8 Ambient' "$out/deck-mixes/sim.log" || { echo "MENU did not go back to the mixtape" >&2; exit 1; }
grep -q '^pod: deck 3/7 Afterglow' "$out/deck-mixes-jump/sim.log" || { echo ">> did not jump over the mixes" >&2; exit 1; }
[ "$(grep '^pod: deck' "$out/deck-mixes-jump/sim.log" | tail -1)" = 'pod: deck 1/7 Ambient' ] || { echo "<< did not jump back to the mixes" >&2; exit 1; }
[ "$(areadiff deck-mixes/01 deck-mixes/02)" != 0 ] || { echo "the player looks like the deck" >&2; exit 1; }
echo "deck mixes ok"

# The current playlist is the first tape of the deck, with a tracklist card:
# nothing plays at boot so there is none, and after Signal / Noise it comes
# before Afterglow (1/6). SELECT on it opens the player without a new
# playlist. Hold SELECT on an album asks Play next / Play last, adds it and
# stays on the deck. The editor opens with hold MENU in the player: SELECT
# picks a track up and drops it, hold SELECT removes one, MENU is the player.
"$root/scripts/sim-shot.sh" "$out/queue" "W300 F2 S W200 M W100 D B9 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/queue-next" "W300 F2 S W200 M W100 F1 SS W50 D S W150 B9 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/queue-edit" "W300 F2 S W200 MM W100 D S F1 D S W50 D SS W50 D M W100 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/queue-play" "W300 F2 S W200 M W100 B9 S W100 D" >/dev/null
for f in queue/02 queue-next/01 queue-edit/01 queue-edit/04 queue-edit/05 queue-play/01; do
  [ -f "$out/$f.png" ] || { echo "$f.png is missing" >&2; exit 1; }
done
grep -q '^pod: deck 1/6 Queue' "$out/queue/sim.log" || { echo "the queue is not the first tape" >&2; exit 1; }
! grep -q '^pod: deck 1/5 Queue' "$out/queue/sim.log" || { echo "a queue with nothing playing" >&2; exit 1; }
grep -q '^pod: deck insert next Tape Loop' "$out/queue-next/sim.log" || { echo "hold SELECT did not add the album" >&2; exit 1; }
[ "$(areadiff queue-next/01 queue-next/02)" != 0 ] || { echo "the menu looks like the deck" >&2; exit 1; }
grep -q '^pod: queue move 0 -> 1' "$out/queue-edit/sim.log" || { echo "the track was not moved" >&2; exit 1; }
grep -q '^pod: queue remove' "$out/queue-edit/sim.log" || { echo "the track was not removed" >&2; exit 1; }
[ "$(areadiff queue-edit/01 queue-edit/04)" != 0 ] || { echo "the queue did not change" >&2; exit 1; }
[ "$(areadiff queue-edit/04 queue-edit/05)" != 0 ] || { echo "MENU did not go back to the player" >&2; exit 1; }
grep -q '^pod: deck play queue' "$out/queue-play/sim.log" || { echo "SELECT did not play the queue" >&2; exit 1; }
[ "$(grep -c '^pod: deck play Signal' "$out/queue-play/sim.log")" = 1 ] || { echo "the queue was replaced" >&2; exit 1; }
[ "$(areadiff queue/02 queue-play/01)" != 0 ] || { echo "the player looks like the deck" >&2; exit 1; }
echo "queue ok"

# double SELECT in the player loves the playing track (rating 10) and does it
# again to unlove (0). A loved track has a small heart by TRK, also after
# skipping away and back, so the rating went through the tagcache. The heart
# is the 21x15 box at 3x, clear of the counter and the reels.
"$root/scripts/sim-shot.sh" "$out/love" "W300 F1 S W100 2S W20 D W200 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/unlove" "W300 F1 S W100 2S W200 2S W20 D W200 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/love-keep" "W300 F1 S W100 2S W100 R W100 L W100 D" >/dev/null
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

# a track over 10 minutes picks up where it stopped, a short one starts at 0.
# Zero Hour (11 min, the Tape Loop tape) plays 8 s, Carrier 6 s, then Zero Hour
# again starts where it stopped and Carrier at 0. One run, because the resume
# points are tagcache updates still queued when the simulator exits.
"$root/scripts/sim-shot.sh" "$out/resume" "W300 F3 S W800 M W100 B1 S W600 M W100 F1 S W300 D M W100 B1 S W300 D" >/dev/null
starts=$(grep '^pod: start' "$out/resume/sim.log")
[ "$(sed -n 1p <<<"$starts")" = 'pod: start long at 0' ] || { echo "Zero Hour is not a long track" >&2; exit 1; }
[ "$(sed -n 2p <<<"$starts")" = 'pod: start short at 0' ] || { echo "Carrier is not a short track" >&2; exit 1; }
sed -n 3p <<<"$starts" | grep -Eq 'long at [5-9][0-9]{3}$' || { echo "the long track did not resume" >&2; exit 1; }
[ "$(sed -n 4p <<<"$starts")" = 'pod: start short at 0' ] || { echo "the short track resumed" >&2; exit 1; }
echo "resume ok"

# the sleep timer (Settings, row 4) cycles Off, 15, 30, 60 min, Album. A
# minute lasts a second here: 15 min fades over its last 10 s, pauses and
# clears the timer; Album pauses at the end of Analog Heart, the last track
# of its album (Carrier, from another one, is next), and so does it with
# Repeat All, where the playlist wraps to the album's first track. Analog
# Heart plays from the search (AN), or from a mixtape for the album timer. The
# player shows the time left in the HUD column until then.
sleepmix=$root/build/sim/simdisk/Mixes
rm -rf "$sleepmix"
mkdir -p "$sleepmix"
printf '/Music/Neon Cassette/Magnetic Dawn/04 Analog Heart.flac\n/Music/Unit Δelta/Signal - Noise/01 Carrier.flac\n' >"$sleepmix/Sleep.m3u8"
export POD_SLEEP_FAST=1
"$root/scripts/sim-shot.sh" "$out/sleep" "W300 2M W100 S F7 F6 S R S W100 MM W100 MM W100 F3 S W50 D M W100 M W100 D W2000 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/sleep-album" "W300 S W100 MM W100 MM W100 F3 S S S S M W100 M W100 D W2600 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/sleep-repeat" "W300 2M W100 S F7 F6 S R S W100 MM W100 MM W100 F1 S F1 S F2 S S S S M W100 M W100 D W2600 D" >/dev/null
unset POD_SLEEP_FAST
rm -rf "$sleepmix"
for d in sleep sleep-album sleep-repeat; do
  grep -q '^pod: sleep fade' "$out/$d/sim.log" || { echo "$d did not fade" >&2; exit 1; }
  grep -q '^pod: sleep pause Analog Heart' "$out/$d/sim.log" || { echo "$d did not pause" >&2; exit 1; }
done
for f in sleep/01 sleep/02 sleep/03 sleep-album/01 sleep-album/02 sleep-repeat/02; do
  [ -f "$out/$f.png" ] || { echo "$f.png is missing" >&2; exit 1; }
done
# the time left is the 60x18 box at 3x under the track count
sleepdiff() {
  magick compare -metric AE "$out/$1.png[60x18+326+178]" "$out/$2.png[60x18+326+178]" null: 2>&1 | cut -d' ' -f1
}
[ "$(sleepdiff sleep/02 sleep/03)" != 0 ] || { echo "the time left stayed after the pause" >&2; exit 1; }
[ "$(sleepdiff sleep-album/01 sleep-album/02)" != 0 ] || { echo "the album timer stayed after the pause" >&2; exit 1; }
echo "sleep ok"

# A double MENU on the deck opens the search: SELECT adds the letter under the
# cursor, the results narrow on the first letters of a word (case, accents and
# the Δ ignored) and are grouped as tracks, artists and albums. T finds three
# tracks and the album Tape Loop, E the artist Unit Δelta by its second word,
# and >> then SELECT picks: an album puts the deck on its tape, an artist on
# its first album, a track plays its album from it. MENU deletes the last
# letter and hold MENU leaves for the deck, not for the settings. Hold SELECT
# on a track while something plays adds it to the playlist. The wheel is
# turned in steps of up to 8 clicks, which is what the input queue holds.
sim() { "$root/scripts/sim-shot.sh" "$out/$1" "$2" >/dev/null; }
rm -f "$root"/build/sim/simdisk/.pod/.playlist_control* "$root"/build/sim/simdisk/.pod/.resume.cfg*
sim search "W300 2M W100 D F8 F8 F3 S W50 D"
sim search-album "W300 2M W100 F8 F8 F3 S R F3 S W100 D"
sim search-artist "W300 2M W100 F8 F8 F4 S R F1 S W100 D"
sim search-accent "W300 2M W100 F4 S W50 D"
sim search-track "W300 2M W100 F2 S F5 S R S W200 D M W100 D"
sim search-leave "W300 2M W100 F2 S M W50 MM W100 D"
sim search-queue "W300 S W200 M W100 2M W100 F2 S F5 S R SS W50 D"
for f in search/01 search/02 search-track/01 search-track/02 search-leave/01; do
  [ -f "$out/$f.png" ] || { echo "$f.png is missing" >&2; exit 1; }
done
grep -q '^pod: search "" 0 0 0' "$out/search/sim.log" || { echo "the search did not open empty" >&2; exit 1; }
grep -q '^pod: search "T" 3 0 1' "$out/search/sim.log" || { echo "T did not find 3 tracks and an album" >&2; exit 1; }
[ "$(areadiff search/01 search/02)" != 0 ] || { echo "the results are not shown" >&2; exit 1; }
grep -q '^pod: search pick album Tape Loop' "$out/search-album/sim.log" || { echo "SELECT did not pick the album" >&2; exit 1; }
grep -q '^pod: deck 4/5 Tape Loop' "$out/search-album/sim.log" || { echo "the deck is not on the album" >&2; exit 1; }
grep -q '^pod: search "U" 1 1 0' "$out/search-artist/sim.log" || { echo "U did not find a track and an artist" >&2; exit 1; }
grep -q '^pod: deck 3/5 Signal / Noise' "$out/search-artist/sim.log" || { echo "the deck is not on the artist's first album" >&2; exit 1; }
grep -q '^pod: search "E" 0 1 0' "$out/search-accent/sim.log" || { echo "E did not find Unit Δelta" >&2; exit 1; }
grep -q '^pod: search "CH" 1 0 0' "$out/search-track/sim.log" || { echo "CH did not narrow to a track" >&2; exit 1; }
grep -q '^pod: deck play Magnetic Dawn from 2' "$out/search-track/sim.log" || { echo "the track did not play from its place" >&2; exit 1; }
grep -q '^pod: deck 3/6 Magnetic Dawn' "$out/search-track/sim.log" || { echo "MENU did not go back to the album" >&2; exit 1; }
[ "$(areadiff search-track/01 search-track/02)" != 0 ] || { echo "the player looks like the deck" >&2; exit 1; }
[ "$(grep -c '^pod: search "' "$out/search-leave/sim.log")" = 3 ] || { echo "MENU did not delete the letter" >&2; exit 1; }
[ "$(areadiff deck-home/01 search-leave/01)" = 0 ] || { echo "hold MENU did not leave for the deck" >&2; exit 1; }
grep -q '^pod: queue Chrome Tape' "$out/search-queue/sim.log" || { echo "hold SELECT did not queue the track" >&2; exit 1; }
echo "search ok"

# Infinite play (Settings, row 3) adds up to 10 tracks to the end of the
# queue when it runs out. Off, the same play (Magnetic Dawn to its 4th track)
# stops at the end of the album. The earlier tests have played every track
# within the last 150 plays, which Infinite skips, so the database starts
# again (and reboots once, like the first boot).
# Repeat is left on by the sleep tests, and stops Infinite: back to Off.
rm -f "$root"/build/sim/simdisk/.pod/database_* "$root"/build/sim/simdisk/.pod/database_changelog.txt
sed -i.bak '/^repeat:/d' "$root/build/sim/simdisk/.pod/config.cfg"
rm -f "$root"/build/sim/simdisk/.pod/config.cfg.bak
"$root/scripts/sim-shot.sh" "$out/infinite-boot" "W1500" >/dev/null
"$root/scripts/sim-shot.sh" "$out/infinite-boot2" "W1500" >/dev/null
rm -f "$root"/build/sim/simdisk/.pod/.playlist_control* "$root"/build/sim/simdisk/.pod/.resume.cfg*
sim infinite-off "W300 F1 S W200 R R R W200 D W3000 D"
rm -f "$root"/build/sim/simdisk/.pod/.playlist_control* "$root"/build/sim/simdisk/.pod/.resume.cfg*
sim infinite "W300 MM W100 F2 S W100 D M W100 F1 S W200 R R R W200 D W3000 D"
# the simulator exits before the setting is written: back to the default
sed -i.bak '/^infinite play/d' "$root/build/sim/simdisk/.pod/config.cfg"
rm -f "$root"/build/sim/simdisk/.pod/config.cfg.{new,bak}
! grep -q '^pod: infinite' "$out/infinite-off/sim.log" || { echo "Infinite added tracks while off" >&2; exit 1; }
grep -q '^pod: infinite +' "$out/infinite/sim.log" || { echo "Infinite added nothing" >&2; exit 1; }
grep -q '^pod: infinite pick' "$out/infinite/sim.log" || { echo "Infinite picked nothing" >&2; exit 1; }
for f in infinite/01 infinite/02 infinite-off/01 infinite-off/02; do
  [ -f "$out/$f.png" ] || { echo "$f.png is missing" >&2; exit 1; }
done
# the added tracks play on after the album: more starts than without
on=$(grep -c '^pod: start' "$out/infinite/sim.log" || true)
off=$(grep -c '^pod: start' "$out/infinite-off/sim.log" || true)
[ "$on" -gt "$off" ] || { echo "playback did not go on with Infinite on ($on starts, $off off)" >&2; exit 1; }
echo "infinite ok"
