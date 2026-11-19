#!/usr/bin/env bash
# install.sh's start of her, on its own (CI's installer job): which running instances it quits,
# where it won't start her, her log kept once, her working folder; the Synaps it installs (the
# manifest's pin) and the daemon's protocol, on stand-ins. install.sh's functions are
# loaded without running it; pgrep only ever sees this test's own stand-ins (never a real Capri),
# and `avatarctl` is never reached.
#
#   bash installer/test-units.sh
# shellcheck disable=SC2034,SC2153,SC2154 # install.sh's own variables (M_*: its manifest's), read by its functions
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
# A second bare one, for a quit that takes her yet reports a failure.
sleep 60 | "$tmp/My Apps/synaps-avatar" &
bare2=$!
pids+=("$bare2")
FAKE=()
# Only the stand-ins listed in FAKE, and only while alive (a reaped or zombie one has no cmdline).
pgrep() {
    local p
    for p in "${FAKE[@]}"; do [ -n "$(tr -d '\000' <"/proc/$p/cmdline" 2>/dev/null)" ] && echo "$p"; done
    return 0
}
RAN=()
QUIT_TAKES=''
# avatarctl fails (nothing answers here); with QUIT_TAKES it took that pid first (gone when it returns).
run() {
    RAN+=("$*")
    case $1 in
        kill) "$@" ;;
        *)
            if [ -n "$QUIT_TAKES" ]; then
                kill "$QUIT_TAKES"
                while [ -n "$(tr -d '\000' <"/proc/$QUIT_TAKES/cmdline" 2>/dev/null)" ]; do sleep 0.05; done
            fi
            return 1
            ;;
    esac
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

# What the binary step says of a run going: what the end of the install will do with it.
note() { old_run_note 2>&1; }
FAKE=()
check "$([ -z "$(note)" ] && echo 0)" "no run going: nothing said"
FAKE=("$bare")
case $(note) in *'restarted at the end'*) r=0 ;; *) r=1 ;; esac
check $r "a bare run, a desktop: restarted at the end"
FAKE=("$bare" "$flagged")
case $(note) in *'as you started her: left be'*) r=0 ;; *) r=1 ;; esac
check $r "his own run: left be, not restarted: $(note)"
case $(SSH_TTY=/dev/pts/3 note) in *'old binary: an ssh session'*"isn't restarted from here") r=0 ;; *) r=1 ;; esac
check $r "over ssh: not restarted from here: $(SSH_TTY=/dev/pts/3 note)"
case $(DISPLAY='' note) in *'old binary: no desktop'*"isn't restarted from here") r=0 ;; *) r=1 ;; esac
check $r "no desktop: not restarted from here"

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

# Its pids read again right before a kill: one the quit already took isn't killed (its pid may be
# another process's by then).
FAKE=("$bare2")
RAN=()
QUIT_TAKES=$bare2
start
case "${RAN[*]}" in *'avatarctl quit'*kill*) r=1 ;; *'avatarctl quit'*) r=0 ;; *) r=1 ;; esac
check $r "a quit that took her: no kill after it: ${RAN[*]}"
case $out in *"she's up"*STARTED=1) r=0 ;; *) r=1 ;; esac
check $r "and she starts"
kill "$(cat "$tmp/started.pid" 2>/dev/null)" 2>/dev/null
pkill -P $$ -x sleep 2>/dev/null

# Synaps: the manifest's pin and its hash, never releases/latest (a newer one may speak another
# protocol). A stand-in `synaps` older than synaps_min; a dry run prints what it would fetch.
CHECKOUT=$here QUIT_TAKES='' STEP_N=0 STEPS=7
manifest
mkdir -p "$tmp/old-synaps" "$tmp/cargo/bin"
printf '#!/bin/sh\necho "synaps 0.9.0"\n' >"$tmp/old-synaps/synaps"
chmod +x "$tmp/old-synaps/synaps"
# From here on (the last checks): GitHub is never asked, nothing is recorded, no systemd.
fetch_text() { echo "$*" >>"$tmp/fetched"; return 1; }
curl() { echo "$*" >>"$tmp/fetched"; return 1; }
rec_meta() { :; }
systemd_user() { return 1; }
out=$(PATH=$tmp/old-synaps:$PATH DRY=1 YES=1 CARGO_BIN=$tmp/cargo/bin TMP=$tmp step_synaps 2>&1)
pin=https://github.com/$M_synaps_repo/releases/download/v$M_synaps_pin/synaps-installer.sh
case $out in *"installing $M_synaps_pin"*"$pin"*"(checks its sha256: $M_synaps_installer_sh_sha256)"*) r=0 ;; *) r=1 ;; esac
check $r "synaps older than $M_synaps_min: the pinned $M_synaps_pin, its manifest hash"
check "$([ ! -e "$tmp/fetched" ] && echo 0)" "and never asks GitHub for its latest: $(head -n1 "$tmp/fetched" 2>/dev/null)"

# The daemon's protocol is hers exactly, or the install stops (never "running"). A stand-in synaps
# whose `daemon status` reads as 0.12.0's (no `pid:` line: none said); $2=2: this run upgraded Synaps.
daemon_says() {
    {
        printf '#!/bin/sh\necho "daemon: running"\necho "socket: /run/user/1/synaps.sock"\n'
        [ -z "$1" ] || printf 'echo "pid: 4242  version: 0.9.0  protocol: %s  started: 2026-11-19T00:00:00Z"\n' "$1"
    } >"$tmp/synaps"
    chmod +x "$tmp/synaps"
    (SYNAPS=$tmp/synaps SYNAPS_V=$M_synaps_pin SYNAPS_NEW=${2:-0} UNITS=$tmp/units DRY=0 step_daemon) 2>&1
    echo "rc=$?"
}
out=$(daemon_says "$M_synaps_protocol")
case $out in *"daemon running (protocol $M_synaps_protocol)"*rc=0) r=0 ;; *) r=1 ;; esac
check $r "a daemon that speaks protocol $M_synaps_protocol: running"
out=$(daemon_says 4)
case $out in *"speaks protocol 4"*"can't attach"*"v$M_synaps_pin/synaps-installer.sh"*"daemon stop"*rc=1) r=0 ;; *) r=1 ;; esac
check $r "one that speaks 4: the install stops, saying how to fix it (the pin, then stop the daemon)"
case $out in *"daemon running"*) r=1 ;; *) r=0 ;; esac
check $r "and never says it's running"
out=$(daemon_says 2 2)
case $out in *"synaps $M_synaps_pin is installed, but the daemon still running is the old one (protocol 2"*"daemon stop"*rc=1) r=0 ;; *) r=1 ;; esac
check $r "Synaps upgraded by this run, the old daemon still up: stop it (not: install the pin again)"
case $out in *"Install synaps"* | *"daemon running"*) r=1 ;; *) r=0 ;; esac
check $r "and nothing else said"
out=$(daemon_says '')
case $out in *"doesn't say which protocol"*"daemon stop"*rc=1) r=0 ;; *) r=1 ;; esac
check $r "one that doesn't say its protocol: the install stops too"

if [ "$fails" -gt 0 ]; then
    echo "$fails install.sh unit check(s) failed"
    exit 1
fi
echo 'all install.sh unit checks passed'
