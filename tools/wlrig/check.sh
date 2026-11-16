#!/bin/bash
# tools/wlrig/check.sh: the phase-0 list (BRIEF-wayland §4.7), V1–V4 and V8 here (V5–V7 and E1–E3 need
# clients: checks.py). One "Vn PASS|FAIL detail" line each. The rig must be up (up.sh).
set -uo pipefail
. "$(dirname "$0")/sandbox.sh"
R="bash $WLRIG/run.sh"
ok() { echo "$1 PASS $2"; }
no() { echo "$1 FAIL $2"; FAILED=1; }
FAILED=0
# V1: the sandbox; the render node is the only DRM node, no NVIDIA device, no uinput, no X sockets
inside=$($R bash -c 'echo $(ls /dev/dri) / $(ls -d /dev/nvidia* /dev/uinput /tmp/.X11-unix /run/user/*/bus 2>/dev/null)')
[ "$inside" = "${NODE##*/} / $RUNDIR/bus" ] && ok V1 "dri: ${NODE##*/} only; no nvidia, uinput, X sockets; bus = the rig's" || no V1 "$inside"
# V2: compositors up, nested Hyprland on the Wayland backend, no DRM session, Mesa Intel, no NVIDIA anywhere
hlog=$(ls "$RIG"/run/hypr/*/hyprland.log | head -1)
drm=$(grep -c 'DRM Backend failed' "$hlog")
wlbe=$(grep -c 'for implementation wayland' "$hlog")
hp=$(for p in $(pgrep -x Hyprland); do grep -q "$RIG" "/proc/$p/cmdline" && echo "$p"; done)
mesa=$(grep -c -e libEGL_mesa -e libgallium "/proc/$hp/maps")
nodes=$(ls -l "/proc/$hp/fd" 2>/dev/null | grep -o '/dev/dri/[a-zA-Z0-9]*' | sort -u | tr '\n' ' ')
nvmaps=0
for p in $(pgrep -f "$RIG/cfg"); do grep -q -i nvidia "/proc/$p/maps" 2>/dev/null && nvmaps=$((nvmaps + 1)); done
nvsmi=$(nvidia-smi | grep -c -i -e ' sway' -e hyprland)
if [ "$wlbe" -ge 1 ] && [ "$drm" -ge 1 ] && [ "$mesa" -ge 1 ] && [ "$nodes" = "$NODE " ] && [ $nvmaps = 0 ] && [ "$nvsmi" = 0 ]; then
  ok V2 "nested on the Wayland backend; DRM backend failed (no session); Mesa EGL/gallium on $nodes; nvidia: 0 maps, 0 in nvidia-smi"
else
  no V2 "wayland=$wlbe drm-failed=$drm mesa=$mesa nodes=[$nodes] nvidia maps=$nvmaps smi=$nvsmi"
fi
# V3: two monitors (the second made at run time), each with its reserved area
$R hyprctl output create headless HEADLESS-2 > /dev/null
sleep 0.5
mons=$($R hyprctl -j monitors | jq -c '[.[] | {name, x, y, width, height, scale, reserved}]')
[ "$($R hyprctl -j monitors | jq 'length')" = 2 ] && ok V3 "$mons" || no V3 "$mons"
# V4: capture both outputs
$R grim -o WAYLAND-1 "$RIG/out/v4-wayland-1.png" && $R grim -o HEADLESS-2 "$RIG/out/v4-headless-2.png" \
  && ok V4 "$(file -b "$RIG"/out/v4-*.png | cut -d, -f2 | tr '\n' ';')" || no V4 "grim failed"
# V8: one batch request, three JSON values
n=$($R hyprctl --batch 'j/monitors;j/clients;j/layers' | python3 -c 'import json,sys; t=sys.stdin.read(); d=json.JSONDecoder(); i=n=0
while True:
    while i < len(t) and t[i].isspace(): i+=1
    if i>=len(t): break
    v,i=d.raw_decode(t,i); n+=1
print(n)')
[ "$n" = 3 ] && ok V8 "[[BATCH]]j/monitors;j/clients;j/layers -> 3 values" || no V8 "$n values"
exit $FAILED
