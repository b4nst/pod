## Install with pod link

Download pod link for your OS (`podlink-macos-arm64.zip`,
`podlink-linux-x86_64`, `podlink-windows-x86_64.exe`), plug the iPod in, then
run UPGRADE, FLASH and SYNC. It downloads the firmware from this release.

The binaries are not notarized. macOS: unzip, then right-click
`pod link.app` > Open the first time (or
`xattr -dr com.apple.quarantine "pod link.app"`). Windows: More info > Run
anyway.

## Install by hand

Pick the files for your iPod mini: `1g` (first generation) or `2g` (second
generation).

1. Put the iPod in disk mode and unzip `pod-ipodmini1g.zip` (or `-2g`) at its
   root so that `.pod/` sits at the top level. Upgrading from Rockbox? Delete
   `.rockbox/`.
2. Once per device, install the bootloader. It writes the raw disk, so it
   needs root:

   ```sh
   diskutil list   # macOS: find the iPod, e.g. /dev/disk5
   diskutil unmountDisk /dev/disk5
   xattr -d com.apple.quarantine ipodpatcher-macos-arm64
   chmod +x ipodpatcher-macos-arm64
   sudo ./ipodpatcher-macos-arm64 /dev/disk5 -a pod-bootloader-ipodmini1g.ipod
   ```

   On Linux use `ipodpatcher-linux-x86_64` with the iPod's block device
   (e.g. `/dev/sdb`).
3. Eject and reboot. Hold MENU (or switch HOLD on) at power on to boot the
   Apple firmware.

To upgrade, unzip the new `pod-<target>.zip` over `.pod/`. That resets the
settings. The bootloader stays.

pod is Rockbox with patches, GPLv2 or later. `ipodpatcher` is built from the
same Rockbox revision (`utils/ipodpatcher`).
