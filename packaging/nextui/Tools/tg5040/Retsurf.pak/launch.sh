#!/bin/sh
# NextUI / NX Redux (TrimUI) launcher. Runs without PortMaster.
gamedir=$(cd "$(dirname "$0")" && pwd)
cd "$gamedir" || exit 1

mkdir -p "$gamedir/data" "$gamedir/downloads"
: > "$gamedir/log.txt"

bin=retsurf.aarch64

# NextUI leaves the CPU clusters capped; each pak lifts its own cap.
for policy in /sys/devices/system/cpu/cpufreq/policy*; do
  cat "$policy/cpuinfo_max_freq" > "$policy/scaling_max_freq" 2>/dev/null
done

# libs/ is added only when the firmware lacks a needed library.
if LD_TRACE_LOADED_OBJECTS=1 "$gamedir/$bin" 2>/dev/null | grep -q "not found"; then
  echo "retsurf: a library is missing from this firmware, falling back to libs/" >> "$gamedir/log.txt"
  export LD_LIBRARY_PATH="$gamedir/libs:$LD_LIBRARY_PATH"
fi

# Firmwares without /etc/fonts/fonts.conf leave fontconfig with no fonts.
fonts_in="$gamedir/etc/fonts/fonts.conf.in"
fonts_conf="$gamedir/data/fonts.conf"
if [ ! -f /etc/fonts/fonts.conf ]; then
  if [ ! -s "$fonts_conf" ] || [ "$fonts_in" -nt "$fonts_conf" ] ||
     ! grep -q "$gamedir" "$fonts_conf"; then
    sed "s|@GAMEDIR@|$gamedir|g" "$fonts_in" > "$fonts_conf"
  fi
  export FONTCONFIG_FILE="$fonts_conf"
fi

# Servo panics without a CA bundle; stock TrimUI ships none.
export SSL_CERT_FILE="$gamedir/etc/ssl/cacert.pem"

# trimui_inputd reports Xbox codes under Nintendo labels; buttons map by label.
export SDL_GAMECONTROLLERCONFIG="030000005e0400008e02000014010000,TRIMUI Player1,a:b1,b:b0,back:b6,dpdown:h0.4,dpleft:h0.8,dpright:h0.2,dpup:h0.1,guide:b8,leftshoulder:b4,leftstick:b9,lefttrigger:a2,leftx:a0,lefty:a1,rightshoulder:b5,rightstick:b10,righttrigger:a5,rightx:a3,righty:a4,start:b7,x:b3,y:b2,platform:Linux,"

export HOME="$gamedir"
export XDG_DATA_HOME="$gamedir"
export RETSURF_DATA_DIR="$gamedir/data"
export RETSURF_DOWNLOAD_DIR="$gamedir/downloads"
export RETSURF_PANIC_FILE="$gamedir/retsurf-panic.log"
#export RETSURF_LOG_LEVEL=debug

exec "$gamedir/$bin" "$@" >> "$gamedir/log.txt" 2>&1
