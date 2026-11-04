# pod

A stripped-down Rockbox for the iPod mini (1G and 2G) in a cassette futurism
theme. bΔnst ⧉ design.

![screens](docs/screens.png)

It boots into your music library and has a cassette-deck player. There is one
settings menu, and you open it by holding MENU.

## Controls

| Where        | Button            | Action                                    |
|--------------|-------------------|-------------------------------------------|
| Library      | wheel / SELECT    | move / open, play                         |
|              | MENU              | back (does nothing at the library root)   |
|              | hold MENU         | settings                                  |
|              | PLAY              | now playing (resumes the last playlist)   |
|              | hold SELECT       | play shuffled / play next / play last / info |
| Player       | wheel             | volume (the counter shows the dB value)   |
|              | PREV / NEXT       | skip, hold to seek                        |
|              | MENU or SELECT    | back to the library                       |
|              | hold MENU         | settings                                  |
|              | hold SELECT       | track info / reshuffle                    |
| Equalizer    | wheel             | gain of the selected band (±12 dB)        |
|              | PREV / NEXT       | select band (60, 250, 1K, 4K, 12K)        |
|              | SELECT            | next preset: FLAT BASS SUB WARM VOCAL AIR LOUD |
|              | PLAY              | EQ on / bypass                            |
|              | MENU              | done (saved)                              |

Settings menu: Equalizer, Shuffle, Repeat, Backlight, Time & Date, Rescan
Library, System (battery, disk, version).

## Install

1. Install the Rockbox bootloader once with the official installer or
   `ipodpatcher`. This repo doesn't build the bootloader.
2. Build (below) or take `dist/pod-ipodmini2g.zip` (or `-1g`) and unzip it at
   the root of the iPod so that `.rockbox/` sits at the top level.
3. Eject and reboot. On the first boot pod scans the library, then restarts by
   itself once so Rockbox can commit the database. After that it boots straight
   into the library. New music is picked up automatically at boot.

## Build

Everything comes from the devenv shell (ARM GCC, host compilers, SDL2,
ImageMagick, fonts):

```sh
devenv shell -- scripts/build.sh            # both generations -> dist/
devenv shell -- scripts/build.sh ipodmini2g # just one
```

`build.sh` clones `$ROCKBOX_SRC` at `ROCKBOX_REV`, applies `patches/`,
builds without plugins, and repacks `rockbox.zip`. It drops the stock themes,
fonts, icons and languages other than English, and adds the theme.

### Simulator

```sh
devenv shell -- scripts/sim-setup.sh   # builds build/sim from build/src, adds a test library
devenv shell -- scripts/sim-shot.sh build/shots/x "W200 D S W60 D MM W80 D"
```

`sim-shot.sh` runs the simulator headless and saves screendumps as PNG. The
script is a list of steps: `W<ticks>` wait (100 ticks = 1 s), `D` screendump,
`M S P L R` press MENU/SELECT/PLAY/PREV/NEXT, a doubled letter is a long
press, and `F<n>`/`B<n>` turn the wheel.

## Layout

- `patches/`: Rockbox changes, applied in order:
  1. simulator scripted input (simulator only)
  2. `%pW` skin tag: reel animation frame, advances every 200 ms of playback
  3. the EQ screen (`apps/gui/pod_eq.c`): five DSP bands pinned to a low
     shelf, three peaks and a high shelf. The plot is the actual biquad
     magnitude response.
  4. navigation: the library is the root, MENU is back, hold MENU opens the
     settings menu, the context menus are cut down, the first scan reboots
     by itself
  5. full-screen splash, codecs: MP3, AAC/ALAC, FLAC, Vorbis, Opus, WAV, AIFF,
     WavPack, APE
- `tools/mktheme.py` + `tools/canvas.py`: draw every bitmap (splash, cassette,
  reels, meters) and write the WPS/SBS from the same layout constants
- `scripts/build-theme.sh`: rasterizes JetBrains Mono and Orbitron to Rockbox
  fonts and runs the generator
- `theme/`: static files: default `config.cfg`, `themes/pod.cfg`, library
  menu (`tagnavi_user.config`)

### Working on the patches

`build/src` is a working clone on branch `pod`. Edit and commit there, then
re-export:

```sh
rm patches/*.patch
git -C build/src format-patch --binary -o "$PWD/patches" $(cat ROCKBOX_REV)..pod
```

## Notes

- The toolchain is nixpkgs' arm-none-eabi GCC 15. Rockbox recommends 9.5, and
  configure warns about it. The firmware builds and links, but it hasn't run
  on hardware yet.
- Peak meters stay empty in the simulator because its dummy audio driver feeds
  no samples. They should move on the device.
- Removed codecs still let those files into the library, but they won't play.
