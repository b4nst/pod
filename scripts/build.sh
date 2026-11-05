#!/usr/bin/env bash
# Build pod for one or both iPod mini generations.
#   devenv shell -- scripts/build.sh [ipodmini2g|ipodmini1g|ipodpatcher|all]...
# Rockbox is the firmware/rockbox submodule; its checked out commit is the
# base the patches apply to.
# Output:
#   dist/pod-<target>.zip             firmware, to unzip at the root of the iPod
#   dist/pod-bootloader-<target>.ipod bootloader, see scripts/install-bootloader.sh
#   dist/ipodpatcher                  host tool that installs the bootloader
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
targets=${*:-all}
[ "$targets" = all ] && targets="ipodmini2g ipodmini1g ipodpatcher"
fw=$root/firmware
upstream=$fw/rockbox
[ -e "$upstream/.git" ] || git -C "$root" submodule update --init firmware/rockbox
rev=$(git -C "$upstream" rev-parse HEAD)
src=$root/build/rockbox
jobs=$(sysctl -n hw.ncpu 2>/dev/null || nproc)
# shown under System > Version and in the bootloader
export VERSION="pod-$(git -C "$root" describe --always --dirty --tags)"

# pristine Rockbox at the pinned revision with the pod patches on top. The
# clone borrows the submodule's objects, so the detached commit is there.
rm -rf "$src"
git clone -q --shared --no-checkout "$upstream" "$src"
git -C "$src" checkout -q -b pod "$rev"
git -C "$src" -c user.name=pod -c user.email=pod@localhost am -q "$fw"/patches/*.patch

mkdir -p "$root/dist"
for t in $targets; do
  [ "$t" = ipodpatcher ] && continue
  b=$root/build/$t
  rm -rf "$b" "$b-boot"; mkdir -p "$b" "$b-boot"
  (
    cd "$b"
    "$src/tools/configure" --target="$t" --type=n </dev/null >configure.log
    # no plugins: nothing on the device launches them
    sed -i.bak 's/^export ENABLEDPLUGINS=yes/export ENABLEDPLUGINS=no/' Makefile
    make -j"$jobs" >make.log 2>&1 || { tail -30 make.log; exit 1; }
    make zip >zip.log 2>&1 || { tail -30 zip.log; exit 1; }
  )
  (
    cd "$b-boot"
    "$src/tools/configure" --target="$t" --type=b </dev/null >configure.log
    make -j"$jobs" >make.log 2>&1 || { tail -30 make.log; exit 1; }
  )
  cp "$b-boot/bootloader-$t.ipod" "$root/dist/pod-bootloader-$t.ipod"

  # the theme needs convbdf, which the build above just compiled
  "$root/scripts/build-theme.sh" "$src" "$root/build/unused-splash.bmp" \
    "$root/build/unused-usb.bmp" >/dev/null

  stage=$b/stage
  rm -rf "$stage"; mkdir -p "$stage"
  (cd "$stage" && unzip -q "$b/rockbox.zip")
  rb=$stage/.pod
  [ -d "$rb" ] || mv "$stage/.rockbox" "$rb"
  # drop the stock themes and assets, keep codecs, langs and system files
  rm -rf "$rb"/{wps,themes,backdrops,icons,eqs,rocks,fonts,debug,viewers.config}
  rm -f "$rb"/docs/sample.*
  find "$rb/langs" -type f ! -name english.lng -delete
  mkdir -p "$rb/fonts"
  cp -R "$root/build/theme/.pod/." "$rb/"
  # GPL text and third party notices stay in docs/, add the fonts' and ours
  cp "$fw"/licenses/*.txt "$rb/docs/"
  cp "$fw/device/README.txt" "$rb/README.txt"
  rm -f "$root/dist/pod-$t.zip"
  (cd "$stage" && zip -qr "$root/dist/pod-$t.zip" .pod)
  echo "dist/pod-$t.zip ($(du -h "$root/dist/pod-$t.zip" | cut -f1))"
done

case " $targets " in *" ipodpatcher "*) ;; *) exit 0 ;; esac
# the default macOS deployment target (10.5) predates the IOKit calls it uses
mac=; [ "$(uname)" = Darwin ] && mac=-mmacosx-version-min=12.0
make -C "$src/utils/ipodpatcher" CFLAGS="-Wall -W $mac" >"$root/build/ipodpatcher.log" 2>&1 \
  || { tail -30 "$root/build/ipodpatcher.log"; exit 1; }
cp "$src/utils/ipodpatcher/ipodpatcher" "$root/dist/ipodpatcher"
