#!/bin/bash
# tools/wlrig/down.sh [KEEP_DIR]: end the rig, check nothing is left (§4.1.5), remove $RIG. KEEP_DIR: copy
# $RIG/out there first.
set -uo pipefail
. "$(dirname "$0")/sandbox.sh"
[ -n "${1:-}" ] && [ -d "$RIG/out" ] && { mkdir -p "$1"; cp -a "$RIG/out/." "$1/"; }
# Every process whose command line names the rig, except this script's own ancestry.
rigpids() {
  local mine=" $$ " p=$$
  while [ "$p" -gt 1 ]; do p=$(awk '{print $4}' "/proc/$p/stat" 2>/dev/null || echo 1); mine+="$p "; done
  for p in $(pgrep -f "$RIG"); do [[ "$mine" == *" $p "* ]] || echo "$p"; done
}
[ -f "$RIG/holder.pid" ] && kill "$(cat "$RIG/holder.pid")" 2>/dev/null
for p in $(rigpids); do kill "$p" 2>/dev/null; done
for _ in $(seq 50); do [ -z "$(rigpids)" ] && break; sleep 0.1; done
for p in $(rigpids); do kill -KILL "$p" 2>/dev/null; done; sleep 0.3
left=$(rigpids)
[ -z "$left" ] || die "still running: $(ps -o pid,args -p "$(echo $left | tr ' ' ,)")"
rm -rf "$RIG"
[ -e "$RIG" ] && die "$RIG still there"
nv=$(nvidia-smi --query-compute-apps=pid,process_name --format=csv,noheader 2>/dev/null | grep -i -e capri-wl-rig -e Hyprland -e sway || true)
[ -z "$nv" ] || die "nvidia-smi lists: $nv"
echo "down: no rig processes, $RIG gone"
