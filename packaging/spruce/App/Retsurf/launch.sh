#!/bin/sh
# Runs the port through spruce's PORTS launcher, which sets up PortMaster and
# picks the system from the Emu/PORTS/ segment of its path.
appdir=$(cd "$(dirname "$0")" && pwd)
export RETSURF_GAMEDIR="$appdir/retsurf/"
exec /mnt/SDCARD/Emu/PORTS/../../spruce/scripts/emu/standard_launch.sh "$appdir/Retsurf.sh"
