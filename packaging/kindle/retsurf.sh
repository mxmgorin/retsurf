#!/bin/sh
# Kindle launcher. The firmware is soft-float with glibc 2.20 and the binary is
# hard-float against 2.28, so it runs under the loader and libc shipped in lib/;
# --inhibit-cache keeps the system's soft-float ld.so.cache out of the search.
d=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$d/data/cache"
# Launched from a menu there is no terminal to read, so every run logs here.
exec >"$d/log.txt" 2>&1
sed "s#@GAMEDIR@#$d#" "$d/etc/fonts.conf.in" > "$d/data/fonts.conf"
[ -f "$d/data/config.toml" ] || cp "$d/etc/config.toml" "$d/data/config.toml"

export HOME="$d" XDG_DATA_HOME="$d" XDG_CACHE_HOME="$d/data/cache"
export RETSURF_DATA_DIR="$d/data" RETSURF_DOWNLOAD_DIR="$d/downloads"
export RETSURF_PANIC_FILE="$d/retsurf-panic.log" RETSURF_SOFTWARE=1
export SSL_CERT_FILE="$d/etc/cacert.pem" FONTCONFIG_FILE="$d/data/fonts.conf"
# FBInk draws the panel; SDL only hosts the window and the event loop.
export SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy

# Park the framework: pillow off, its chrome bar hanging the home app against a
# paused window manager, then the window manager paused.
fb_dump=/var/tmp/retsurf-fb.dump
# Services stopped while the browser runs, for their RAM.
toggled_services="stored webreader kfxreader kfxview todo tmd rcm archive scanner otav3 otaupd"
# A KUAL launch is a child of the framework, which sends it SIGTERM on stop.
trap "" TERM

cat /dev/fb0 >"$fb_dump"
lipc-set-prop com.lab126.pillow disableEnablePillow disable
killall -STOP awesome
for job in $toggled_services; do
  stop "$job" >/dev/null 2>&1
done

"$d/lib/ld-linux-armhf.so.3" --inhibit-cache --library-path "$d/lib" "$d/retsurf" "$@"
status=$?

for job in $toggled_services; do
  start "$job" >/dev/null 2>&1
done
killall -CONT awesome
# What the panel showed before, until the home app repaints.
cat "$fb_dump" >/dev/fb0
rm -f "$fb_dump"
lipc-set-prop com.lab126.pillow disableEnablePillow enable
lipc-set-prop com.lab126.appmgrd start app://com.lab126.booklet.home
exit $status
