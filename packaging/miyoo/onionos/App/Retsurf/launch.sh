#!/bin/sh
# OnionOS (Miyoo Mini Plus / Flip) launcher.
sysdir=/mnt/SDCARD/.tmp_update
miyoodir=/mnt/SDCARD/miyoo
gamedir=$(cd "$(dirname "$0")" && pwd)
cd "$gamedir" || exit 1

mkdir -p "$gamedir/data" "$gamedir/downloads"

# Shipped defaults never overwrite the user's.
[ -f "$gamedir/data/config.toml" ] || cp "$gamedir/etc/config.toml" "$gamedir/data/config.toml"

# Ours first: no upstream SDL2 reaches the SigmaStar panel. `fallback` last:
# stubs an Onion card does not need (lib/README.md).
export LD_LIBRARY_PATH="$gamedir/lib:$sysdir/lib/parasyte:$sysdir/lib:$miyoodir/lib:/lib:/config/lib:/customer/lib:$gamedir/lib/fallback"
export LD_PRELOAD="$gamedir/lib/libSDL2-2.0.so.0"
export SDL_VIDEODRIVER=Mini
export EGL_VIDEODRIVER=Mini
# SDL lists its own `software` driver first, and that one never reaches the panel.
export SDL_RENDER_DRIVER="Miyoo Mini"
#export SDL_AUDIODRIVER=dummy # if the panel build's audio driver misbehaves

# The stock HOME is read-only rootfs.
export HOME="$gamedir"
export XDG_DATA_HOME="$gamedir"
export XDG_CACHE_HOME="$gamedir/data/cache"
export RETSURF_DATA_DIR="$gamedir/data"
export RETSURF_DOWNLOAD_DIR="$gamedir/downloads"
export RETSURF_PANIC_FILE="$gamedir/retsurf-panic.log"
export RETSURF_SOFTWARE=1 # no GPU on the SSD202
# The pad arrives as keys, and detection expects the driver name `mmiyoo`.
export RETSURF_KEYMAP=miyoo
# MENU is the app's: `pressMenu2Kill` is a SIGKILL, and quitting writes the session.
export RETSURF_MENU_QUIT=1
#export RETSURF_LOG_LEVEL=debug

# Servo panics when the platform store holds no CA bundle, which is every Miyoo.
export SSL_CERT_FILE="$gamedir/etc/ssl/cacert.pem"

# Onion ships no fontconfig; the template is rebaked when it or the path changes.
fonts_in="$gamedir/etc/fonts/fonts.conf.in"
fonts_conf="$gamedir/data/fonts.conf"
if [ ! -s "$fonts_conf" ] || [ "$fonts_in" -nt "$fonts_conf" ] ||
   ! grep -q "$gamedir" "$fonts_conf"; then
  sed "s|@GAMEDIR@|$gamedir|g" "$fonts_in" > "$fonts_conf"
fi
export FONTCONFIG_FILE="$fonts_conf"

# A clock behind a certificate's `notBefore` fails every https page, and the Mini
# Plus has no RTC.
NTP_PEER=pool.ntp.org
clock=ok
if [ "${RETSURF_CLOCK_FIX:-1}" != 0 ]; then
  floor=0
  # keymon rewrites system.json on any volume or brightness change, so it tracks
  # the device's last use.
  for f in /appconfigs/system.json /mnt/SDCARD/Saves/CurrentProfile \
           "$gamedir/log.txt" "$gamedir/data/session.toml" "$gamedir/retsurf"; do
    t=$(date -r "$f" +%s 2>/dev/null) || continue
    [ "$t" -gt "$floor" ] && floor=$t
  done
  if [ "$(date +%s)" -lt "$floor" ]; then
    date -s "@$floor" >/dev/null 2>&1 && clock=floor
    # No TLS, so it works from the wrong year; backgrounded while wifi associates.
    (
      timeout -t 20 ntpd -q -n -p "$NTP_PEER" >/dev/null 2>&1 &&
        echo "retsurf: clock = ntp ($(date -u))" >> "$gamedir/log.txt"
      hwclock -w -u >/dev/null 2>&1
    ) &
  fi
fi

# 128 MB of RAM against a working set of hundreds: without swap it gets killed.
ZRAM_MB=96
SWAPFILE_MB=512

# One page per fault: the default readahead bets on a fast disk, this is a card.
echo 0 > /proc/sys/vm/page-cluster 2>/dev/null
swapfile="$gamedir/data/swapfile"
swap_ready=no

# Onion brings 128 MB of its own, which is enough and not ours to switch off.
have_mb=$(awk 'NR > 1 { kb += $3 } END { print int(kb / 1024) }' /proc/swaps 2>/dev/null)
[ "${have_mb:-0}" -ge "$ZRAM_MB" ] && swap_ready=system

# zram costs no card I/O, so it goes first where a kernel has it.
if [ "$swap_ready" = no ]; then
  [ -e /dev/zram0 ] || modprobe zram 2>/dev/null
  if [ -e /dev/zram0 ] && [ -w /sys/block/zram0/disksize ] &&
     echo $((ZRAM_MB * 1024 * 1024)) > /sys/block/zram0/disksize 2>/dev/null &&
     mkswap /dev/zram0 >/dev/null 2>&1 && swapon /dev/zram0 2>/dev/null; then
    swap_ready=zram
  fi
fi

# vfat has `bmap`, so a card file swaps with no loop device. The `dd` is ~40 s, once.
if [ "$swap_ready" = no ]; then
  [ -s "$swapfile" ] || dd if=/dev/zero of="$swapfile" bs=1M count="$SWAPFILE_MB" 2>/dev/null
  mkswap "$swapfile" >/dev/null 2>&1 && swapon "$swapfile" 2>/dev/null && swap_ready=file
fi

release_swap() {
  case "$swap_ready" in
    zram) swapoff /dev/zram0 2>/dev/null ;;
    file) swapoff "$swapfile" 2>/dev/null ;;
  esac
}

{
  echo "retsurf: clock = $clock ($(date -u))"
  echo "retsurf: swap = $swap_ready"
  free 2>/dev/null || head -3 /proc/meminfo
} > "$gamedir/log.txt" 2>&1

# Not `exec`: the shell releases the swap and forwards SIGTERM, the clean shutdown.
./retsurf >> "$gamedir/log.txt" 2>&1 &
app=$!
trap 'kill -TERM "$app" 2>/dev/null' TERM INT HUP
# The first `wait` returns when the trap fires, the second on the actual exit.
wait "$app"
wait "$app" 2>/dev/null
release_swap
