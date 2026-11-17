#!/usr/bin/env bash
# Install the pod bootloader on a connected iPod (it replaces the stock
# Rockbox bootloader; the Apple firmware is kept). Run after scripts/build.sh.
#   scripts/install-bootloader.sh ipodmini1g|ipodmini2g [/dev/diskN]
#   scripts/install-bootloader.sh ipod6g
# Only one iPod may be connected when no disk is given. The Classic must be in
# DFU mode (see utils/mks5lboot/README in Rockbox).
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
t=${1:?usage: install-bootloader.sh ipodmini1g|ipodmini2g [/dev/diskN] | ipod6g}
if [ "$t" = ipod6g ]; then
  dfu=$root/dist/pod-installer-$t.dfu
  [ -f "$dfu" ] || { echo "missing $dfu, run scripts/build.sh $t" >&2; exit 1; }
  # libusb talks to the device in DFU mode, which Linux lets only root do
  [ "$(uname)" = Linux ] && sudo=sudo || sudo=
  $sudo "$root/dist/mks5lboot" --dfusend "$dfu"
  exit
fi
boot=$root/dist/pod-bootloader-$t.ipod
[ -f "$boot" ] || { echo "missing $boot, run scripts/build.sh $t" >&2; exit 1; }
disk=${2:-}
if [ -n "$disk" ] && command -v diskutil >/dev/null; then
  diskutil unmountDisk "$disk"
fi
# ipodpatcher writes the raw disk, so it needs root
sudo "$root/dist/ipodpatcher" ${disk:+"$disk"} -a "$boot"
