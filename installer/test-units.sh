#!/usr/bin/env bash
# install.sh's start of her, on its own (CI's installer job): which running instances it quits,
# where it won't start her, her log kept once, her working folder. install.sh's functions are
# loaded without running it; pgrep only ever sees this test's own stand-ins (never a real Capri),
# and `avatarctl` is never reached.
#
#   bash installer/test-units.sh
# shellcheck disable=SC2034,SC2153 # install.sh's own variables, read by its functions
set -uo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=/dev/null
source <(sed '$d' "$here/install.sh")
set +e

fails=0
check() {
    if [ "$1" = 0 ]; then echo "ok   $2"; else echo "FAIL $2"; fails=$((fails + 1)); fi
}
ok() { "$@" >/dev/null 2>&1; echo $?; }
# start_her here (not in a subshell: what it ran is checked), its output and STARTED in $out.
start() {
    start_her >"$tmp/out" 2>&1
    out="$(cat "$tmp/out")
STARTED=$STARTED"
}

tmp=$(mktemp -d)
pids=()
trap 'kill "${pids[@]}" 2>/dev/null; rm -rf "$tmp"' EXIT

# Stand-ins named synaps-avatar: one started bare (from a folder with a space), one with arguments.
mkdir -p "$tmp/My Apps" "$tmp/flags" "$tmp/home" "$tmp/state/synaps-avatar" "$tmp/bin"
cp "$(command -v cat)" "$tmp/My Apps/synaps-avatar"
cp "$(command -v sleep)" "$tmp/flags/synaps-avatar"
sleep 60 | "$tmp/My Apps/synaps-avatar" &
bare=$!
pids+=("$bare")
"$tmp/flags/synaps-avatar" 60 &
flagged=$!
pids+=("$flagged")
FAKE=()
# Only the stand-ins listed in FAKE, and only while alive (a reaped or zombie one has no cmdline).
pgrep() {
    local p
    for p in "${FAKE[@]}"; do [ -n "$(tr -d '\000' <"/proc/$p/cmdline" 2>/dev/null)" ] && echo "$p"; done
    return 0
}
RAN=()
run() {
    RAN+=("$*")
    case $1 in kill) "$@" ;; *) return 1 ;; esac
}
ui
DRY=0 AUTOSTART=1 DESKTOP=hyprland SYSTEMD=1 BIN=$tmp/bin STATE=$tmp/state HOME=$tmp/home
unset SSH_CONNECTION SSH_TTY WAYLAND_DISPLAY
export DISPLAY=:9

# her_runs: bare or with arguments, from the command line.
FAKE=("$bare" "$flagged")
her_runs
check "$([ "${BARE[*]}" = "$bare" ] && echo 0)" "a bare run is the old binary (its path has a space: still bare)"
case $HIS in */flags/synaps-avatar\ 60) r=0 ;; *) r=1 ;; esac
check $r "one with arguments is his own: $HIS"

# Where she isn't started: ssh (ssh -X has a DISPLAY too), no desktop.
SSH_CONNECTION='10.0.0.2 50000 10.0.0.1 22'
why=$(no_desktop)
check $? "an ssh session isn't a desktop"
case $why in *ssh*) r=0 ;; *) r=1 ;; esac
check $r "and says so: $why"
unset SSH_CONNECTION
SSH_TTY=/dev/pts/3
check "$(ok no_desktop)" "SSH_TTY alone, too"
unset SSH_TTY
check "$([ "$(ok no_desktop)" = 1 ] && echo 0)" "a DISPLAY and no ssh: a desktop"
check "$(DISPLAY='' ok no_desktop)" "no DISPLAY or WAYLAND_DISPLAY: no desktop"

# start_her over ssh -X: not started, nothing quit.
FAKE=()
RAN=()
SSH_CONNECTION='10.0.0.2 50000 10.0.0.1 22' start
case $out in *'an ssh session'*'not started here'*STARTED=0) r=0 ;; *) r=1 ;; esac
check $r "ssh -X: she isn't started on the far screen"
check "$([ ${#RAN[@]} = 0 ] && [ ! -e "$tmp/state/synaps-avatar/launch.log" ] && echo 0)" "and nothing ran"

# His own run (--socket, --profile...): left be, never quit or killed.
FAKE=("$flagged")
RAN=()
start
case $out in *'left be'*'avatarctl quit'*STARTED=0) r=0 ;; *) r=1 ;; esac
check $r "his own run is left be, and he's told how to switch"
check "$([ ${#RAN[@]} = 0 ] && kill -0 "$flagged" 2>/dev/null && echo 0)" "nothing quit, still running"

# The old binary, bare: quit (avatarctl, else kill), then she starts, from his home folder, her
# last log kept once.
# shellcheck disable=SC2016 # her stand-in's own lines
printf '%s\n' '#!/bin/sh' "echo \$\$ >'$tmp/started.pid'" 'echo "cwd=$(pwd)" >&2' 'echo "first-frame: painted 7 ms" >&2' 'exec sleep 30' >"$tmp/bin/synaps-avatar"
chmod +x "$tmp/bin/synaps-avatar"
echo 'the last run' >"$tmp/state/synaps-avatar/launch.log"
FAKE=("$bare")
RAN=()
cd "$tmp" && start
case $out in *"she's up (7 ms to her first frame)"*STARTED=1) r=0 ;; *) r=1 ;; esac
check $r "the old binary is quit and she starts"
case "${RAN[*]}" in *"avatarctl quit"*"kill $bare"*) r=0 ;; *) r=1 ;; esac
check $r "avatarctl quit first, else kill (that pid only): ${RAN[*]}"
check "$(kill -0 "$flagged" 2>/dev/null && echo 0)" "his own run untouched"
check "$(grep -qx "cwd=$tmp/home" "$tmp/state/synaps-avatar/launch.log" && echo 0)" "started from his home folder"
check "$(grep -qx 'the last run' "$tmp/state/synaps-avatar/launch.old.log" && echo 0)" "the last run's log kept as launch.old.log"
kill "$(cat "$tmp/started.pid" 2>/dev/null)" 2>/dev/null
pkill -P $$ -x sleep 2>/dev/null

if [ "$fails" -gt 0 ]; then
    echo "$fails install.sh unit check(s) failed"
    exit 1
fi
echo 'all install.sh unit checks passed'
