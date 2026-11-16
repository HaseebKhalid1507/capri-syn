#!/bin/bash
# tools/wlrig/e1.sh [SECONDS]: E1, full-output (2560x1600) vs small (600x800) layer at 5 presents/s, interleaved
# twice; Hyprland's and sway's CPU (% of a core) from /proc for each run.
set -uo pipefail
. "$(dirname "$0")/sandbox.sh"
S=${1:-60}
pidof_rig() { for p in $(pgrep -x "$1"); do grep -q "$RIG" "/proc/$p/cmdline" && echo "$p"; done; }
cpu() { awk '{print $14 + $15}' "/proc/$1/stat"; }
hp=$(pidof_rig Hyprland); sp=$(pidof_rig sway)
hz=$(getconf CLK_TCK)
for r in 1 2; do
  for size in "2560 1600" "600 800"; do
    h0=$(cpu "$hp"); s0=$(cpu "$sp"); t0=$(date +%s.%N)
    bash "$WLRIG/run.sh" python3 "$WLRIG/e1.py" $size "$S" | tr '\n' ' '
    h1=$(cpu "$hp"); s1=$(cpu "$sp"); t1=$(date +%s.%N)
    awk -v h=$((h1 - h0)) -v s=$((s1 - s0)) -v t0="$t0" -v t1="$t1" -v hz="$hz" -v l="$(cut -d' ' -f1 /proc/loadavg)" \
      'BEGIN { d = t1 - t0; printf "Hyprland %.2f %%, sway %.2f %% (load %s)\n", 100 * h / hz / d, 100 * s / hz / d, l }'
  done
done
