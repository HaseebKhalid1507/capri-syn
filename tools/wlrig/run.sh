#!/bin/bash
# tools/wlrig/run.sh CMD...: run CMD in a client sandbox (same rules as the compositors') against the rig's
# Hyprland. Ends with the calling shell (--die-with-parent).
set -euo pipefail
. "$(dirname "$0")/sandbox.sh"
[ -f "$RIG/run/rig.ready" ] || die "the rig isn't up (up.sh)"
env=()
while IFS='=' read -r k v; do env+=(--setenv "$k" "$v"); done < "$RIG/run/rig.env"
rig_bwrap "${env[@]}" "$@"
