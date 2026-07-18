#!/bin/bash

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

GAMEDIR=/$directory/ports/retsurf/

# Pick the build matching this device's CPU. Match only cores we  recognize
# anything unknown falls through to the v8.0baseline, which runs on every ARMv8.0+ core.
# All target SoCs are homogeneous, so the first CPU's part id is representative.
#   0xd05 = Cortex-A55 (RK3566, Allwinner A523)
#   0xd04 = Cortex-A35 (RK3326)
#   0xd03 = Cortex-A53 (H700, Allwinner A133 Plus) — and the sane default.
# ARM "CPU part" id of the first (representative) core, lowercased. Kept as a
# global so the selection can be logged to log.txt after the redirect below.
CPU_PART="$(grep -m1 -i 'CPU part' /proc/cpuinfo | grep -oiE '0x[0-9a-f]+' | head -1 | tr 'A-Z' 'a-z')"
select_binary() {
  case "$CPU_PART" in
    0xd05) grep -qw atomics /proc/cpuinfo && echo "retsurf.a55" || echo "retsurf.a53" ;;
    0xd04) echo "retsurf.a35" ;;
    *)     echo "retsurf.a53" ;;   # A53 and any unrecognized CPU
  esac
}

BINNAME="$(select_binary)"
# Guard against a missing/non-executable variant: prefer the baseline, and if
# even that is gone, fail loudly instead of exec-ing nothing.
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

# Record which per-CPU build the launcher picked (CPU part -> binary) so log.txt
# shows it for support/debugging.
echo "retsurf: CPU part ${CPU_PART:-unknown}, selected $BINNAME"

export HOME="$GAMEDIR"
export XDG_DATA_HOME="$GAMEDIR"
export SDL_GAMECONTROLLERCONFIG="$sdl_controllerconfig"

export RETSURF_DATA_DIR="$GAMEDIR/data"
export RETSURF_DOWNLOAD_DIR="$GAMEDIR/downloads"
export RETSURF_PANIC_FILE="$GAMEDIR/retsurf-panic.log"
#export RETSURF_LOG_FILE="$GAMEDIR/retsurf.log"
#export RETSURF_LOG_LEVEL=debug

$GPTOKEYB "$BINNAME" &
pm_platform_helper "$BIN"
"$BIN"

pm_finish
