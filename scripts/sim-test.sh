#!/usr/bin/env bash
# Smoke test: boot the simulator, walk deck -> player -> settings and check
# that every screendump was taken. Run after scripts/sim-setup.sh.
#   scripts/sim-test.sh [outdir]
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-$root/build/shots/test}
script="W300 D P W100 D MM W100 D M W100 D"
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
"$root/scripts/sim-shot.sh" "$out/unplug" "W300 F2 P W300 D H W200 D W300 D H W300 D" >/dev/null
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
done
echo "double tap ok"

# The deck is home: the albums go by artist, then title (Warehouse Sessions,
# Afterglow, Magnetic Dawn, Signal / Noise, ...), the wheel moves one at a
# time, >> and << jump by the artist's initial (N, U, back to N), PLAY plays the album from its first track and opens the player, and
# MENU in the player comes back to the deck on the playing album. With nothing
# playing, a single MENU on the deck opens the search.
areadiff() {
  magick compare -metric AE "$out/$1.png[2000x2000+0+45]" "$out/$2.png[2000x2000+0+45]" null: 2>&1 | cut -d' ' -f1
}
"$root/scripts/sim-shot.sh" "$out/deck" "W300 D F1 W20 D R W20 D L W20 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/deck-play" "W300 F3 P W200 D M W100 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/deck-shuffle" "W300 F3 2P W200 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/deck-home" "W300 D M W100 D" >/dev/null
for f in deck/01 deck/04 deck-play/01 deck-play/02 deck-home/01 deck-home/02; do
  [ -f "$out/$f.png" ] || { echo "$f.png is missing" >&2; exit 1; }
done
grep -q '^pod: deck 1/5 Warehouse Sessions' "$out/deck/sim.log" || { echo "the deck did not open on the first album" >&2; exit 1; }
grep -q '^pod: deck 2/5 Afterglow' "$out/deck/sim.log" || { echo "the wheel did not move the deck" >&2; exit 1; }
grep -q '^pod: deck 4/5 Signal / Noise' "$out/deck/sim.log" || { echo ">> did not jump to the next initial" >&2; exit 1; }
[ "$(grep '^pod: deck' "$out/deck/sim.log" | tail -1)" = 'pod: deck 2/5 Afterglow' ] || { echo "<< did not jump back" >&2; exit 1; }
[ "$(areadiff deck/01 deck/04)" != 0 ] || { echo "the deck did not change" >&2; exit 1; }
grep -q '^pod: deck play Signal / Noise' "$out/deck-play/sim.log" || { echo "PLAY did not play the album" >&2; exit 1; }
grep -q '^pod: deck 5/6 Signal / Noise' "$out/deck-play/sim.log" || { echo "MENU did not go back to the deck" >&2; exit 1; }
[ "$(areadiff deck-play/01 deck-play/02)" != 0 ] || { echo "the player looks like the deck" >&2; exit 1; }
grep -q '^pod: play shuffled' "$out/deck-shuffle/sim.log" || { echo "a double PLAY did not shuffle the album" >&2; exit 1; }
! grep -q '^pod: play shuffled' "$out/deck-play/sim.log" || { echo "PLAY shuffled the album" >&2; exit 1; }
grep -q '^pod: search ""' "$out/deck-home/sim.log" || { echo "MENU did not open the search" >&2; exit 1; }
echo "deck ok"

# The gallery draws the covers of covers.bin v2: Signal / Noise has one (a
# black square) in place of its generated card, Warehouse Sessions has none,
# and a v1 file or none is not read, so the deck is the same in both
covers=$root/build/sim/simdisk/.pod/covers.bin
cardiff() {
  magick compare -metric AE "$out/$1.png[132x132+96+180]" "$out/$2.png[132x132+96+180]" null: 2>&1 | cut -d' ' -f1
}
"$root/scripts/sim-shot.sh" "$out/covers" "W300 D F3 W20 D" >/dev/null
cp "$covers" "$covers.v2"
printf '\001' | dd of="$covers" bs=1 seek=4 conv=notrunc 2>/dev/null
"$root/scripts/sim-shot.sh" "$out/v1covers" "W300 D F3 W20 D" >/dev/null
mv "$covers.v2" "$covers"
mv "$covers" "$covers.off"
"$root/scripts/sim-shot.sh" "$out/nocovers" "W300 D F3 W20 D" >/dev/null
mv "$covers.off" "$covers"
grep -q '^pod: covers 1' "$out/covers/sim.log" || { echo "the deck did not read covers.bin v2" >&2; exit 1; }
grep -q '^pod: covers 0' "$out/v1covers/sim.log" || { echo "the deck read a v1 covers.bin" >&2; exit 1; }
grep -q '^pod: covers 0' "$out/nocovers/sim.log" || { echo "the deck found covers without covers.bin" >&2; exit 1; }
grep -q '^pod: cover read' "$out/covers/sim.log" || { echo "the deck did not read a cover" >&2; exit 1; }
! grep -q '^pod: cover read' "$out"/{v1covers,nocovers}/sim.log || { echo "the deck read a cover record" >&2; exit 1; }
[ "$(cardiff covers/01 nocovers/01)" = 0 ] || { echo "an album without a cover changed" >&2; exit 1; }
[ "$(cardiff covers/02 nocovers/02)" != 0 ] || { echo "the cover did not replace the card" >&2; exit 1; }
[ "$(cardiff v1covers/02 nocovers/02)" = 0 ] || { echo "the v1 covers changed the deck" >&2; exit 1; }
echo "covers ok"

# Deck v2: SELECT steps the panes of a tape (Details, Tracks, Stats and back,
# with dots under the battery) and the wheel moves the track in Tracks, a pane
# goes back to Details 5 s after the last press, PLAY in Tracks plays from the
# track, MENU goes to the player while something plays (the cassette in the
# corner is lit, grey when paused) and opens the search when nothing does.
# The gallery moves with the wheel.
cassettediff() {
  magick compare -metric AE "$out/$1.png[57x33+288+0]" "$out/$2.png[57x33+288+0]" null: 2>&1 | cut -d' ' -f1
}
"$root/scripts/sim-shot.sh" "$out/gallery" "W300 D F3 W20 D R W20 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/panes" "W300 F3 D S W20 D S W20 D S W20 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/panes-back" "W300 F3 D S W20 D W700 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/panes-play" "W300 F3 S F1 P W200 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/cassette" "W300 F3 P W200 M W100 D M W100 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/cassette-pause" "W300 F3 P W200 P W100 M W100 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/nothing-plays" "W300 M W100 D" >/dev/null
for f in gallery/03 panes/04 panes-back/03 panes-play/01 cassette/02 cassette-pause/01 nothing-plays/01; do
  [ -f "$out/$f.png" ] || { echo "$f.png is missing" >&2; exit 1; }
done
[ "$(areadiff gallery/01 gallery/02)" != 0 ] || { echo "the gallery did not move" >&2; exit 1; }
grep -q '^pod: deck pane 1' "$out/panes/sim.log" || { echo "SELECT did not open Tracks" >&2; exit 1; }
grep -q '^pod: deck pane 2' "$out/panes/sim.log" || { echo "SELECT did not open Stats" >&2; exit 1; }
[ "$(grep '^pod: deck pane' "$out/panes/sim.log" | tail -1)" = 'pod: deck pane 0' ] || { echo "SELECT did not go back to Details" >&2; exit 1; }
[ "$(areadiff panes/01 panes/02)" != 0 ] || { echo "Tracks looks like Details" >&2; exit 1; }
[ "$(areadiff panes/02 panes/03)" != 0 ] || { echo "Stats looks like Tracks" >&2; exit 1; }
[ "$(areadiff panes/01 panes/04)" = 0 ] || { echo "the panes did not come back to Details" >&2; exit 1; }
[ "$(areadiff panes-back/01 panes-back/03)" = 0 ] || { echo "a pane did not go back after 5 s" >&2; exit 1; }
grep -q '^pod: deck play Signal / Noise from 2' "$out/panes-play/sim.log" || { echo "PLAY in Tracks did not play from the track" >&2; exit 1; }
grep -q '^pod: deck to player' "$out/cassette/sim.log" || { echo "MENU did not go to the player" >&2; exit 1; }
[ "$(areadiff cassette/01 cassette/02)" != 0 ] || { echo "the player looks like the deck" >&2; exit 1; }
[ "$(cassettediff gallery/01 cassette/01)" != 0 ] || { echo "the cassette is not shown while it plays" >&2; exit 1; }
[ "$(cassettediff cassette/01 cassette-pause/01)" != 0 ] || { echo "the cassette is not grey on pause" >&2; exit 1; }
grep -q '^pod: search ""' "$out/nothing-plays/sim.log" || { echo "MENU with nothing playing did not open the search" >&2; exit 1; }
echo "deck v2 ok"

# Player v2: SELECT steps the panes (Cover, Rating, Up next and back, with
# the dots) and one that isn't Cover goes back after 5 s. The wheel sets the
# stars in Rating, saved to the tagcache (read back after a skip away and
# back), and scrolls Up next, which lists the playlist (a mixtape of six
# tracks here). The counter dims and the cassette stops on pause, shows the dB
# while the wheel turns and follows the cue. The pane is the area at 3x of the
# cover, the stars and the list, the counter the bottom of the screen with the
# cassette.
panediff() {
  magick compare -metric AE "$out/$1.png[310x120+66+45]" "$out/$2.png[310x120+66+45]" null: 2>&1 | cut -d' ' -f1
}
counterdiff() {
  magick compare -metric AE "$out/$1.png[414x100+0+228]" "$out/$2.png[414x100+0+228]" null: 2>&1 | cut -d' ' -f1
}
playmix=$root/build/sim/simdisk/Mixes
rm -rf "$playmix"
mkdir -p "$playmix"
{
  for t in '01 Reel to Reel' '02 Chrome Tape' '03 Night Drive' '04 Analog Heart'; do
    echo "/Music/Neon Cassette/Magnetic Dawn/$t.flac"
  done
  printf '/Music/Unit Δelta/Signal - Noise/01 Carrier.flac\n/Music/Unit Δelta/Signal - Noise/02 Low Orbit.flac\n'
} >"$playmix/Six.m3u8"
"$root/scripts/sim-shot.sh" "$out/pane-up" "W300 P W200 S W50 S W20 D F1 W20 D F1 W20 D F9 W20 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/pane-up-top" "W300 P W200 S W50 S W20 D F1 W20 D B3 W20 D" >/dev/null
rm -rf "$playmix"
"$root/scripts/sim-shot.sh" "$out/pane" "W300 F2 P W200 D S W20 D S W50 D S W50 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/pane-back" "W300 F2 P W200 S W20 D W700 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/rate" "W300 F2 P W200 S F4 W20 D R W100 L W100 S W50 S W50 S W50 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/pause" "W300 F2 P W200 D P W100 D W100 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/volume" "W300 F2 P W200 F3 W10 D W200 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/cue" "W300 F2 P W200 RR W20 D" >/dev/null
for f in pane-up/04 pane-up-top/03 pane/04 pane-back/02 rate/02 pause/03 volume/02 cue/01; do
  [ -f "$out/$f.png" ] || { echo "$f.png is missing" >&2; exit 1; }
done
[ "$(grep '^pod: player pane' "$out/pane/sim.log" | cut -d' ' -f4 | tr -d '\n')" = 120 ] || { echo "SELECT did not step Rating, Up next, Cover" >&2; exit 1; }
[ "$(panediff pane/01 pane/02)" != 0 ] || { echo "Rating looks like Cover" >&2; exit 1; }
[ "$(panediff pane/02 pane/03)" != 0 ] || { echo "Up next looks like Rating" >&2; exit 1; }
[ "$(panediff pane/01 pane/04)" = 0 ] || { echo "the panes did not come back to Cover" >&2; exit 1; }
[ "$(panediff pane-back/02 pane/01)" = 0 ] || { echo "a pane did not go back after 5 s" >&2; exit 1; }
[ "$(panediff pane-back/01 pane-back/02)" != 0 ] || { echo "Rating is not shown" >&2; exit 1; }
[ "$(grep '^pod: rate' "$out/rate/sim.log" | tail -1)" = 'pod: rate 4' ] || { echo "the wheel did not rate the track" >&2; exit 1; }
[ "$(grep '^pod: player pane' "$out/rate/sim.log" | tail -1)" = 'pod: player pane 1 rating 4' ] || { echo "the rating was lost on a skip" >&2; exit 1; }
[ "$(panediff rate/01 pane/02)" != 0 ] || { echo "the stars did not change" >&2; exit 1; }
[ "$(grep '^pod: up next' "$out/pane-up/sim.log" | tr '\n' ',')" = 'pod: up next Chrome Tape,pod: up next Night Drive,pod: up next Analog Heart,' ] || { echo "Up next did not list and scroll the playlist" >&2; exit 1; }
[ "$(grep '^pod: up next' "$out/pane-up-top/sim.log" | tail -1)" = 'pod: up next Chrome Tape' ] || { echo "the wheel did not scroll Up next back" >&2; exit 1; }
[ "$(panediff pane-up/01 pane-up/02)" != 0 ] || { echo "Up next did not scroll" >&2; exit 1; }
[ "$(panediff pane-up/03 pane-up/04)" = 0 ] || { echo "Up next scrolled past its end" >&2; exit 1; }
[ "$(counterdiff pause/01 pause/02)" != 0 ] || { echo "the counter and the cassette did not change on pause" >&2; exit 1; }
[ "$(counterdiff pause/02 pause/03)" = 0 ] || { echo "the counter moved while paused" >&2; exit 1; }
[ "$(counterdiff volume/01 volume/02)" != 0 ] || { echo "the counter did not show the volume" >&2; exit 1; }
cue=$(sed -n 's/^pod: cue //p' "$out/cue/sim.log" | tail -1)
[ -n "$cue" ] && [ "$cue" -gt 4000 ] || { echo "holding >> did not cue (cue '$cue')" >&2; exit 1; }
echo "player v2 ok"

# Rescan Library keeps the play stats: the first track plays to the end (20 s),
# then the rescan exports its count to the changelog and imports it after the
# rebuild, now or at the next boot (stats still queued when the simulator
# exits are lost, so this is one run)
cl=$root/build/sim/simdisk/.pod/database_changelog.txt
"$root/scripts/sim-shot.sh" "$out/rescan" "W300 F3 P W3500 M W100 MM W100 F6 W50 D S W1500 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/rescan-boot" "W1500" >/dev/null
grep -q 'playcount="[1-9]' "$cl" || { echo "the changelog has no play count" >&2; exit 1; }
cat "$out"/rescan/sim.log "$out"/rescan-boot/sim.log | grep -q '^pod: stats imported for [1-9]' || { echo "the stats were not imported" >&2; exit 1; }
echo "stats ok"

# a finished play is a line of /.pod/plays.log, written at the USB connect
# (the storage idle flush): the header, then a 5 s play of a 20 s track as S
# and a 12 s one as L, with a Unix timestamp
log=$root/build/sim/simdisk/.pod/plays.log
rm -f "$log"
"$root/scripts/sim-shot.sh" "$out/playlog" "W300 F2 P W800 R W1800 R W300 U W200 D" >/dev/null
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
"$root/scripts/sim-shot.sh" "$out/deck-mixes" "W300 D P W200 D M W100 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/deck-mixes-jump" "W300 R W20 D L W20 D" >/dev/null
rm -rf "$mixes"
for f in deck-mixes/01 deck-mixes/02 deck-mixes/03 deck-mixes-jump/01; do
  [ -f "$out/$f.png" ] || { echo "$f.png is missing" >&2; exit 1; }
done
grep -q '^pod: deck 1/7 Ambient' "$out/deck-mixes/sim.log" || { echo "the mixes are not first in the deck" >&2; exit 1; }
grep -q '^pod: deck play mix /Mixes/Ambient.m3u8' "$out/deck-mixes/sim.log" || { echo "PLAY did not play the mix" >&2; exit 1; }
grep -q '^pod: deck 2/8 Ambient' "$out/deck-mixes/sim.log" || { echo "MENU did not go back to the mixtape" >&2; exit 1; }
grep -q '^pod: deck 3/7 Warehouse Sessions' "$out/deck-mixes-jump/sim.log" || { echo ">> did not jump over the mixes" >&2; exit 1; }
[ "$(grep '^pod: deck' "$out/deck-mixes-jump/sim.log" | tail -1)" = 'pod: deck 1/7 Ambient' ] || { echo "<< did not jump back to the mixes" >&2; exit 1; }
[ "$(areadiff deck-mixes/01 deck-mixes/02)" != 0 ] || { echo "the player looks like the deck" >&2; exit 1; }
echo "deck mixes ok"

# The current playlist is the first tape of the deck, with a tracklist card:
# nothing plays at boot so there is none, and after Signal / Noise it comes
# before Warehouse Sessions (1/6). SELECT on it opens the player without a new
# playlist. Hold SELECT on an album asks Play next / Play last, adds it and
# stays on the deck. The editor opens with hold MENU in the player: SELECT
# picks a track up and drops it, hold SELECT removes one, MENU is the player.
"$root/scripts/sim-shot.sh" "$out/queue" "W300 F3 P W200 M W100 D B9 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/queue-next" "W300 F3 P W200 M W100 F1 SS W50 D S W150 B9 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/queue-edit" "W300 F3 P W200 MM W100 D S F1 D S W50 D SS W50 D M W100 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/queue-play" "W300 F3 P W200 M W100 B9 P W100 D" >/dev/null
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
grep -q '^pod: deck play queue' "$out/queue-play/sim.log" || { echo "PLAY did not play the queue" >&2; exit 1; }
[ "$(grep -c '^pod: deck play Signal' "$out/queue-play/sim.log")" = 1 ] || { echo "the queue was replaced" >&2; exit 1; }
[ "$(areadiff queue/02 queue-play/01)" != 0 ] || { echo "the player looks like the deck" >&2; exit 1; }
echo "queue ok"

# double SELECT in the player loves the playing track (rating 10) and does it
# again to unlove (0). A heart flashes over the cover for a second, filled
# after a love and hollow after an unlove. The rating went through the
# tagcache: it is still 10 in Rating after skipping away and back. The heart
# is the 15x14 box at 3x, centred on the cover.
"$root/scripts/sim-shot.sh" "$out/love" "W300 F2 P W100 2S W20 D W200 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/unlove" "W300 F2 P W100 2S W200 2S W20 D W200 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/love-keep" "W300 F2 P W100 2S W100 R W100 L W100 S W20 D" >/dev/null
grep -q '^pod: love 10' "$out/love/sim.log" || { echo "double SELECT did not love" >&2; exit 1; }
grep -q '^pod: love 0' "$out/unlove/sim.log" || { echo "double SELECT did not unlove" >&2; exit 1; }
flashdiff() {
  magick compare -metric AE "$out/$1.png[45x42+108+81]" "$out/$2.png[45x42+108+81]" null: 2>&1 | cut -d' ' -f1
}
for f in love/01 love/02 unlove/01 unlove/02 love-keep/01; do
  [ -f "$out/$f.png" ] || { echo "$f.png is missing" >&2; exit 1; }
done
[ "$(flashdiff love/01 love/02)" != 0 ] || { echo "the heart flash is not shown" >&2; exit 1; }
[ "$(flashdiff love/01 unlove/01)" != 0 ] || { echo "the unlove flash looks like the love one" >&2; exit 1; }
[ "$(flashdiff love/02 unlove/02)" = 0 ] || { echo "the heart stayed" >&2; exit 1; }
[ "$(grep '^pod: player pane' "$out/love-keep/sim.log" | tail -1)" = 'pod: player pane 1 rating 10' ] || { echo "the love was lost on a skip" >&2; exit 1; }
echo "love ok"

# a track over 10 minutes picks up where it stopped, a short one starts at 0.
# Zero Hour (11 min, the Tape Loop tape) plays 8 s, Carrier 6 s, then Zero Hour
# again starts where it stopped and Carrier at 0. One run, because the resume
# points are tagcache updates still queued when the simulator exits.
"$root/scripts/sim-shot.sh" "$out/resume" "W300 F4 P W800 M W100 B1 P W600 M W100 F1 P W300 D M W100 B1 P W300 D" >/dev/null
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
# player shows the time left under the battery until then.
sleepmix=$root/build/sim/simdisk/Mixes
rm -rf "$sleepmix"
mkdir -p "$sleepmix"
printf '/Music/Neon Cassette/Magnetic Dawn/04 Analog Heart.flac\n/Music/Unit Δelta/Signal - Noise/01 Carrier.flac\n' >"$sleepmix/Sleep.m3u8"
export POD_SLEEP_FAST=1
"$root/scripts/sim-shot.sh" "$out/sleep" "W300 2M W100 S F7 F6 S R S W100 MM W100 MM W100 F3 S W50 D M W100 M W100 D W2000 D" >/dev/null
"$root/scripts/sim-shot.sh" "$out/sleep-album" "W300 P W100 MM W100 MM W100 F3 S S S S M W100 M W100 D W2600 D" >/dev/null
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
# the time left is the 72x27 box at 3x beside the battery
sleepdiff() {
  magick compare -metric AE "$out/$1.png[72x27+276+0]" "$out/$2.png[72x27+276+0]" null: 2>&1 | cut -d' ' -f1
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
sim search-queue "W300 P W200 M W100 2M W100 F2 S F5 S R SS W50 D"
for f in search/01 search/02 search-track/01 search-track/02 search-leave/01; do
  [ -f "$out/$f.png" ] || { echo "$f.png is missing" >&2; exit 1; }
done
grep -q '^pod: search "" 0 0 0' "$out/search/sim.log" || { echo "the search did not open empty" >&2; exit 1; }
grep -q '^pod: search "T" 3 0 1' "$out/search/sim.log" || { echo "T did not find 3 tracks and an album" >&2; exit 1; }
[ "$(areadiff search/01 search/02)" != 0 ] || { echo "the results are not shown" >&2; exit 1; }
grep -q '^pod: search pick album Tape Loop' "$out/search-album/sim.log" || { echo "SELECT did not pick the album" >&2; exit 1; }
grep -q '^pod: deck 5/5 Tape Loop' "$out/search-album/sim.log" || { echo "the deck is not on the album" >&2; exit 1; }
grep -q '^pod: search "U" 1 1 0' "$out/search-artist/sim.log" || { echo "U did not find a track and an artist" >&2; exit 1; }
grep -q '^pod: deck 4/5 Signal / Noise' "$out/search-artist/sim.log" || { echo "the deck is not on the artist's first album" >&2; exit 1; }
grep -q '^pod: search "E" 0 1 0' "$out/search-accent/sim.log" || { echo "E did not find Unit Δelta" >&2; exit 1; }
grep -q '^pod: search "CH" 1 0 0' "$out/search-track/sim.log" || { echo "CH did not narrow to a track" >&2; exit 1; }
grep -q '^pod: deck play Magnetic Dawn from 2' "$out/search-track/sim.log" || { echo "the track did not play from its place" >&2; exit 1; }
grep -q '^pod: deck 4/6 Magnetic Dawn' "$out/search-track/sim.log" || { echo "MENU did not go back to the album" >&2; exit 1; }
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
sim infinite-off "W300 F2 P W200 R R R W200 D W3000 D"
rm -f "$root"/build/sim/simdisk/.pod/.playlist_control* "$root"/build/sim/simdisk/.pod/.resume.cfg*
sim infinite "W300 MM W100 F2 S W100 D M W100 F2 P W200 R R R W200 D W3000 D"
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

# The deck reads the library once. Back from the player, the settings and the
# search it does not read it again; Stats reads its album's plays again, so a
# play shows without a reload; a new track makes the next open read again.
loads() { grep -c '^pod: deck load' "$out/$1/sim.log" || true; }
rm -f "$root"/build/sim/simdisk/.pod/.playlist_control* "$root"/build/sim/simdisk/.pod/.resume.cfg*
sim loads-stats "W300 F3 S S W20 D P W3500 M W100 S S W20 D"
sim loads-settings "W300 MM W100 M W100 D"
[ "$(loads loads-stats)" = 1 ] || { echo "the deck read the library again after the player" >&2; exit 1; }
[ "$(loads loads-settings)" = 1 ] || { echo "the deck read the library again after the settings" >&2; exit 1; }
plays=$(grep '^pod: deck stats Signal / Noise played' "$out/loads-stats/sim.log" | sed 's/.* //')
[ "$(wc -l <<<"$plays")" = 2 ] && [ "$(tail -1 <<<"$plays")" -gt "$(head -1 <<<"$plays")" ] \
  || { echo "Stats did not show the play" >&2; exit 1; }
cp "$root/build/sim/simdisk/Music/Unit Δelta/Tape Loop/01 Zero Hour.flac" \
   "$root/build/sim/simdisk/Music/Unit Δelta/Tape Loop/02 Zero Hour Again.flac"
sim loads-new-boot "W1500"
sim loads-new "W1500 MM W100 M W100 D"
rm -f "$root/build/sim/simdisk/Music/Unit Δelta/Tape Loop/02 Zero Hour Again.flac"
sim loads-gone-boot "W1500"
grep '^pod: deck load' "$out/loads-new/sim.log" | tail -1 | grep -q ' 15 tracks' \
  || { echo "the deck did not read the new track" >&2; exit 1; }
echo "deck loads once ok"
