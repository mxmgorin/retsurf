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

# Swap tuning, off unless `swap-tuning.on` exists here: it is system-wide and
# outlives the port, and every CFW has its own idea about swap.
SWAP_TUNING_ZRAM=""
SWAP_TUNING_CLUSTER=""
SWAP_TUNING_SWAPPINESS=""
SWAP_TUNING_MODULE=""

# Separate so an unmet requirement can bail out without skipping the caller's rest.
swap_tuning_add_zram() {
  local total_mb=$1

  # ROCKNIX ships zram as a module and never loads it; muOS 4.9 has none.
  if [ ! -w /sys/class/zram-control/hot_add ]; then
    modprobe zram >/dev/null 2>&1 && SWAP_TUNING_MODULE=1
  fi
  [ -w /sys/class/zram-control/hot_add ] || return 0
  command -v mkswap >/dev/null && command -v swapon >/dev/null || return 0

  local n
  n=$(cat /sys/class/zram-control/hot_add 2>/dev/null) || return 0
  [ -n "$n" ] || return 0

  # Fastest to decompress wins: a fault waits on that. lz4 is absent from Batocera.
  local algo
  for algo in lz4 lzo-rle lzo; do
    grep -qw "$algo" "/sys/block/zram$n/comp_algorithm" 2>/dev/null || continue
    echo "$algo" > "/sys/block/zram$n/comp_algorithm" 2>/dev/null && break
  done

  # zram lives in the RAM it stands in for, so size it from RAM, never a constant.
  if ! echo $(( total_mb * 2 / 3 * 1024 * 1024 )) > "/sys/block/zram$n/disksize" 2>/dev/null; then
    echo "$n" > /sys/class/zram-control/hot_remove 2>/dev/null
    return 0
  fi

  # Outrank the firmware's device so new pages land in ours and come back fast.
  if mkswap "/dev/zram$n" >/dev/null 2>&1 && swapon -p 1100 "/dev/zram$n" 2>/dev/null; then
    SWAP_TUNING_ZRAM="$n"
    echo "retsurf: swap tuning on (zram$n, $(( total_mb * 2 / 3 )) MiB, $(sed 's/.*\[\(.*\)\].*/\1/' "/sys/block/zram$n/comp_algorithm"))"
  else
    echo "$n" > /sys/class/zram-control/hot_remove 2>/dev/null
  fi
}

swap_tuning_start() {
  [ -f "$GAMEDIR/swap-tuning.on" ] || return 0

  # 1536 MB is where the engine's own `tight` profile stops applying.
  local total_mb
  total_mb=$(awk '/^MemTotal:/{print int($2/1024)}' /proc/meminfo 2>/dev/null)
  [ -n "$total_mb" ] && [ "$total_mb" -le 1536 ] || return 0

  swap_tuning_add_zram "$total_mb"

  # Both knobs only describe how swap is used; muOS has none at all.
  [ -n "$SWAP_TUNING_ZRAM" ] || grep -q "^/" /proc/swaps 2>/dev/null || return 0

  # The default of 3 fetches eight pages a fault; zram decompresses each one.
  if [ -w /proc/sys/vm/page-cluster ]; then
    SWAP_TUNING_CLUSTER=$(cat /proc/sys/vm/page-cluster)
    echo 0 > /proc/sys/vm/page-cluster
  fi
  # Dropping a file page here means re-reading the 79 MB binary off the card.
  if [ -w /proc/sys/vm/swappiness ]; then
    SWAP_TUNING_SWAPPINESS=$(cat /proc/sys/vm/swappiness)
    echo 100 > /proc/sys/vm/swappiness
  fi
}

# On any exit, including a crash: a device left attached holds RAM, and a changed
# swappiness silently changes how everything else on the system behaves.
swap_tuning_stop() {
  [ -n "$SWAP_TUNING_CLUSTER" ] && echo "$SWAP_TUNING_CLUSTER" > /proc/sys/vm/page-cluster 2>/dev/null
  [ -n "$SWAP_TUNING_SWAPPINESS" ] && echo "$SWAP_TUNING_SWAPPINESS" > /proc/sys/vm/swappiness 2>/dev/null
  if [ -n "$SWAP_TUNING_ZRAM" ]; then
    # A swapoff with nowhere to put the pages fails; a working swap beats forcing it.
    if swapoff "/dev/zram$SWAP_TUNING_ZRAM" 2>/dev/null; then
      echo "$SWAP_TUNING_ZRAM" > /sys/class/zram-control/hot_remove 2>/dev/null
    else
      echo "retsurf: could not release zram$SWAP_TUNING_ZRAM; it stays until reboot"
      return 0
    fi
  fi
  [ -n "$SWAP_TUNING_MODULE" ] && modprobe -r zram >/dev/null 2>&1
  return 0
}

trap swap_tuning_stop EXIT
swap_tuning_start

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
