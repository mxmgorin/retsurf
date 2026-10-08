#!/bin/sh
# Via CrossMix's PORTS launcher, for its CPU mode, library path and gptokeyb delay.
appdir=$(cd "$(dirname "$0")" && pwd)
export RETSURF_GAMEDIR="$appdir/retsurf/"
# PortMaster maps this pad by Xbox position; the TSP's A is on the right.
export RETSURF_GAMECONTROLLERCONFIG="030000005e0400008e02000014010000,TRIMUI Player1,a:b1,b:b0,back:b6,dpdown:h0.4,dpleft:h0.8,dpright:h0.2,dpup:h0.1,guide:b8,leftshoulder:b4,leftstick:b9,lefttrigger:a2,leftx:a0,lefty:a1,rightshoulder:b5,rightstick:b10,righttrigger:a5,rightx:a3,righty:a4,start:b7,x:b3,y:b2,platform:Linux,"
exec /mnt/SDCARD/Emus/PORTS/launch.sh "$appdir/Retsurf.sh"
