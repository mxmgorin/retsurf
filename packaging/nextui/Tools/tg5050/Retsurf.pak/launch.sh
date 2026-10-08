#!/bin/sh
# The Smart Pro S shares the tg5040 pak, so the card holds one copy.
exec "$(dirname "$0")/../../tg5040/Retsurf.pak/launch.sh" "$@"
