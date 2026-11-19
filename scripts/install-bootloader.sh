#!/usr/bin/env bash
# Install the pod bootloader on a connected iPod (it replaces the stock
# Rockbox bootloader; the Apple firmware is kept). Run after scripts/build.sh.
#   scripts/install-bootloader.sh ipodmini1g|ipodmini2g [/dev/diskN]
#   scripts/install-bootloader.sh ipod6g [single]
# Only one iPod may be connected when no disk is given. The Classic goes to
# DFU mode while the script waits; single is for a Classic without Apple's NOR
# boot (see docs/DEVELOPMENT.md).
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
usage="usage: install-bootloader.sh ipodmini1g|ipodmini2g [/dev/diskN] | ipod6g [single]"
t=${1:?$usage}
if [ "$t" = ipod6g ]; then
  case ${2:-} in "") dfu=$root/dist/pod-installer-$t.dfu ;;
    single) dfu=$root/dist/pod-installer-$t-single.dfu ;;
    *) echo "$usage" >&2; exit 1 ;;
  esac
  [ -f "$dfu" ] || { echo "missing $dfu, run scripts/build.sh $t" >&2; exit 1; }
  mks=$root/dist/mks5lboot
  # libusb talks to the device in DFU mode, which Linux lets only root do
  [ "$(uname)" = Linux ] && sudo=sudo || sudo=
  if [ "$(uname)" = Darwin ]; then
    # macOS's device agents load Apple's recovery image into an iPod in DFU
    # (WTF mode), and ours then hangs it: they are paused until the script ends
    agents=$(pgrep -f 'MobileDeviceUpdater|AMPDeviceDiscoveryAgent|AMPDevicesAgent|amp.devicesui|AMPLibraryAgent' || true)
    muxd=$(pgrep -x usbmuxd || true)
    trap '[ -z "$agents" ] || kill -CONT $agents; [ -z "$muxd" ] || sudo kill -CONT $muxd' EXIT
    [ -z "$agents" ] || kill -STOP $agents
    [ -z "$muxd" ] || sudo kill -STOP $muxd
  fi
  echo "Hold MENU+SELECT, through the reset, until the screen stays black (about 15 s)."
  while :; do
    scan=$($sudo "$mks" --dfuscan 2>&1 || true)
    case $scan in
      *"mode: WTF"*) echo "the iPod left DFU for WTF mode: hold MENU+SELECT again" >&2; exit 1 ;;
      *"mode: DFU"*"state: 2"*) break ;;
    esac
    sleep 1
  done
  echo "In DFU, you can let go."
  $sudo "$mks" --dfusend "$dfu"
  echo "Sent. pod's bootloader shows the pod splash at boot; \"Rockbox boot loader\" means nothing changed."
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
