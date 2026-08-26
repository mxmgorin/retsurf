#!/usr/bin/env bash
# What does retsurf need to browse a page, and does it survive a hard ceiling?
#
#   tools/memory-gate.sh <url> [mem_max] [swap_max] [seconds]
#
# With no ceiling it reports peak RSS. With one it runs the binary inside a
# cgroup scope and reports how the working set splits between RAM and swap —
# which is the number that matters for a 128 MB handheld, because the working
# set barely moves with the ceiling; only the swap share does.
#
#   tools/memory-gate.sh https://example.com            # peak RSS, no limit
#   tools/memory-gate.sh https://example.com 80M 250M    # the gate
#
# Headless on its own X server at the device's 640x480, on the `embedded`
# profile, against a throwaway data dir. Reading the result on a desktop build:
# x86_64 pointers, `webgl` on and llvmpipe in RSS all make it worse than an
# armv7 device build, so fitting here is a floor, not a forecast. What does not
# transfer at all is the stall figure — swap is NVMe here and an SD card there.
set -uo pipefail

url=${1:?usage: memory-gate.sh <url> [mem_max] [swap_max] [seconds]}
mem=${2:-}
swap=${3:-250M}
secs=${4:-120}

repo=$(cd "$(dirname "$0")/.." && pwd)
bin=$repo/target/release/retsurf
[ -x "$bin" ] || { echo "no release build at $bin (cargo build --release)" >&2; exit 1; }

display=:83
profile=$(mktemp -d)
trap 'rm -rf "$profile"' EXIT

cat > "$profile/config.toml" <<EOF
[browser]
home_page = "$url"

[performance]
memory_profile = "embedded"
EOF

Xvfb $display -screen 0 640x480x24 +extension GLX +extension RANDR >/dev/null 2>&1 &
xvfb=$!
trap 'kill $xvfb 2>/dev/null; rm -rf "$profile"' EXIT
sleep 2

# LIBGL_ALWAYS_SOFTWARE: GL renders into a buffer XGetImage cannot read otherwise,
# which turns every screenshot into a uniform 286-byte file.
run=(env -u WAYLAND_DISPLAY DISPLAY=$display SDL_VIDEODRIVER=x11
     LIBGL_ALWAYS_SOFTWARE=1 RETSURF_DATA_DIR="$profile" RUST_LOG=warn "$bin")

if [ -n "$mem" ]; then
  # The X server stays outside the budget: on the device SDL talks to the panel.
  systemd-run --user --scope --quiet \
    -p MemoryMax="$mem" -p MemorySwapMax="$swap" \
    "${run[@]}" > "$profile/app.log" 2>&1 &
else
  "${run[@]}" > "$profile/app.log" 2>&1 &
fi

peak_rss=0 peak_mem=0 peak_swap=0 peak_stall=0 cg=""
for _ in $(seq 1 "$secs"); do
  pid=$(pgrep -f "target/release/retsurf" | head -1)
  [ -z "$pid" ] && break
  rss=$(ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ')
  [ -n "$rss" ] && [ "$rss" -gt "$peak_rss" ] && peak_rss=$rss
  if [ -n "$mem" ]; then
    [ -z "$cg" ] && cg="/sys/fs/cgroup$(awk -F: '/^0::/{print $3}' "/proc/$pid/cgroup" 2>/dev/null)"
    if [ -f "$cg/memory.current" ]; then
      m=$(cat "$cg/memory.current" 2>/dev/null || echo 0)
      w=$(cat "$cg/memory.swap.current" 2>/dev/null || echo 0)
      s=$(awk '/^full/{print $2}' "$cg/memory.pressure" 2>/dev/null | cut -d= -f2 | head -1)
      [ "${m:-0}" -gt "$peak_mem" ] && peak_mem=$m
      [ "${w:-0}" -gt "$peak_swap" ] && peak_swap=$w
      awk -v a="${s:-0}" -v b="$peak_stall" 'BEGIN{exit !(a>b)}' && peak_stall=${s:-0}
    fi
  fi
  sleep 1
done

shot=${TMPDIR:-/tmp}/retsurf-memory-gate.png
pid=$(pgrep -f "target/release/retsurf" | head -1)
if [ -n "$pid" ]; then
  verdict="alive after ${secs}s"
  # Survival proves nothing on its own — check the page actually drew.
  import -display $display -window root "$shot" 2>/dev/null && verdict="$verdict, screenshot at $shot"
  kill -TERM "$pid" 2>/dev/null
else
  verdict="DIED before ${secs}s"
fi
sleep 1
pkill -f "target/release/retsurf" 2>/dev/null

echo "page: $url"
echo "verdict: $verdict"
if [ -n "$mem" ]; then
  echo "ceiling: $mem RAM + $swap swap"
  echo "peak: $((peak_mem/1048576)) MB resident + $((peak_swap/1048576)) MB swapped" \
       "= $(((peak_mem+peak_swap)/1048576)) MB working set"
  echo "worst memory.pressure full avg10: ${peak_stall}%"
else
  echo "peak RSS: $((peak_rss/1024)) MB (no ceiling)"
fi
