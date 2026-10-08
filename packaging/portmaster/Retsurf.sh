#!/bin/bash

# For the updater, which replaces this file; resolved before any `cd`.
export RETSURF_LAUNCHER="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"

XDG_DATA_HOME=${XDG_DATA_HOME:-$HOME/.local/share}

if [ -d "/opt/system/Tools/PortMaster/" ]; then
  controlfolder="/opt/system/Tools/PortMaster"
elif [ -d "/opt/tools/PortMaster/" ]; then
  controlfolder="/opt/tools/PortMaster"
elif [ -d "$XDG_DATA_HOME/PortMaster/" ]; then
  controlfolder="$XDG_DATA_HOME/PortMaster"
else
  controlfolder="/roms/ports/PortMaster"
fi

source "$controlfolder/control.txt"
[ -f "${controlfolder}/mod_${CFW_NAME}.txt" ] && source "${controlfolder}/mod_${CFW_NAME}.txt"
get_controls

# RETSURF_GAMEDIR overrides the location for installs outside ports/.
GAMEDIR="${RETSURF_GAMEDIR:-/$directory/ports/retsurf/}"

# These SoCs are homogeneous, so the first core's part id speaks for all.
CPU_PART="$(grep -m1 -i 'CPU part' /proc/cpuinfo | grep -oiE '0x[0-9a-f]+' | head -1 | tr 'A-Z' 'a-z')"
select_binary() {
  case "$CPU_PART" in
    0xd05) grep -qw atomics /proc/cpuinfo && echo "retsurf.a55" || echo "retsurf.a53" ;; # A55: RK3566, A523
    0xd04) echo "retsurf.a35" ;; # A35: RK3326
    *)     echo "retsurf.a53" ;; # A53 (H700, A133 Plus) and unknown: any ARMv8.0 core
  esac
}

BINNAME="$(select_binary)"
# A missing variant falls back to the baseline; with that gone too, fail loudly.
if [ ! -x "$GAMEDIR/$BINNAME" ]; then
  BINNAME="retsurf.a53"
fi
if [ ! -x "$GAMEDIR/$BINNAME" ]; then
  echo "ERROR: no runnable retsurf binary found in $GAMEDIR" >&2
  exit 1
fi
BIN="$GAMEDIR/$BINNAME"

cd "$GAMEDIR"

> "$GAMEDIR/log.txt" && exec > >(tee "$GAMEDIR/log.txt") 2>&1

echo "retsurf: CPU part ${CPU_PART:-unknown}, selected $BINNAME"

# The pre-config way to opt in to `[performance] swap_tuning`.
[ -f "$GAMEDIR/swap-tuning.on" ] && export RETSURF_SWAP_TUNING=1

# Bundled libraries are a fallback: a firmware carrying its own keeps using it.
# The loader answers directly, since a busybox userland may ship no `ldd`.
if [ -d "$GAMEDIR/libs" ] &&
  LD_TRACE_LOADED_OBJECTS=1 "$BIN" 2>/dev/null | grep -q "not found"; then
  echo "retsurf: a library is missing from this firmware, falling back to libs/"
  export LD_LIBRARY_PATH="$GAMEDIR/libs:$LD_LIBRARY_PATH"
fi

# fontconfig finds no font without a config, and some firmwares ship none.
fonts_in="$GAMEDIR/etc/fonts/fonts.conf.in"
fonts_conf="$GAMEDIR/data/fonts.conf"
if [ ! -f /etc/fonts/fonts.conf ] && [ -f "$fonts_in" ]; then
  if [ ! -s "$fonts_conf" ] || [ "$fonts_in" -nt "$fonts_conf" ] ||
    ! grep -q "$GAMEDIR" "$fonts_conf"; then
    mkdir -p "$GAMEDIR/data"
    sed "s|@GAMEDIR@|$GAMEDIR|g" "$fonts_in" > "$fonts_conf"
  fi
  export FONTCONFIG_FILE="$fonts_conf"
fi

export HOME="$GAMEDIR"
export XDG_DATA_HOME="$GAMEDIR"
# A launcher that knows its pad better than PortMaster names the mapping itself.
export SDL_GAMECONTROLLERCONFIG="${RETSURF_GAMECONTROLLERCONFIG:-$sdl_controllerconfig}"

export RETSURF_DATA_DIR="$GAMEDIR/data"
export RETSURF_DOWNLOAD_DIR="$GAMEDIR/downloads"
export RETSURF_PANIC_FILE="$GAMEDIR/retsurf-panic.log"
#export RETSURF_LOG_FILE="$GAMEDIR/retsurf.log"
#export RETSURF_LOG_LEVEL=debug

$GPTOKEYB "$BINNAME" &
pm_platform_helper "$BIN"
"$BIN"

pm_finish
