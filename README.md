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
|              | MENU              | back (at the library root: now playing)   |
|              | << / >>           | previous / next letter (hold repeats)     |
|              | fast wheel        | shows the letter you are on               |
|              | hold MENU         | settings                                  |
|              | PLAY              | now playing (resumes the last playlist)   |
|              | hold SELECT       | play shuffled / play next / play last / info |
| Player       | wheel             | volume (the counter shows the dB value)   |
|              | SELECT            | scrub: wheel seeks (SEEK), ends after 3 s or SELECT |
|              | PREV / NEXT       | skip, hold to seek                        |
|              | MENU              | the playing track in its album            |
|              | hold MENU         | settings                                  |
|              | hold SELECT       | track info / reshuffle                    |
| Equalizer    | wheel             | gain of the selected band (±12 dB)        |
|              | PREV / NEXT       | select band (60, 250, 1K, 4K, 12K)        |
|              | SELECT            | next preset: FLAT BASS SUB WARM VOCAL AIR LOUD |
|              | PLAY              | EQ on / bypass                            |
|              | MENU              | done (saved)                              |

Settings menu: Equalizer, Shuffle, Repeat, Backlight, Time & Date, Rescan
Library, System (battery, disk, version).

Plays are logged with the time to `/.pod/plays.log` (.scrobbler.log format),
written when the disk is up anyway, so the log never spins it up.

Unplugging the headphones pauses, and plugging them back in leaves it paused.
pod never starts louder than −20 dB.

## Install

pod lives in `/.pod` and boots `/.pod/pod.ipod`, so it needs its own
bootloader. The stock Rockbox bootloader won't find it. The Apple firmware
stays installed: hold MENU (or switch HOLD on) at power on to boot it.

### With pod link

![pod link](docs/podlink.png)

pod link is a desktop app (macOS, Linux, Windows) that flashes the
bootloader, installs and upgrades the firmware, and syncs a music folder.
Download it from the
[latest release](https://github.com/b4nst/pod/releases/latest)
(`podlink-<os>`), plug the iPod in, and go through the screens in order:

1. UPGRADE copies the firmware into `/.pod`. It keeps your settings and can
   remove a stock Rockbox install.
2. FLASH installs the bootloader, once per iPod. It asks for your password,
   because it writes the firmware partition directly (macOS: the system
   prompt, Linux: pkexec, Windows: UAC).
3. SYNC compares a folder with `/Music` and copies what's new or changed.
   MIRROR also deletes music that isn't in the folder anymore.

Eject, and the iPod boots pod. On the first boot pod scans the library, then
restarts by itself once to commit the database. After that it boots straight
into the library, and new music is picked up at boot.

pod needs a Windows formatted (FAT32) iPod: one restored on a Mac is HFS+,
and pod link says so.

### By hand

1. Download `pod-<target>.zip`, `pod-bootloader-<target>.ipod` and
   `ipodpatcher-<os>` from the
   [latest release](https://github.com/b4nst/pod/releases/latest), or build
   them (below) into `dist/`.
2. Put the iPod in disk mode and unzip `pod-ipodmini1g.zip` (or `-2g`) at its
   root so that `.pod/` sits at the top level. If you're upgrading from
   Rockbox, delete `.rockbox/`. Unzipping replaces `config.cfg`, which resets
   the settings.
3. Install the bootloader once per device. It writes the raw disk, so it asks
   for sudo, and the terminal needs access to removable volumes:

   ```sh
   diskutil list                                          # find the iPod, e.g. disk5
   scripts/install-bootloader.sh ipodmini1g /dev/disk5
   ```

4. Eject and reboot.

## Build

Everything comes from the devenv shell (ARM GCC, host compilers, SDL2,
ImageMagick, fonts):

```sh
devenv shell -- scripts/build.sh                        # everything -> dist/
devenv shell -- scripts/build.sh ipodmini2g ipodpatcher # pick targets
```

Rockbox is the `firmware/rockbox` submodule, pinned to the revision the
patches apply to. Clone with `git clone --recurse-submodules` (it is shallow),
or `build.sh` initializes it. `build.sh` copies it to `build/rockbox`, applies
`firmware/patches/`, builds without plugins, and repacks `rockbox.zip`. It
drops the stock themes, fonts, icons and languages other than English, and
adds the theme.

### pod link

```sh
devenv shell -- bash -c 'cd app && cargo run -p podlink'
devenv shell -- bash -c 'cd app && cargo test'
```

`app/core` (`pod-core`) does the work without a UI: device detection,
releases, firmware install, sync, and the bootloader install ported from
ipodpatcher (`core/src/ipod.rs`). Set `POD_IPODPATCHER=dist/ipodpatcher` and
its tests also check that both produce the same bytes on a fake iPod disk.
`app/podlink` is the iced UI. To flash, it runs itself again as root with
`podlink flash-helper`. The `flash-image` example runs that path on a fake
iPod disk image (see the comment at its top).

### Simulator

```sh
devenv shell -- scripts/sim-setup.sh   # builds build/sim from build/src, adds a test library
devenv shell -- scripts/sim-shot.sh build/shots/x "W200 D S W60 D MM W80 D"
```

`sim-setup.sh` takes another patched tree as an argument, e.g. `build/rockbox`
after `build.sh`. `sim-test.sh` is the smoke test CI runs: it walks a few
screens and fails if a screendump is missing or blank.

`sim-shot.sh` runs the simulator headless and saves screendumps as PNG. The
script is a list of steps: `W<ticks>` wait (100 ticks = 1 s), `D` screendump,
`M S P L R` press MENU/SELECT/PLAY/PREV/NEXT, a doubled letter is a long
press and `2<letter>` a double tap, `F<n>`/`B<n>` turn the wheel (`f<n>`/`b<n>`:
fast), `U` plugs or unplugs USB and `H` the headphones.

## Layout

- `app/`: pod link, the desktop app (Rust, iced): `core/` and `podlink/`, the
  bundled fonts in `assets/`
- `firmware/rockbox`: Rockbox, a submodule at the pinned revision
- `firmware/patches/`: Rockbox changes, applied in order:
  1. simulator scripted input (simulator only)
  2. `%pW` skin tag: reel animation frame, advances every 200 ms of playback
  3. the EQ screen (`apps/gui/pod_eq.c`): five DSP bands pinned to a low
     shelf, three peaks and a high shelf. The plot is the actual biquad
     magnitude response.
  4. navigation: the library is the root, MENU is back (at the root: now
     playing), `<<`/`>>` do nothing in lists, hold MENU opens the settings
     menu, the context menus are cut down, the first scan reboots by itself
  5. full-screen splash, codecs: MP3, AAC/ALAC, FLAC, Vorbis, Opus, WAV, AIFF,
     WavPack, APE
  6. full-screen "DATA LINK" USB screen in place of the stock logo
  7. with the backlight off, the player redraws at most every 5 s and stops
     polling the peak meters. The mini's LCD has no sleep mode, so it would
     otherwise keep animating in a pocket.
  8. the rename: `/.pod/pod.ipod` in place of `/.rockbox/rockbox.ipod`
     (configure's default `--rbdir` and the mini's boot file), a bootloader
     that shows the Δ splash from power on, "pod" in the UI, and the source
     link and license on the System screen
  9. tagtree buffer pinned so the library labels can't go stale
  10. pauses when the headphones are unplugged and stays paused when they're
      plugged back in, starts at no more than −20 dB
  11. double tap: `BUTTON_DOUBLE` on a second press within 300 ms of a short
      tap, for keymaps that map it. Unmapped, it is a plain press.
  12. no [All tracks] / [Random] rows in the library lists, so an artist's
      albums come by year (`tagnavi_user.config`)
  13. player SELECT scrubs the track (`%pS` skin tag: SEEK in the status row),
      MENU opens the playing track in its album (`tagtree_goto_current`)
  14. library letters: `<<`/`>>` jump to the previous/next initial
      (`ACTION_TREE_LETTER_*`), a fast wheel or a jump shows the letter in a
      box over the list for 600 ms (`apps/gui/pod_letter.c`), and lists of
      more than 64 rows scroll twice as fast
  15. Rescan Library keeps the play stats: exported to the changelog before
      the rebuild, imported after
  16. play log: every play of 3 s or more is a line of `/.pod/plays.log`
      (Audioscrobbler 1.1), kept in RAM and written by the storage idle
      callback (`apps/pod_playlog.c`)
- `firmware/tools/mktheme.py` + `firmware/tools/canvas.py`: draw every bitmap (splash, USB
  screen, cassette, reels, meters) and write the WPS/SBS from the same layout
  constants. Levels are ink (0 = bare panel, 3 = full ink): dark on light,
  because a dark background shows up as a frame on the backlit LCD.
- `scripts/build-theme.sh`: rasterizes JetBrains Mono and Orbitron to Rockbox
  fonts and runs the generator. Orbitron's license reserves its name for
  modified versions, so the converted fonts are called Deck.
- `scripts/install-bootloader.sh`: installs the pod bootloader with ipodpatcher
- `scripts/bundle-macos.sh`: wraps the app binary in an ad-hoc signed
  `pod link.app`
- `firmware/device/README.txt`, `firmware/licenses/`: shipped in `/.pod` with the GPL text and
  Rockbox's third party notices (`/.pod/docs`)
- `firmware/theme/`: static files: default `config.cfg`, `themes/pod.cfg`, library
  menu (`tagnavi_user.config`)

### CI and releases

`.github/workflows/ci.yml` builds the firmware on Linux in the devenv shell,
runs the simulator smoke test and the app's fmt, clippy and tests, and builds
pod link and `ipodpatcher` for Linux, macOS and Windows (no `ipodpatcher` on
Windows) with the runners' own toolchains, so the binaries don't depend on
`/nix/store`. Pushing a `v*` tag also publishes a GitHub release with all of
it and `SHA256SUMS` (a tag with a `-`, like `v0.2.0-rc1`, is a prerelease). The asset names don't carry the version, so
`releases/latest/download/<name>` always points at the newest build.

### Working on the patches

`build/src` is a working clone on branch `pod` (`build.sh` leaves one in
`build/rockbox`: `git clone --no-local build/rockbox build/src`). Edit and
commit there, then re-export:

```sh
rm firmware/patches/*.patch
git -C build/src format-patch --binary --full-index --zero-commit -N \
  --no-signature -o "$PWD/firmware/patches" \
  $(git -C firmware/rockbox rev-parse HEAD)..pod
```

The flags keep the commit hashes, the series numbering, the abbreviation
length and the git version out of the files. A patch you didn't touch comes
out byte for byte the same, so a change only shows up in the patches it
touches.

To move to a newer Rockbox, rebase `pod` in `build/src` onto it, re-export the
patches, and check the submodule out at the new base:

```sh
git -C firmware/rockbox fetch --depth 1 origin <rev> && git -C firmware/rockbox checkout <rev>
```

## Agent loop

`devenv up` starts `scripts/agent-loop.sh`, which takes issues from a plan to
a merge with four Claude Code agents (`.claude/agents/`). It needs the
`claude` CLI and `gh` logged in. GitHub holds all the state: labels, and
`<!-- pod-agent ... -->` markers at the top of the agents' comments.

| Agent            | Model  | Does                                                    |
|------------------|--------|---------------------------------------------------------|
| `planner`        | Opus   | comments a plan with a definition of done, labels the issue `planned` |
| `implementer`    | Sonnet | implements it on `issue-<n>` and opens the PR, fixes red CI and review findings, rebases |
| `reviewer`       | Opus   | correctness: the plan is done and nothing breaks        |
| `style-reviewer` | Sonnet | the code, commits, patches and docs match the existing ones |

1. Label an issue `ready`. Only your own `ready` counts: the loop checks the
   issue's history for it (`POD_LOOP_APPROVER`, the repo owner by default)
   before planning, and again before opening a PR. The planner comments a
   plan, what the issue waits for (`blocked`, back to `ready` once those are
   closed) or questions (`needs-human`).
2. The implementer takes `planned` issues, one PR at a time
   (`POD_LOOP_WIP`), since patches don't merge as text.
3. Once CI is green, both reviewers comment `approve` or `changes` on the
   head commit. The implementer folds fixes into its commits or replies, and
   the reviewers look again.
4. When both approve the head commit, the loop pushes it to `main`
   fast-forward (a branch that's behind goes back for a rebase), closes the
   issue and deletes the branch.

The loop leaves an issue or a PR labeled `needs-human` alone: a planner or
implementer run that changed nothing, or a PR past `POD_LOOP_MAX_ROUNDS` (5)
fix rounds. Remove the label to hand it back. The agents work in worktrees
under `build/agents/` and log there. Their definitions are read from this
checkout, never from the branch they work on.

The agents run headless with `--permission-mode auto`
(`POD_AGENT_PERMISSION_MODE`), so what they may not do is refused, not asked.
They skip your user settings, whose `ask` rules would refuse every push, and
the loop gives each one the commands its job needs, on its own issue or PR.
Agents only comment. Labels are the loop's, from the marker on each comment,
so an agent can't mark an issue ready. The implementer pushes
its branch once and opens the PR. After that it only commits, because auto
mode won't let an agent rewrite a pushed branch, and the loop force-pushes
it with a lease on the commit it handed out. Nothing but the loop can push
`main` or merge.

## License

pod is Rockbox with patches, so it is GPLv2 or later like Rockbox (see
`/.pod/docs/COPYING.txt` on the device). pod link contains code ported from
Rockbox's ipodpatcher and is GPLv2 or later too. The fonts are under the SIL Open
Font License (`firmware/licenses/`).

## Notes

- The toolchain is nixpkgs' arm-none-eabi GCC 15. Rockbox recommends 9.5, and
  configure warns about it. It runs on a 1G mini.
- Peak meters stay empty in the simulator because its dummy audio driver feeds
  no samples. They should move on the device.
- Play counts are kept per file path: a renamed file starts again from zero.
- Removed codecs still let those files into the library, but they won't play.
