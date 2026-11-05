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
  Library   wheel to move, SELECT to open or play, MENU to go back,
            PLAY for now playing, hold SELECT for play next/last/shuffled
  Player    wheel for volume, PREV/NEXT to skip (hold to seek),
            MENU or SELECT back to the library
  Anywhere  hold MENU for settings

Music
  Copy music into the Music folder in disk mode. pod scans it at boot.

The original Apple firmware is still installed: hold MENU while the iPod
starts (or switch HOLD on) to boot it.
