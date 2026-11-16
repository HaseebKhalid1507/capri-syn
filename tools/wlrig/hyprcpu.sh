#!/bin/bash
# tools/wlrig/hyprcpu.sh SECONDS: on the host, the rig's Hyprland CPU ticks once a second ("epoch ticks"), for
# p1.py's idle windows (its pid lives in the sandbox's namespace).
. "$(dirname "$0")/sandbox.sh"
p=$(pgrep -f "^Hyprland --config $RIG/cfg/" | head -1)
[ -n "$p" ] || die "no rig Hyprland"
for _ in $(seq "$1"); do
  echo "$(date +%s.%N) $(awk '{print $14 + $15}' "/proc/$p/stat" 2>/dev/null || echo gone)"
  sleep 1
done
