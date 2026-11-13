pod
===

This iPod mini runs pod, a custom firmware by bΔnst ⧉ design.

pod is based on Rockbox (https://www.rockbox.org) and, like Rockbox, it is
free software under the GNU General Public License, version 2 or later.
You may use, study, change and share it. There is NO WARRANTY.

Source code, build instructions and updates:
  https://github.com/b4nst/pod

What is in this folder
  pod.ipod       the firmware, loaded by the pod bootloader
  docs/COPYING.txt              GNU General Public License v2
  docs/LICENSES.txt             licenses of third party code in Rockbox
  docs/OFL-JetBrainsMono.txt    font license (JetBrains Mono)
  docs/OFL-Orbitron.txt         font license (the "Deck" fonts are converted
                                from Orbitron)

Controls
  Deck      wheel for the tape, PREV/NEXT for the artist initial,
            SELECT for the next pane, PLAY to play, double PLAY to shuffle,
            hold SELECT for play next/last, double MENU to search
  Search    wheel for the letters and the results, SELECT to pick,
            NEXT/PREV between the letters and the results, MENU to delete
  Player    wheel for volume, PREV/NEXT to skip (hold to seek),
            SELECT for the next pane, PLAY to pause, double SELECT to love,
            MENU back to the deck, hold MENU for the queue
  Queue     wheel to move, SELECT to pick up and drop, hold SELECT to remove
  Deck, Queue  hold MENU for settings

Music
  Sync a folder with pod link (https://github.com/b4nst/pod/releases), or
  copy music into the Music folder in disk mode. pod scans it at boot.

The original Apple firmware is still installed: hold MENU while the iPod
starts (or switch HOLD on) to boot it.
