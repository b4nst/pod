#!/usr/bin/env bash
# Build the simulator from a patched tree (default build/src), install the
# theme and a test library.
#   scripts/sim-setup.sh [rockbox-src [target]]
# The target is ipodmini2g (build/sim, the default) or ipod6g (build/sim-ipod6g).
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
src=$(cd "${1:-$root/build/src}" && pwd)
target=${2:-ipodmini2g}
sim=$root/build/sim
[ "$target" = ipodmini2g ] || sim=$root/build/sim-$target
if [ ! -f "$sim/Makefile" ]; then
  mkdir -p "$sim"
  (cd "$sim" && "$src/tools/configure" --target="$target" --type=s </dev/null >configure.log)
fi
make -C "$sim" -j"$(sysctl -n hw.ncpu 2>/dev/null || nproc)" >"$sim/make.log" 2>&1 || { tail -30 "$sim/make.log"; exit 1; }
make -C "$sim" install >/dev/null 2>&1
"$root/scripts/build-theme.sh" "$src" "$target" >/dev/null
cp -R "$root/build/theme/.pod/." "$sim/simdisk/.pod/"
[ -d "$sim/simdisk/Music" ] || python3 "$root/scripts/mk-test-library.py" "$sim/simdisk"
# the covers SYNC would write: one album gets a cover.jpg
art="$sim/simdisk/Music/Unit Δelta/Signal - Noise/cover.jpg"
[ -f "$art" ] || magick -size 200x200 xc:white -fill black -draw "rectangle 50,50 150,150" "$art"
cargo run -q --manifest-path "$root/app/Cargo.toml" -p pod-core --example covers -- "$sim/simdisk" "$target"
echo "simulator ready"
