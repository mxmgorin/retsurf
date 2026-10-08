#!/bin/sh
# NextUI / NX Redux (TrimUI) launcher. No PortMaster here, so the binary pick
# and the pad mapping are done here.
gamedir=$(cd "$(dirname "$0")" && pwd)
cd "$gamedir" || exit 1

mkdir -p "$gamedir/data" "$gamedir/downloads"
: > "$gamedir/log.txt"

# These SoCs are homogeneous, so the first core's part id speaks for all.
cpu_part=$(grep -m1 -i 'CPU part' /proc/cpuinfo | grep -oiE '0x[0-9a-f]+' | head -1 | tr 'A-Z' 'a-z')
case "$cpu_part" in
  0xd05) grep -qw atomics /proc/cpuinfo && bin=retsurf.a55 || bin=retsurf.a53 ;; # A523
  *)     bin=retsurf.a53 ;; # A133 Plus, and any ARMv8.0 core
esac
[ -x "$gamedir/$bin" ] || bin=retsurf.a53
echo "retsurf: CPU part ${cpu_part:-unknown}, selected $bin" >> "$gamedir/log.txt"

# The menu leaves the clusters capped and each pak raises its own; the governor
# still drops the clock when idle.
for policy in /sys/devices/system/cpu/cpufreq/policy*; do
  cat "$policy/cpuinfo_max_freq" > "$policy/scaling_max_freq" 2>/dev/null
done

# Bundled libraries are a fallback: a firmware carrying its own keeps using it.
if LD_TRACE_LOADED_OBJECTS=1 "$gamedir/$bin" 2>/dev/null | grep -q "not found"; then
  echo "retsurf: a library is missing from this firmware, falling back to libs/" >> "$gamedir/log.txt"
  export LD_LIBRARY_PATH="$gamedir/libs:$LD_LIBRARY_PATH"
fi

# fontconfig finds no font without a config, and the stock firmware ships none.
fonts_in="$gamedir/etc/fonts/fonts.conf.in"
fonts_conf="$gamedir/data/fonts.conf"
if [ ! -f /etc/fonts/fonts.conf ]; then
  if [ ! -s "$fonts_conf" ] || [ "$fonts_in" -nt "$fonts_conf" ] ||
     ! grep -q "$gamedir" "$fonts_conf"; then
    sed "s|@GAMEDIR@|$gamedir|g" "$fonts_in" > "$fonts_conf"
  fi
  export FONTCONFIG_FILE="$fonts_conf"
fi

# Servo panics when the platform store holds no CA bundle, as on stock TrimUI.
export SSL_CERT_FILE="$gamedir/etc/ssl/cacert.pem"

# trimui_inputd's pad reports Xbox codes under Nintendo labels; map by label.
export SDL_GAMECONTROLLERCONFIG="030000005e0400008e02000014010000,TRIMUI Player1,a:b1,b:b0,back:b6,dpdown:h0.4,dpleft:h0.8,dpright:h0.2,dpup:h0.1,guide:b8,leftshoulder:b4,leftstick:b9,lefttrigger:a2,leftx:a0,lefty:a1,rightshoulder:b5,rightstick:b10,righttrigger:a5,rightx:a3,righty:a4,start:b7,x:b3,y:b2,platform:Linux,"

export HOME="$gamedir"
export XDG_DATA_HOME="$gamedir"
export RETSURF_DATA_DIR="$gamedir/data"
export RETSURF_DOWNLOAD_DIR="$gamedir/downloads"
export RETSURF_PANIC_FILE="$gamedir/retsurf-panic.log"
#export RETSURF_LOG_LEVEL=debug

exec "$gamedir/$bin" >> "$gamedir/log.txt" 2>&1
