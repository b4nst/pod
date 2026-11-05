#!/usr/bin/env bash
# Build the simulator from build/src, install the theme and a test library.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
sim=$root/build/sim
if [ ! -f "$sim/Makefile" ]; then
  mkdir -p "$sim"
  (cd "$sim" && "$root/build/src/tools/configure" --target=ipodmini2g --type=s </dev/null >configure.log)
fi
make -C "$sim" -j"$(sysctl -n hw.ncpu 2>/dev/null || nproc)" >"$sim/make.log" 2>&1 || { tail -30 "$sim/make.log"; exit 1; }
make -C "$sim" install >/dev/null 2>&1
"$root/scripts/build-theme.sh" "$root/build/src" >/dev/null
cp -R "$root/build/theme/.pod/." "$sim/simdisk/.pod/"
[ -d "$sim/simdisk/Music" ] || python3 "$root/scripts/mk-test-library.py" "$sim/simdisk"
echo "simulator ready"
