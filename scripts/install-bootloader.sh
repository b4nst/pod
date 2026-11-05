#!/usr/bin/env bash
# Install the pod bootloader on a connected iPod mini (it replaces the stock
# Rockbox bootloader; the Apple firmware is kept). Run after scripts/build.sh.
#   scripts/install-bootloader.sh ipodmini1g|ipodmini2g [/dev/diskN]
# Only one iPod may be connected when no disk is given.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
t=${1:?usage: install-bootloader.sh ipodmini1g|ipodmini2g [/dev/diskN]}
boot=$root/dist/pod-bootloader-$t.ipod
[ -f "$boot" ] || { echo "missing $boot, run scripts/build.sh $t" >&2; exit 1; }
disk=${2:-}
if [ -n "$disk" ] && command -v diskutil >/dev/null; then
  diskutil unmountDisk "$disk"
fi
# ipodpatcher writes the raw disk, so it needs root
sudo "$root/dist/ipodpatcher" ${disk:+"$disk"} -a "$boot"
