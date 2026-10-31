#!/bin/sh
# Runs the browser in KOReader's place: waits until KOReader and its launcher
# script have exited and restored the framework, runs the browser, then starts
# KOReader again. Must run detached from KOReader.
d=$(cd "$(dirname "$0")" && pwd)
# Upper bounds on the home app coming back: after a full framework restart, or
# after koreader.sh's own relaunch.
restart_timeout=180
relaunch_timeout=30

# Parking the framework while the home app starts crashes it. Armed before the
# wait below, so the app's event cannot be missed.
home_timeout=$relaunch_timeout
status lab126_gui | grep -q "start/running" || home_timeout=$restart_timeout
lipc-wait-event -s "$home_timeout" com.lab126.appmgrd appStarted >/dev/null 2>&1 &
home_wait=$!

while ps | grep -q -e "[r]eader.lua" -e "[k]oreader/koreader.sh"; do
  sleep 1
done
wait "$home_wait"

"$d/retsurf.sh"
exec /mnt/us/koreader/koreader.sh
