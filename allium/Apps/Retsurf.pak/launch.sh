#!/bin/sh
# Allium (Miyoo Mini Plus / Flip) launcher.
gamedir=$(cd "$(dirname "$0")" && pwd)
cd "$gamedir" || exit 1

mkdir -p "$gamedir/data" "$gamedir/downloads"

# Our SDL2 first, preloaded like every SDL2 port here; the libmi_* are the
# firmware's. `lib/fallback` goes last: stubs for what an Onion card carries and
# this one does not (lib/README.md).
export LD_LIBRARY_PATH="$gamedir/lib:/mnt/SDCARD/miyoo/lib:/lib:/config/lib:/customer/lib:$gamedir/lib/fallback"
export LD_PRELOAD="$gamedir/lib/libSDL2-2.0.so.0"
export SDL_VIDEODRIVER=Mini
export EGL_VIDEODRIVER=Mini
# SDL lists its own `software` driver ahead of the panel's, and what that one
# draws never reaches the screen, so name the panel's outright.
export SDL_RENDER_DRIVER="Miyoo Mini"
# WebAudio and <audio> open a device through SDL. Swap in `dummy` if the panel
# build's audio driver misbehaves — the browser runs without it.
#export SDL_AUDIODRIVER=dummy

# The stock HOME is read-only rootfs; keep every writable path on the card.
export HOME="$gamedir"
export XDG_DATA_HOME="$gamedir"
export XDG_CACHE_HOME="$gamedir/data/cache"
export RETSURF_DATA_DIR="$gamedir/data"
export RETSURF_DOWNLOAD_DIR="$gamedir/downloads"
export RETSURF_PANIC_FILE="$gamedir/retsurf-panic.log"
export RETSURF_SOFTWARE=1 # no GPU on the SSD202
# The pad arrives as key presses here, not as a controller. Named outright
# because detection keys off the driver name `mmiyoo` and this build spells
# itself `Mini`; with it, the keys answer to the `[gamepad]` bindings.
export RETSURF_KEYMAP=miyoo
# Allium keeps no kill helper of the kind OnionOS has, and does nothing with a
# bare MENU, so the key is the app's way out. Held with a pad it stays Allium's:
# brightness, volume, screenshot.
export RETSURF_MENU_QUIT=1
#export RETSURF_LOG_LEVEL=debug

# Servo verifies TLS through the platform certificate store and *panics* when it
# finds none, which is every Miyoo: the firmware carries no CA bundle. The one in
# the package is what `rustls-native-certs` reads instead.
export SSL_CERT_FILE="$gamedir/etc/ssl/cacert.pem"

# Neither firmware ships fontconfig or a single font, so the package brings both
# and this is the only configuration fontconfig will see. The path is baked in
# here because it is only known once the app is installed.
sed "s|@GAMEDIR@|$gamedir|g" "$gamedir/etc/fonts/fonts.conf.in" > "$gamedir/data/fonts.conf"
export FONTCONFIG_FILE="$gamedir/data/fonts.conf"

# 128 MB of RAM against a working set measured in hundreds: without somewhere to
# page anonymous memory the kernel kills the browser rather than swapping it.
ZRAM_MB=96
SWAPFILE_MB=256
swapfile="$gamedir/data/swapfile"
swap_ready=no

# zram costs no card I/O, so it goes first where a kernel has it. Allium's has
# neither zram nor loop, which is why the fallback below is a plain file.
[ -e /dev/zram0 ] || modprobe zram 2>/dev/null
if [ -e /dev/zram0 ] && [ -w /sys/block/zram0/disksize ]; then
  if grep -q /dev/zram0 /proc/swaps 2>/dev/null; then
    swap_ready=zram
  elif echo $((ZRAM_MB * 1024 * 1024)) > /sys/block/zram0/disksize 2>/dev/null &&
       mkswap /dev/zram0 >/dev/null 2>&1 && swapon /dev/zram0 2>/dev/null; then
    swap_ready=zram
  fi
fi

# A swapfile on the card otherwise. It can be swapped on directly even though the
# card is FAT32 — vfat implements `bmap` — so no loop device is needed, which is
# just as well because this kernel has none. Created once; the `dd` is ~20 s.
if [ "$swap_ready" = no ]; then
  if grep -q "$swapfile" /proc/swaps 2>/dev/null; then
    swap_ready=file
  else
    [ -s "$swapfile" ] || dd if=/dev/zero of="$swapfile" bs=1M count="$SWAPFILE_MB" 2>/dev/null
    mkswap "$swapfile" >/dev/null 2>&1 && swapon "$swapfile" 2>/dev/null && swap_ready=file
  fi
fi

release_swap() {
  case "$swap_ready" in
    zram) swapoff /dev/zram0 2>/dev/null ;;
    file) swapoff "$swapfile" 2>/dev/null ;;
  esac
}

{
  echo "retsurf: swap = $swap_ready"
  free 2>/dev/null || head -3 /proc/meminfo
} > "$gamedir/log.txt" 2>&1

# Not `exec`, because the swap set up above has to be released on the way out —
# so the shell stays and passes the signal on instead. SIGTERM reaching the
# browser is a clean shutdown: cookies and the open tabs are written on it.
./retsurf >> "$gamedir/log.txt" 2>&1 &
app=$!
trap 'kill -TERM "$app" 2>/dev/null' TERM INT HUP
# Twice: the first `wait` returns as soon as the trap fires, the second waits for
# the browser to finish shutting down.
wait "$app"
wait "$app" 2>/dev/null
release_swap
