# pod

A stripped-down Rockbox for the iPod mini (1G and 2G) in a cassette futurism
theme. bΔnst ⧉ design.

![screens](docs/screens.png)

pod boots into a cassette deck with your albums as tapes, and the player is
the same deck in play mode. There is one settings menu. pod link, a desktop
app, installs pod and puts your music on the iPod.

## Install

pod lives in `/.pod` and boots `/.pod/pod.ipod`, so it needs its own
bootloader. The stock Rockbox bootloader won't find it. The Apple firmware
stays installed: hold MENU (or switch HOLD on) at power on to boot it.

pod needs a Windows formatted (FAT32) iPod: one restored on a Mac is HFS+,
and pod link says so.

### With pod link

![pod link](docs/podlink.png)

Download pod link (macOS, Linux, Windows) from the
[latest release](https://github.com/b4nst/pod/releases/latest)
(`podlink-<os>`), plug the iPod in, and go through the screens in order:

1. UPGRADE copies the firmware into `/.pod`. It keeps your settings and can
   remove a stock Rockbox install.
2. FLASH installs the bootloader, once per iPod. It asks for your password,
   because it writes the firmware partition directly (macOS: the system
   prompt, Linux: pkexec, Windows: UAC).
3. SYNC copies your music folder to `/Music` (see pod link below).

Eject, and the iPod boots pod. On the first boot pod scans the library, then
restarts by itself once. After that it boots straight into the deck, and new
music is picked up at boot.

### By hand

1. Download `pod-<target>.zip`, `pod-bootloader-<target>.ipod` and
   `ipodpatcher-<os>` from the
   [latest release](https://github.com/b4nst/pod/releases/latest).
2. Put the iPod in disk mode and unzip `pod-ipodmini1g.zip` (or `-2g`) at its
   root so that `.pod/` sits at the top level. If you're upgrading from
   Rockbox, delete `.rockbox/`. Unzipping replaces `config.cfg`, which resets
   the settings.
3. Install the bootloader once per device with `ipodpatcher`, see
   [Install the bootloader by hand](docs/DEVELOPMENT.md#install-the-bootloader-by-hand).
4. Eject and reboot.

Copy music into `/Music` in disk mode and pod scans it at boot.

## Using pod

### The deck

![deck](docs/deck.png)

The deck is home. Each album is a tape, by artist then title, behind the
queue and the mixtapes. The wheel changes tape, and `<<` and `>>` jump to the
previous or next artist initial. The gallery shows the cover of the current
album and its neighbours, with a generated card for an album that has none.
SELECT steps through three panes beside the bargraph: Details, the Tracks
(the wheel moves through them) and the Stats. PLAY plays the album from track
1, or from the selected track, and opens the player. A double PLAY shuffles.
Hold SELECT adds the album to the queue, after the playing track or at the
end. Hold MENU opens the settings.

### Search

A double MENU on the deck searches. Turn the wheel to a letter and press
SELECT to add it. The tracks, artists and albums with a word that starts with
what you typed are listed under the letters, ignoring case and accents. An
album puts the deck on its tape, an artist on its first album, and a track
plays its album from that track and opens the player.

### The player

The player is the deck on the playing album: a bargraph lit up to the cue
position (the volume while the wheel turns), the battery, the time left of the
sleep timer, and a cassette whose hubs turn while it plays. Below it, the
track number and title and the elapsed time with the time left beside it.
Paused, the digits and the cassette are grey and still.

SELECT steps through three panes: the Cover, the Rating and Up next. In
Rating the wheel sets the stars in half steps, and the number of plays shows
under them. Up next lists the tracks after the playing one, and the wheel
scrolls it. A double SELECT loves the track, which sets its rating to five
stars, and flashes a heart.

### The queue

The queue is the current playlist. It is the first tape of the deck whenever
there is one, also after a reboot before anything plays. In the player, hold
MENU opens it as a list of titles: SELECT picks a track up, the wheel moves
it and SELECT drops it. Hold SELECT removes it.

### Settings

Hold MENU on the deck or the queue (in the player, hold MENU opens the queue first).

- Equalizer: five bands, from 60 Hz to 12 kHz, and seven presets.
- Repeat.
- Infinite keeps the music going: when the queue runs out and Repeat is off,
  up to 10 tracks that fit the last one (genre, year, another artist) join the
  end of the queue. Off by default.
- Sleep cycles Off, 15, 30, 60 min and Album with each SELECT. The player shows
  the time left beside the battery (`Z 27` minutes, `Z 27S` in the last
  minute, `Z ALB` until the album ends). The last 10 s fade out, then it
  pauses.
- Backlight.
- Time & Date.
- Rescan Library rebuilds the library and keeps the play counts and ratings.
- System: battery, disk, version.

## Controls

| Where        | Button            | Action                                    |
|--------------|-------------------|-------------------------------------------|
| Deck (home)  | wheel             | previous / next tape (queue, mixtapes, then albums) |
|              | << / >>           | previous / next artist initial (hold repeats) |
|              | wheel (Tracks)    | previous / next track                     |
|              | SELECT            | next pane: Details, Tracks, Stats (a mix or the queue: Details only; back to Details after 5 s) |
|              | PLAY              | play the album or mixtape from track 1 (from the selected track in Tracks), open the player (the queue tape: open the player) |
|              | double PLAY       | play it shuffled                          |
|              | hold SELECT       | on an album or mixtape: play next / play last |
|              | MENU              | the player when something plays, else search |
|              | double MENU       | search                                    |
|              | hold MENU         | settings                                  |
| Search       | wheel             | on the strip: A-Z, 0-9, space. In the results: move |
|              | SELECT            | on the strip: add the letter. In the results: pick |
|              | PLAY              | in the results: pick                      |
|              | >> / <<           | into the results / back to the strip      |
|              | MENU              | in the results: back to the strip; on the strip: delete the last letter (none: back to the deck) |
|              | hold MENU         | back to the deck                          |
|              | hold SELECT       | on a track, with something playing: add it at the end of the queue |
| Player       | wheel             | volume (the counter shows the dB value); in Rating the stars, in Up next the list |
|              | SELECT            | next pane: Cover, Rating, Up next (back to Cover after 5 s) |
|              | PLAY              | pause / resume                            |
|              | hold PLAY         | stop and leave the player                 |
|              | PREV / NEXT       | skip, hold to cue (SEEK)                  |
|              | MENU              | the deck, on the playing album            |
|              | hold MENU         | the queue                                 |
|              | double SELECT     | love / unlove the track (heart)           |
|              | hold SELECT       | track info / reshuffle                    |
| Queue        | wheel             | move                                      |
|              | SELECT            | pick the track up, SELECT again drops it  |
|              | hold SELECT       | remove the track (picked up: let go)      |
|              | MENU              | back to the player                        |
|              | hold MENU         | settings                                  |
| Equalizer    | wheel             | gain of the selected band (±12 dB)        |
|              | PREV / NEXT       | select band (60, 250, 1K, 4K, 12K)        |
|              | SELECT            | next preset: FLAT BASS SUB WARM VOCAL AIR LOUD |
|              | PLAY              | EQ on / bypass                            |
|              | MENU              | done (saved)                              |

## pod link

SYNC compares a folder with `/Music` and copies what's new or changed.
MIRROR also deletes music that isn't in the folder anymore. FORMAT can convert
lossless files (FLAC, WAV, AIFF, ALAC) to Ogg Vorbis (OGG Q3, Q5 or Q7, about
110, 160 or 220 kbit/s) to fit more music: tags, ReplayGain and cover art are
kept, files above 48 kHz are resampled to 44.1 kHz, and everything else is
copied as it is. Changing the FORMAT replaces only the files it affects.

SCAN first imports the iPod's play log into a history kept per iPod on the
computer, and backs up the play counts and ratings.

SYNC also writes:

- the mixtapes to `/Mixes`, from the listening history, at most 100 tracks each
  and never a track you often skip: Heavy Rotation (most played in the last 30
  days), Forgotten Gems (played 5+ times, not for 180 days), Never Played (a
  different pick every day), Mornings (played 5-11 h) and Nights (played 21-4 h).
- the covers of the albums on the iPod, from the art in the tracks or a
  `cover.jpg`, `folder.jpg` or `front.jpg` in the album's folder.

## Good to know

- Unplugging the headphones pauses, and plugging them back in leaves it paused.
- pod never starts louder than −20 dB.
- A track longer than 10 minutes (a podcast, an audiobook, a mix) starts where
  you stopped it. PREV takes it back to the start.
- Every play of 3 s or more is logged with the time to `/.pod/plays.log`,
  written when the disk is up anyway, so the log never spins it up.
- Play counts are kept per file path: a renamed file starts again from zero,
  and so does a converted one.
- Some codecs are removed. Those files still get into the library, but they
  won't play.

## Development

Building pod, the patches and the agent loop are in
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

## License

pod is Rockbox with patches, so it is GPLv2 or later like Rockbox (see
`/.pod/docs/COPYING.txt` on the device). pod link contains code ported from
Rockbox's ipodpatcher and is GPLv2 or later too. The fonts are under the SIL Open
Font License (`firmware/licenses/`).
