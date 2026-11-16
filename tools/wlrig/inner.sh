#!/bin/bash
# tools/wlrig/inner.sh: runs inside the sandbox (up.sh). Private session bus, headless sway on the render
# node (the parent compositor that gives nested Hyprland an allocator), nested Hyprland, a second output.
set -u
O=$RIG/out
mkdir -p "$XDG_CONFIG_HOME" "$XDG_STATE_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME"
dbus-daemon --config-file=/usr/share/dbus-1/session.conf --address="unix:path=$XDG_RUNTIME_DIR/bus" --nofork --nopidfile > "$O/dbus.log" 2>&1 &
waitfor() { for _ in $(seq 100); do compgen -G "$1" > /dev/null && return 0; sleep 0.1; done; echo "no $1" >&2; return 1; }
WLR_BACKENDS=headless WLR_RENDERER=gles2 WLR_RENDER_DRM_DEVICE=$NODE WLR_HEADLESS_OUTPUTS=1 \
  sway -c "$RIG/cfg/sway.conf" > "$O/sway.log" 2>&1 &
waitfor "$XDG_RUNTIME_DIR/wayland-*.lock" || exit 1
parent=$(basename "$(compgen -G "$XDG_RUNTIME_DIR/wayland-*.lock" | head -1)" .lock)
# Hyprland's nested backend binds xdg_wm_base v6, sway 1.12 offers v5: a relay clamps binds to what is offered.
python3 "$WLRIG/wlclamp.py" "$XDG_RUNTIME_DIR/wayland-clamp" "$XDG_RUNTIME_DIR/$parent" > "$O/wlclamp.log" 2>&1 &
waitfor "$XDG_RUNTIME_DIR/wayland-clamp" || exit 1
WAYLAND_DISPLAY=wayland-clamp Hyprland --config "$RIG/cfg/hypr-rig.lua" > "$O/hypr.log" 2>&1 &
waitfor "$XDG_RUNTIME_DIR/hypr/*/.socket.sock" || exit 1
sig=$(basename "$(dirname "$(compgen -G "$XDG_RUNTIME_DIR/hypr/*/.socket.sock" | head -1)")")
export HYPRLAND_INSTANCE_SIGNATURE=$sig
n=
for _ in $(seq 50); do n=$(hyprctl -j instances 2> /dev/null | jq length 2> /dev/null) && [ -n "$n" ] && break; sleep 0.1; done
[ "$n" = 1 ] || { echo "instances: $n, want 1" >&2; exit 1; }
wl=$(hyprctl -j instances | jq -r '.[0].wl_socket')
printf 'PARENT_DISPLAY=%s\nHYPRLAND_INSTANCE_SIGNATURE=%s\nWAYLAND_DISPLAY=%s\n' "$parent" "$sig" "$wl" > "$XDG_RUNTIME_DIR/rig.env"
touch "$XDG_RUNTIME_DIR/rig.ready"
wait
