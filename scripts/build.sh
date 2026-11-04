#!/usr/bin/env bash
# Build pod for one or both iPod mini generations.
#   devenv shell -- scripts/build.sh [ipodmini2g|ipodmini1g|all]
# Output: dist/pod-<target>.zip, to unzip at the root of the iPod.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
targets=${1:-all}
[ "$targets" = all ] && targets="ipodmini2g ipodmini1g"
rev=$(cat "$root/ROCKBOX_REV")
src=$root/build/rockbox

# pristine Rockbox at the pinned revision with the pod patches on top
rm -rf "$src"
git clone -q --shared "${ROCKBOX_SRC:?}" "$src"
git -C "$src" checkout -q -b pod "$rev"
git -C "$src" -c user.name=pod -c user.email=pod@localhost am -q "$root"/patches/*.patch

mkdir -p "$root/dist"
for t in $targets; do
  b=$root/build/$t
  rm -rf "$b"; mkdir -p "$b"
  (
    cd "$b"
    "$src/tools/configure" --target="$t" --type=n </dev/null >configure.log
    # no plugins: nothing on the device launches them
    sed -i.bak 's/^export ENABLEDPLUGINS=yes/export ENABLEDPLUGINS=no/' Makefile
    make -j"$(sysctl -n hw.ncpu 2>/dev/null || nproc)" >make.log 2>&1 \
      || { tail -30 make.log; exit 1; }
    make zip >zip.log 2>&1 || { tail -30 zip.log; exit 1; }
  )

  # the theme needs convbdf, which the build above just compiled
  "$root/scripts/build-theme.sh" "$src" "$root/build/unused-splash.bmp" \
    "$root/build/unused-usb.bmp" >/dev/null

  stage=$b/stage
  rm -rf "$stage"; mkdir -p "$stage"
  (cd "$stage" && unzip -q "$b/rockbox.zip")
  rb=$stage/.rockbox
  # drop the stock themes and assets, keep codecs, langs and system files
  rm -rf "$rb"/{wps,themes,backdrops,icons,eqs,docs,rocks,fonts,debug,viewers.config}
  find "$rb/langs" -type f ! -name english.lng -delete
  mkdir -p "$rb/fonts"
  cp -R "$root/build/theme/.rockbox/." "$rb/"
  rm -f "$root/dist/pod-$t.zip"
  (cd "$stage" && zip -qr "$root/dist/pod-$t.zip" .rockbox)
  echo "dist/pod-$t.zip ($(du -h "$root/dist/pod-$t.zip" | cut -f1))"
done
