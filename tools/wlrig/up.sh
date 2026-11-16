#!/bin/bash
# tools/wlrig/up.sh: start the Wayland test rig on bella (BRIEF-wayland §4.3): bwrap → private D-Bus →
# headless sway on the Intel render node → nested Hyprland (outputs WAYLAND-1 + HEADLESS-2).
# Guards (§4.1.3): over ssh, a session without a seat, load ≤ 20, ≥ 6 GB available, no game running.
# Prints "ready" with the instance; clients go through run.sh, teardown through down.sh.
set -euo pipefail
. "$(dirname "$0")/sandbox.sh"
[ -n "${SSH_CONNECTION:-}" ] || die "launch over ssh only"
seat=$(loginctl show-session "${XDG_SESSION_ID:-none}" -p Seat --value 2>/dev/null || true)
[ -z "$seat" ] || die "this session has a seat ($seat)"
load=$(cut -d' ' -f1 /proc/loadavg)
awk -v l="$load" 'BEGIN { exit !(l <= 20) }' || die "load $load > 20"
avail=$(awk '/MemAvailable/ { print int($2 / 1048576) }' /proc/meminfo)
[ "$avail" -ge 6 ] || die "only $avail GB available"
game=$(pgrep -af 'SteamLaunch|reaper|proton|wine|\.exe' | grep -v -e pgrep -e oom_reaper -e gamescopereaper -e 'bash -c' || true)
[ -z "$game" ] || die "a game is running: $game"
pgrep -f "bwrap .*$RIG" > /dev/null && die "a rig is already up at $RIG"
mkdir -p "$RIG"/{run,home,cfg,bin,out}
chmod 700 "$RIG/run"
cp "$WLRIG"/sway.conf "$WLRIG"/hypr-rig.lua "$RIG/cfg/"
# The binds' avatarctl: logs each call (V6), then passes it on to the real one when there is one.
printf '#!/bin/bash\necho "$(date +%%s.%%N) $*" >> %s/out/avatarctl.log\n[ -x %s/bin/avatarctl.py ] && exec python3 %s/bin/avatarctl.py "$@"\n' "$RIG" "$RIG" "$RIG" > "$RIG/bin/avatarctl"
chmod +x "$RIG/bin/avatarctl"
cp "$WLRIG/../avatarctl" "$RIG/bin/avatarctl.py" 2> /dev/null || true
# A holder shell is bwrap's parent (bwrap dies with it): killing the holder or bwrap ends the rig.
setsid nohup bash -c ". '$WLRIG/sandbox.sh'; rig_bwrap bash '$WLRIG/inner.sh' '$RIG'" > "$RIG/out/rig.log" 2>&1 < /dev/null &
holder=$!
echo $holder > "$RIG/holder.pid"
for _ in $(seq 150); do
  [ -f "$RIG/run/rig.ready" ] && break
  kill -0 "$holder" 2> /dev/null || { tail -20 "$RIG/out/rig.log"; die "the sandbox exited"; }
  sleep 0.2
done
[ -f "$RIG/run/rig.ready" ] || { tail -20 "$RIG/out/rig.log"; die "not ready after 30 s"; }
cat "$RIG/run/rig.env"
echo ready
