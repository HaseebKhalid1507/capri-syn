#!/usr/bin/env bash
# install.sh's start of her, on its own (CI's installer job): which running instances it quits,
# where it won't start her, her log kept once, her working folder; the Synaps it installs (the
# manifest's pin) and the daemon's protocol; her app-menu entry, her summon key on GNOME and
# Hyprland, and --uninstall taking them away; --voice's PipeWire tools and the Python her voice gets
# (tools/capri-voice-setup too, with a python3 that says 3.14); on stand-ins. install.sh's functions are
# loaded without running it; pgrep only ever sees this test's own stand-ins (never a real Capri),
# and `avatarctl`, kill and pkill are never reached.
#
#   bash installer/test-units.sh
# shellcheck disable=SC2034,SC2153,SC2154 # install.sh's own variables (M_*: its manifest's), read by its functions
# shellcheck disable=SC2016 # $mainMod in single quotes is Hyprland's, as written in its config
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
# Only the stand-ins listed in FAKE, and only while alive (a reaped or zombie one has no cmdline);
# none: 1, as pgrep says it.
pgrep() {
    local p r=1
    for p in "${FAKE[@]}"; do [ -n "$(tr -d '\000' <"/proc/$p/cmdline" 2>/dev/null)" ] && echo "$p" && r=0; done
    return $r
}
RAN=()
QUIT_TAKES=''
# avatarctl fails (nothing answers here); with QUIT_TAKES it took that pid first (gone when it returns).
# RUN_REAL=1 (the last checks, in a home under $tmp): everything runs but avatarctl, kill and pkill.
RUN_REAL=0
run() {
    RAN+=("$*")
    if [ "$RUN_REAL" = 1 ]; then
        case $1 in kill | pkill | */avatarctl) return 0 ;; esac
        "$@"
        return
    fi
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
printf '%s\n' '#!/bin/sh' "echo \$\$ >'$tmp/started.pid'" 'echo "cwd=$(pwd)" >&2' 'echo "notice=${SA_NO_NOTICE-}" >&2' 'echo "first-frame: painted 7 ms" >&2' 'exec sleep 30' >"$tmp/bin/synaps-avatar"
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
check "$(grep -qx 'notice=1' "$tmp/state/synaps-avatar/launch.log" && echo 0)" "SA_NO_NOTICE=1: a failed start is said here, not in a notification too"
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

# A start that fails: the end of her log, not her line saying no notification was shown.
printf '%s\n' '#!/bin/sh' 'for i in 1 2 3 4 5; do echo "line $i" >&2; done' \
    "echo \"synaps-avatar: not said on screen (SA_NO_NOTICE=\$SA_NO_NOTICE): Capri couldn't start.\" >&2" 'exit 1' >"$tmp/bin/synaps-avatar"
FAKE=()
RAN=()
start
case $out in *"she didn't start"*'line 2'*'line 5'*STARTED=0) r=0 ;; *) r=1 ;; esac
case $out in *'line 1'* | *'not said on screen'*) r=1 ;; esac
check $r "a failed start: her log's last 4 lines, not the notice's"
check "$(grep -q 'not said on screen (SA_NO_NOTICE=1)' "$tmp/state/synaps-avatar/launch.log" && echo 0)" "and none was shown (SA_NO_NOTICE=1)"

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
case $out in *"daemon stop"*) r=1 ;; *) r=0 ;; esac
check $r "no daemon running: nothing said about one"
# An upgrade, with a daemon up: step 1 says to stop it, so he can while the rest installs.
# shellcheck disable=SC2016 # the stand-in's own $1
printf '#!/bin/sh\ncase $1 in daemon) echo "daemon: running" ;; esac\n' >"$tmp/cargo/bin/synaps"
chmod +x "$tmp/cargo/bin/synaps"
out=$(PATH=$tmp/old-synaps:$PATH DRY=1 YES=1 CARGO_BIN=$tmp/cargo/bin TMP=$tmp step_synaps 2>&1)
case $out in *"a daemon is running, from the Synaps before this one"*"cargo/bin/synaps daemon stop"*) r=0 ;; *) r=1 ;; esac
check $r "an upgrade with a daemon up: stop it now, said in step 1"
rm -f "$tmp/cargo/bin/synaps"

# The daemon's protocol is hers exactly, or the install stops (never "running"). A stand-in synaps
# whose `daemon status` reads as 0.12.0's (no `pid:` line: none said); $2: SYNAPS_NEW (this run put
# Synaps in: 1 new, 2 an upgrade).
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
for n in 2 1; do
    out=$(daemon_says 2 $n)
    case $out in *"synaps $M_synaps_pin is installed, but the daemon still running is another one (protocol 2"*"daemon stop"*rc=1) r=0 ;; *) r=1 ;; esac
    check $r "Synaps put in by this run (SYNAPS_NEW=$n), another daemon still up: stop it (not: install the pin again)"
    case $out in *"Install synaps"* | *"daemon running"*) r=1 ;; *) r=0 ;; esac
    check $r "and nothing else said"
done
out=$(daemon_says '')
case $out in *"doesn't say which protocol"*"daemon stop"*rc=1) r=0 ;; *) r=1 ;; esac
check $r "one that doesn't say its protocol: the install stops too"

# Her app-menu entry (step 6, with --no-autostart too), her summon key as a GNOME custom shortcut
# (only on his yes at a terminal) and Hyprland's Super+Alt+C when Super+C is taken; --uninstall takes
# each away again. A home of its own under $tmp; gsettings is a stand-in (its values in GS; with
# GS_LOST=1 it says yes to a write and keeps nothing, as when dconf doesn't commit), and nothing here
# quits or kills anything.
h=$tmp/h2
mkdir -p "$h"
HOME=$h DATA=$h/.local/share CONF=$h/.config STATE=$h/.local/state CACHE_BASE=$h/.cache
CACHE_ROOT=$h/.cache/synaps-avatar CACHE=$h/.cache/synaps-avatar/installer BIN=$h/.local/bin
PACK=$h/.local/share/synaps-avatar/packs/capri RECEIPT=$h/.local/share/synaps-avatar/install.receipt
PREFIXES=("$h/.local") CARGO_BIN=$h/.cargo/bin TMP=$tmp DRY=0 YES=0 VOICE=0 UNINSTALL=0 PURGE=0 SYSTEMD=1 WMCONF=''
RUN_REAL=1 FAKE=()
mkdir -p "$PACK" "$BIN"
: >"$PACK/icon.png"
declare -A GS=()
GS_WM='' GS_EXT='' GS_EGO='' GS_APP='' GS_LOST=0
# The system's schemas: two with shortcuts, and an app's (its own shortcuts, in its window only).
SYS=(org.gnome.desktop.wm.keybindings org.gnome.shell.extensions.tiling-assistant org.example.Editor)
# A string as gsettings set takes one ('...' or "...", a backslash before a quote or a backslash),
# and as gsettings prints one: GLib's way, in single quotes, or in double ones when it holds a single
# quote (a backslash before that quote and before a backslash).
gs_in() {
    local s=${1:1:${#1}-2} out='' c
    while [ -n "$s" ]; do
        c=${s:0:1} s=${s:1}
        if [ "$c" = "\\" ]; then c=${s:0:1} s=${s:1}; fi
        out+=$c
    done
    printf '%s' "$out"
}
gs_out() {
    local s=${1//\\/\\\\}
    case $1 in
        *"'"*) printf '"%s"' "${s//\"/\\\"}" ;;
        *) printf "'%s'" "$s" ;;
    esac
}
# GS holds a list as gsettings prints it, a string as it is.
gsettings() {
    local dir='' own=() ids=() s
    if [ "$1" = --schemadir ]; then dir=$2 && shift 2; fi
    # An extension of his own: its schemas only in its folder (--schemadir), whose source has the
    # system's as its parent.
    if [ "$dir" = "$DATA/gnome-shell/extensions/ego@x/schemas" ]; then own=(org.gnome.shell.extensions.ego); fi
    case $1 in
        set | reset-recursively) if [ "$GS_LOST" = 1 ]; then return 0; fi ;;
    esac
    case $1 in
        writable) echo true ;;
        list-schemas) printf '%s\n' "${SYS[@]}" "${own[@]}" ;;
        get)
            if [ "$3" = custom-keybindings ]; then
                echo "${GS["$2 $3"]:-@as []}"
            else
                gs_out "${GS["$2 $3"]:-}" && echo
            fi
            ;;
        # A list reads back in single quotes, as gsettings prints one.
        set) if [ "$3" = custom-keybindings ]; then GS["$2 $3"]=${4//\"/\'}; else GS["$2 $3"]=$(gs_in "$4"); fi ;;
        reset-recursively) for k in "${!GS[@]}"; do case $k in "$2 "*) unset "GS[$k]" ;; esac; done ;;
        list-recursively)
            # With no schema: every one its source lists, the system's too, sorted. GLib before 2.82
            # (Ubuntu 24.04's 2.80) opens each from the default source: it prints the ones sorting
            # before the folder's own, then aborts on that.
            if [ $# -ge 2 ]; then ids=("$2"); else mapfile -t ids < <(printf '%s\n' "${SYS[@]}" "${own[@]}" | LC_ALL=C sort); fi
            for s in "${ids[@]}"; do
                case $s in
                    org.gnome.desktop.wm.keybindings) printf '%s\n' "$s close ['<Alt>F4']" "$GS_WM" ;;
                    org.gnome.shell.extensions.tiling-assistant) printf '%s\n' "$s tile-left-half ['<Super>Left']" "$GS_EXT" ;;
                    org.example.Editor) printf '%s\n' "$s find ['<Primary>f']" "$GS_APP" ;;
                    org.gnome.shell.extensions.ego)
                        if [ $# -lt 2 ]; then return 133; fi
                        if [ ${#own[@]} -gt 0 ]; then printf '%s\n' "$GS_EGO"; fi
                        ;;
                esac
            done
            ;;
    esac
}
gk() { gsettings get "$GSCHEMA.custom-keybinding:$1" "$2"; }
C0=/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/

# The app-menu entry: written with --no-autostart, shown, her pack's icon, her start command.
app=$DATA/applications/synaps-avatar.desktop
out=$(AUTOSTART=0 DESKTOP=other step_autostart 2>&1)
check "$([ -f "$app" ] && [ ! -e "$CONF/autostart/synaps-avatar.desktop" ] && echo 0)" "--no-autostart: her app-menu entry all the same, no autostart one"
check "$(grep -q NoDisplay "$app" || echo 0)" "it shows in the menu (no NoDisplay)"
check "$(grep -qx Name=Capri "$app" && grep -qx 'Comment=Capri, your desktop companion (synaps-avatar)' "$app" && echo 0)" "named Capri, the binary's name in its Comment"
check "$(grep -qx "Icon=$PACK/icon.png" "$app" && echo 0)" "her pack's icon"
check "$(grep -qx "Exec=env SA_BACKEND=x11 $BIN/synaps-avatar" "$app" && grep -qx "TryExec=$BIN/synaps-avatar" "$app" && echo 0)" "her start command (XWayland off Hyprland and i3): $(grep '^Exec=' "$app")"
check "$(grep -qxF "file	$app" "$RECEIPT" && echo 0)" "in the receipt"
mkdir -p "$CONF/synaps-avatar"
: >"$CONF/synaps-avatar/icon.png"
out=$(AUTOSTART=1 DESKTOP=other step_autostart 2>&1)
check "$(grep -qx "Icon=$CONF/synaps-avatar/icon.png" "$app" && echo 0)" "his own icon wins, as for her window"
check "$(grep -qx NoDisplay=true "$CONF/autostart/synaps-avatar.desktop" && echo 0)" "autostart: its own entry, hidden from the menu"
check "$([ "$(grep -E '^(Name|Comment)=' "$app")" = "$(grep -E '^(Name|Comment)=' "$CONF/autostart/synaps-avatar.desktop")" ] && echo 0)" "the same Name and Comment in both"
check "$(grep -qx "TryExec=$BIN/synaps-avatar" "$CONF/autostart/synaps-avatar.desktop" && echo 0)" \
    "autostart: TryExec too (one left behind starts nothing once she's gone)"
check "$([ -z "$(compgen -G "$app.synaps-avatar-*.bak")" ] && [ -z "$(compgen -G "$CONF/autostart/synaps-avatar.desktop.synaps-avatar-*.bak")" ] && echo 0)" \
    "hers written again: no backup"
# His entry where hers goes (his own, or hers once KDE's menu editor rewrote it: no header): backed
# up first, as his configs are. In a home of its own.
his_entries() {
    local DATA=$tmp/h3/share CONF=$tmp/h3/config RECEIPT=$tmp/h3/receipt f b
    mkdir -p "$DATA/applications" "$CONF/autostart"
    printf '%s\n' '[Desktop Entry]' 'Name=Capri (his)' | tee "$DATA/applications/synaps-avatar.desktop" >"$CONF/autostart/synaps-avatar.desktop"
    AUTOSTART=1 DESKTOP=other step_autostart >/dev/null 2>&1
    for f in "$DATA/applications/synaps-avatar.desktop" "$CONF/autostart/synaps-avatar.desktop"; do
        b=$(compgen -G "$f.synaps-avatar-*.bak") && ours "$f" && grep -qx 'Name=Capri (his)' "$b" && grep -qxF "backup	$b" "$RECEIPT" || return 0
    done
    echo 0
}
check "$(his_entries)" "an entry there that isn't hers (his, or KDE's rewrite): a backup first, in the receipt; then hers"

# Hyprland as her binary reads it (linux::hyprland_in): a signature naming one that runs on this
# display, or the desktop variables while one does. One runs while its lock's PID is a Hyprland (by
# its name: sleep run as Hyprland here, and as Nix's .Hyprland-wrapped), else not (a crash leaves the
# lock and a socket behind; its PID may be another process's by now); with no lock it doesn't (a
# shutdown that crashed after removing it leaves the socket). Signatures and desktop variables left
# over aren't Hyprland.
sock() { python3 -c 'import socket, sys; socket.socket(socket.AF_UNIX).bind(sys.argv[1])' "$1"; }
sleep 0 &
gone=$!
wait "$gone"
mkdir -p "$tmp/hy"
for n in Hyprland .Hyprland-wrapped; do
    cp "$(command -v sleep)" "$tmp/hy/$n"
    "$tmp/hy/$n" 600 &
    pids+=("$!")
done
hyp=${pids[-2]} nixp=${pids[-1]}
for _ in {1..100}; do
    [ "$(cat "/proc/$hyp/comm" "/proc/$nixp/comm" 2>/dev/null)" = $'Hyprland\n.Hyprland-wrapp' ] && break
    sleep 0.05
done
# rt3: one with no lock alone; rt2: none that runs; rt4: one running in a folder named by a blank
# signature (a space and a tab).
blank=$(printf ' \t')
mkdir -p "$tmp/rt/hypr/live_1" "$tmp/rt/hypr/crashed_2" "$tmp/rt/hypr/bad_4" "$tmp/rt/hypr/reused_5" "$tmp/rt/hypr/nix_6" \
    "$tmp/rt2/hypr/crashed_2" "$tmp/rt3/hypr/torn_3" "$tmp/rt4/hypr/$blank"
printf '%s\nwayland-1\n' "$hyp" >"$tmp/rt4/hypr/$blank/hyprland.lock"
printf '%s\nwayland-1\n' "$hyp" >"$tmp/rt/hypr/live_1/hyprland.lock"
printf '%s\nwayland-1\n' "$gone" >"$tmp/rt/hypr/crashed_2/hyprland.lock"
cp "$tmp/rt/hypr/crashed_2/hyprland.lock" "$tmp/rt2/hypr/crashed_2/"
printf 'self\nwayland-1\n' >"$tmp/rt/hypr/bad_4/hyprland.lock"
printf '%s\nwayland-1\n' "$$" >"$tmp/rt/hypr/reused_5/hyprland.lock"
printf '%s\nwayland-1\n' "$nixp" >"$tmp/rt/hypr/nix_6/hyprland.lock"
for d in rt/hypr/live_1 rt/hypr/crashed_2 rt/hypr/bad_4 rt/hypr/reused_5 rt2/hypr/crashed_2 rt3/hypr/torn_3; do sock "$tmp/$d/.socket.sock"; done
# de VAR=VALUE...: the desktop, those set (no i3 running).
de() {
    (
        # shellcheck disable=SC2317,SC2329 # desktop's pgrep
        pgrep() { return 1; }
        unset CAPRI_DESKTOP XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP HYPRLAND_INSTANCE_SIGNATURE WAYLAND_DISPLAY SWAYSOCK I3SOCK
        # shellcheck disable=SC2163 # "$@" are VAR=VALUE
        export XDG_RUNTIME_DIR=$tmp/rt "$@"
        desktop
        echo "$DESKTOP"
    )
}
# is WANT VAR=VALUE...: whether the desktop those make is WANT.
is() { [ "$(de "${@:2}")" = "$1" ]; }
hy=HYPRLAND_INSTANCE_SIGNATURE xd=XDG_CURRENT_DESKTOP
check "$(is hyprland WAYLAND_DISPLAY=wayland-1 $hy=live_1 && echo 0)" "a signature whose lock names a Hyprland that runs, on this display: Hyprland"
check "$(is hyprland WAYLAND_DISPLAY=wayland-1 $hy=nix_6 && echo 0)" "Nix's (.Hyprland-wrapped) too"
check "$(is hyprland WAYLAND_DISPLAY=/run/user/1000/wayland-1 $hy=live_1 && is hyprland $hy=live_1 && echo 0)" "its display by name (a path, or no WAYLAND_DISPLAY: any)"
check "$(is other WAYLAND_DISPLAY=wayland-1 $hy=crashed_2 $xd=GNOME && echo 0)" "its lock's PID gone (a crash: lock and socket still there): not Hyprland"
check "$(is other WAYLAND_DISPLAY=wayland-0 $hy=old_9 $xd=GNOME && echo 0)" "a signature with nothing there: not Hyprland"
check "$(is other WAYLAND_DISPLAY=wayland-1 $hy=reused_5 $xd=GNOME && echo 0)" "its lock's PID another process's by now (a crash, the PID reused): not Hyprland"
check "$(is other WAYLAND_DISPLAY=wayland-1 $hy=bad_4 && echo 0)" "a lock that doesn't read as one (self, not a PID): not Hyprland"
check "$(is other WAYLAND_DISPLAY=wayland-0 $hy=torn_3 $xd=Hyprland XDG_RUNTIME_DIR="$tmp/rt3" && echo 0)" \
    "no lock (a shutdown that crashed after removing it: its socket left): not Hyprland"
check "$(is other WAYLAND_DISPLAY=wayland-1 "$hy=$blank" $xd=GNOME XDG_RUNTIME_DIR="$tmp/rt4" && echo 0)" "a blank signature (a space, a tab) isn't one, as her binary trims it"
check "$(is other WAYLAND_DISPLAY=wayland-2 $hy=live_1 $xd=Hyprland && echo 0)" "a compositor nested in Hyprland (another display): not Hyprland"
check "$(is hyprland WAYLAND_DISPLAY=wayland-1 $xd=Hyprland && is hyprland WAYLAND_DISPLAY=wayland-1 XDG_SESSION_DESKTOP=hyprland && echo 0)" \
    "no signature (lost), the desktop says Hyprland and one runs on this display: Hyprland"
check "$(is other WAYLAND_DISPLAY=wayland-0 $hy=crashed_2 $xd=Hyprland && echo 0)" "the desktop says Hyprland, one runs but on another display (tmux on GNOME): not Hyprland"
check "$(is other WAYLAND_DISPLAY=wayland-1 $hy=crashed_2 $xd=Hyprland XDG_RUNTIME_DIR="$tmp/rt2" && echo 0)" "nor with none running (tmux, its WAYLAND_DISPLAY left over too)"
out=$(
    # shellcheck disable=SC2317,SC2329 # desktop's pgrep
    pgrep() { return 1; }
    unset CAPRI_DESKTOP XDG_SESSION_DESKTOP $hy
    XDG_RUNTIME_DIR=$tmp/rt2 WAYLAND_DISPLAY=wayland-1 XDG_CURRENT_DESKTOP=Hyprland step_summon 2>&1
)
case $out in *'desktop: says Hyprland (Hyprland:), but none runs on this display: left over'*'talk tap'*) r=0 ;; *) r=1 ;; esac
check $r "and the summon step says so"

# GNOME: a custom shortcut, never on --yes, never without a terminal, default no.
export XDG_CURRENT_DESKTOP=ubuntu:GNOME CAPRI_DESKTOP=other
check "$(ok on_gnome)" "ubuntu:GNOME is GNOME"
check "$([ "$(XDG_CURRENT_DESKTOP=KDE ok on_gnome)" = 1 ] && echo 0)" "KDE isn't"
out=$(YES=1 confirm_tty 'add it' 2>&1)
r=$?
check "$([ $r = 1 ] && case $out in *"--yes doesn't answer"*) echo 0 ;; esac)" "--yes doesn't answer it"
out=$(setsid -w bash -c 'source <(sed "\$d" "$1"); set +e; ui; YES=0 DRY=0; confirm_tty "add it"; echo "rc=$?"' _ "$here/install.sh" </dev/null 2>&1)
case $out in *'no terminal to ask on: skipped (add it)'*rc=1) r=0 ;; *) r=1 ;; esac
check $r "no terminal: not asked, not done"
confirm_tty() { echo "asked: $1"; [ "$ANSWER" = y ]; }
gsettings set "$GSCHEMA.custom-keybinding:$C0" command "'kgx'"
gsettings set "$GSCHEMA.custom-keybinding:$C0" binding "'<Super>Return'"
gsettings set "$GSCHEMA" custom-keybindings "['$C0']"
ANSWER=n SUMMON='' SUMMON_KEY=''
step_summon >"$tmp/out" 2>&1
case $(cat "$tmp/out") in *'asked: add a GNOME keyboard shortcut: Super+Alt+C'*'Custom Shortcuts'*"$BIN/avatarctl talk tap"*) r=0 ;; *) r=1 ;; esac
check $r "asked; his no: how to bind it himself"
check "$([ "$(gsettings get "$GSCHEMA" custom-keybindings)" = "['$C0']" ] && [ "$(gk "$GKEY" command)" = "''" ] && echo 0)" "and nothing set"
ANSWER=y
step_summon >"$tmp/out" 2>&1
check "$([ "$SUMMON_KEY" = Super+Alt+C ] && echo 0)" "his yes: Super+Alt+C ($SUMMON)"
check "$([ "$(gsettings get "$GSCHEMA" custom-keybindings)" = "['$C0', '$GKEY']" ] && echo 0)" "added to his list, his own kept: $(gsettings get "$GSCHEMA" custom-keybindings)"
check "$([ "$(gk "$GKEY" command)" = "'$BIN/avatarctl talk tap'" ] && [ "$(gk "$GKEY" binding)" = "'<Super><Alt>c'" ] && echo 0)" "running avatarctl talk tap on <Super><Alt>c (as gsettings prints them: $(gk "$GKEY" command))"
q2="'/My Apps/bin/avatarctl' talk tap"
check "$([ "$(gs_out "$q2")" = "\"$q2\"" ] && [ "$(gstr "$(gs_out "$q2")")" = "$q2" ] && echo 0)" \
    "gstr reads GLib's other quotes too (a single quote inside: \"...\")"
check "$(grep -qxF "gkey	$GKEY" "$RECEIPT" && echo 0)" "in the receipt"
SUMMON='' SUMMON_KEY=''
step_summon >"$tmp/out" 2>&1
case $(cat "$tmp/out") in *asked:*) r=1 ;; *'already a GNOME shortcut: <Super><Alt>c runs'*) r=0 ;; *) r=1 ;; esac
check "$([ $r = 0 ] && [ "$SUMMON_KEY" = Super+Alt+C ] && echo 0)" "run again: already there, not asked"
GS_WM="org.gnome.desktop.wm.keybindings toggle-maximized ['<Alt><Super>c']"
GS["$GSCHEMA.custom-keybinding:$GKEY command"]=/usr/bin/true
step_summon >"$tmp/out" 2>&1
case $(cat "$tmp/out") in *asked:*) r=1 ;; *'Super+Alt+C is taken: org.gnome.desktop.wm.keybindings toggle-maximized'*) r=0 ;; *) r=1 ;; esac
check $r "Super+Alt+C taken (any modifier order): not asked"
GS_WM='' GS_EXT="org.gnome.shell.extensions.tiling-assistant restore-window ['<Mod1><Mod4>c']"
step_summon >"$tmp/out" 2>&1
case $(cat "$tmp/out") in *asked:*) r=1 ;; *'Super+Alt+C is taken: org.gnome.shell.extensions.tiling-assistant restore-window'*) r=0 ;; *) r=1 ;; esac
check $r "taken by a Shell extension, as <Mod1><Mod4>c: not asked"
GS_EXT=''
mkdir -p "$DATA/gnome-shell/extensions/ego@x/schemas"
: >"$DATA/gnome-shell/extensions/ego@x/schemas/gschemas.compiled"
GS_EGO="org.gnome.shell.extensions.ego toggle ['<Super><Alt>c']"
step_summon >"$tmp/out" 2>&1
case $(cat "$tmp/out") in *asked:*) r=1 ;; *'Super+Alt+C is taken: org.gnome.shell.extensions.ego toggle'*) r=0 ;; *) r=1 ;; esac
check $r "taken by an extension he installed himself (its schemas in its own folder): not asked"
GS_EGO='' GS_APP="org.example.Editor find-in-files ['<Super><Alt>c']"
case $(gnome_taken) in *org.example.Editor*) r=1 ;; *) r=0 ;; esac
check $r "an app's own shortcut isn't read, through his extension's folder either (that folder's own schemas only)"
GS_APP=''
rm -rf "$DATA/gnome-shell"
GS["$GSCHEMA.custom-keybinding:$GKEY command"]="$BIN/avatarctl talk tap" GS_WM=''
# gsettings says yes and keeps nothing (dconf didn't commit it): read back, he's told, with the
# commands to run by hand. In a subshell: her shortcut stays set out here.
out=$(
    GS["$GSCHEMA custom-keybindings"]="['$C0']"
    for k in name command binding; do unset "GS[$GSCHEMA.custom-keybinding:$GKEY $k]"; done
    GS_LOST=1 ANSWER=y SUMMON='' SUMMON_KEY=''
    step_summon 2>&1
    echo "SUMMON_KEY=$SUMMON_KEY"
)
case $out in
    *"gsettings didn't keep it"*"binding '\"<Super><Alt>c\"'"*"custom-keybindings '[\"$C0\", \"$GKEY\"]'"*'Custom Shortcuts'*SUMMON_KEY=) r=0 ;;
    *) r=1 ;;
esac
check $r "gsettings kept nothing: read back, he's told, the commands by hand"
unset XDG_CURRENT_DESKTOP

# Hyprland: its stock Super+C closes a window, so Super+Alt+C (Windows' key), when that's free.
export CAPRI_DESKTOP=hyprland
# Never query a real compositor from unit tests. Failed queries exercise the static fallback.
hyprctl() {
    [ "$*" = 'binds -j' ] && [ -n "${HYPR_TEST_BINDS:-}" ] || return 1
    printf '%s\n' "$HYPR_TEST_BINDS"
}
mkdir -p "$CONF/hypr"
hc=$CONF/hypr/hyprland.conf
hypr() {
    printf '%s\n' '$mainMod = SUPER' "$@" 'bind = $mainMod, Q, exec, kitty' >"$hc"
    SUMMON='' SUMMON_KEY=''
    step_summon >"$tmp/out" 2>&1
}
confirm() { echo "asked: $1"; return "${CONFIRM_RESULT:-0}"; }
hypr 'bind = $mainMod, C, killactive,'
check "$(grep -q 'Hyprland 0.57+ ignores it' "$tmp/out" && echo 0)" "legacy-only config warns that Hyprland 0.57 ignores it"
check "$([ "$SUMMON_KEY" = Super+Alt+C ] && grep -qx "bind = SUPER ALT, C, exec, $BIN/avatarctl talk press" "$hc" &&
    grep -qx "bindr = SUPER ALT, C, exec, $BIN/avatarctl talk release" "$hc" && grep -qx 'bind = $mainMod, C, killactive,' "$hc" && echo 0)" \
    "Super+C taken (killactive): Super+Alt+C, his line kept"
VOICE=1 hypr 'bind = $mainMod, C, killactive,'
check "$(grep -qx "bind = SUPER ALT CTRL, C, exec, $BIN/avatarctl talk toggle" "$hc" && echo 0)" "--voice: Super+Alt+Ctrl+C toggles her mic"
hypr 'bind = SUPER, C, killactive,' 'bind = $mainMod ALT, C, exec, foo'
case $(cat "$tmp/out") in *'Super+Alt+C is taken too: bind = $mainMod ALT, C, exec, foo'*'pick another key'*) r=0 ;; *) r=1 ;; esac
check "$([ $r = 0 ] && [ -z "$SUMMON_KEY" ] && ! grep -q avatarctl "$hc" && echo 0)" "both taken: nothing added, he picks one"
hypr 'bind = $mainMod, X, killactive,'
check "$([ "$SUMMON_KEY" = Super+C ] && grep -qx "bind = SUPER, C, exec, $BIN/avatarctl talk press" "$hc" && echo 0)" "Super+C free: Super+C, as before"
VOICE=1 hypr 'bind = CTRL $mainMod, C, exec, his-command'
check "$([ "$SUMMON_KEY" = Super+Alt+C ] && grep -q 'Super+Ctrl+C is taken' "$tmp/out" &&
    grep -qx "bind = SUPER ALT CTRL, C, exec, $BIN/avatarctl talk toggle" "$hc" && echo 0)" "legacy --voice: Ctrl collision moves both chords to Alt"
VOICE=1 hypr 'bind = SUPER, C, killactive,' 'bind = CTRL ALT $mainMod, C, exec, his-command'
check "$([ -z "$SUMMON_KEY" ] && ! grep -q avatarctl "$hc" && grep -q 'Super+Alt+Ctrl+C is taken too' "$tmp/out" && echo 0)" "legacy --voice: fallback mic collision leaves config alone"
HYPR_TEST_BINDS="[{\"modmask\":64,\"key\":\"C\",\"dispatcher\":\"exec\",\"arg\":\"$BIN/avatarctl talk press\"}, {\"modmask\":68,\"key\":\"C\",\"dispatcher\":\"exec\",\"arg\":\"$BIN/avatarctl talk toggle\"}]" \
    VOICE=1 hypr '# reinstall'
check "$([ "$SUMMON_KEY" = Super+C ] && echo 0)" "legacy: exact exec/arg self-skip retained"
hypr 'bind = $mainMod, C, killactive,'

# Hyprland 0.5x: the Lua config wins, including when both formats exist. Never evaluate his Lua.
hl=$CONF/hypr/hyprland.lua
cp "$hc" "$tmp/conf-before-lua"
hypr_lua_config() {
    printf '%s\n' 'local mainMod = "SUPER"' "$@" 'hl.bind(mainMod .. " + Q", hl.dsp.exec_cmd("kitty"))' >"$hl"
    SUMMON='' SUMMON_KEY=''
    step_summon >"$tmp/out" 2>&1
}
hypr_lua_config '-- hl.bind("SUPER + C", hl.dsp.no_op())'
check "$([ "$WMCONF" = "$hl" ] && [ "$SUMMON_KEY" = Super+C ] && cmp -s "$hc" "$tmp/conf-before-lua" &&
    grep -qxF "hl.bind(\"SUPER + C\", hl.dsp.exec_cmd(\"$BIN/avatarctl talk press\"), { description = \"synaps-avatar summon\" })" "$hl" &&
    grep -qxF "hl.bind(\"SUPER + C\", hl.dsp.exec_cmd(\"$BIN/avatarctl talk release\"), { release = true, description = \"synaps-avatar summon\" })" "$hl" &&
    ! grep -q 'talk toggle' "$hl" && echo 0)" "Lua preferred over .conf: literal Super+C press/release, comments ignored, no toggle without --voice"
check "$(grep -qxF -- '-- >>> synaps-avatar (install.sh): summon >>>' "$hl" &&
    grep -qxF -- '-- <<< synaps-avatar (install.sh): summon <<<' "$hl" &&
    grep -qF 'block' "$RECEIPT" && echo 0)" "Lua uses the existing receipt/markers with Lua comments"
check "$(cmp -s "$(find "$CONF/hypr" -name 'hyprland.lua.*.bak' | head -n1)" <(block_drop "$hl" summon) && echo 0)" "Lua: backup contains the original config"
cp "$hl" "$tmp/lua-once"
step_summon >"$tmp/out" 2>&1
check "$(cmp -s "$hl" "$tmp/lua-once" && [ "$SUMMON_KEY" = Super+C ] && echo 0)" "Lua: reinstall ignores and replaces only her own block, without duplication"
VOICE=1 hypr_lua_config 'local closeWindowBind = hl.bind(mainMod .. " + C", hl.dsp.window.close())'
check "$([ "$SUMMON_KEY" = Super+Alt+C ] &&
    grep -qxF "hl.bind(\"SUPER + ALT + CTRL + C\", hl.dsp.exec_cmd(\"$BIN/avatarctl talk toggle\"), { description = \"synaps-avatar summon\" })" "$hl" &&
    grep -qxF 'local closeWindowBind = hl.bind(mainMod .. " + C", hl.dsp.window.close())' "$hl" && echo 0)" "Lua stock mainMod+C taken: Super+Alt+C, --voice adds Ctrl, his line kept"
hypr_lua_config "hl.bind(' SUPER+c ', hl.dsp.no_op())" "hl.bind('alt + super + C', hl.dsp.no_op())"
check "$([ -z "$SUMMON_KEY" ] && ! grep -q avatarctl "$hl" && grep -q 'Super+Alt+C is taken too' "$tmp/out" && echo 0)" "Lua: both literal keys taken (case/spacing/order/single quotes), neither overridden"
hypr_lua_config 'hl.bind("WIN + C", hl.dsp.no_op())' 'hl.bind(mainMod .. " + ALT + C", hl.dsp.no_op())'
check "$([ -z "$SUMMON_KEY" ] && ! grep -q avatarctl "$hl" && echo 0)" "Lua: mainMod+Alt+C fallback taken too"
for alias in LOGO MOD4 META; do
    hypr_lua_config "if true then hl.bind([[$alias + C]], hl.dsp.window.close()) end"
    check "$([ "$SUMMON_KEY" = Super+Alt+C ] && echo 0)" "Lua: inline conditional, long string, $alias alias collision"
done
hypr_lua_config '--[=[' 'hl.bind("SUPER + C", hl.dsp.window.close())' ']=]' \
    'local text = [[hl.bind("SUPER + C", not_code)]]' 'local other = 1 -- hl.bind("SUPER + C", not_code)'
check "$([ "$SUMMON_KEY" = Super+C ] && echo 0)" "Lua: long comments, strings and trailing comments are not binds"
for computed in 'hl.bind(key("C"), hl.dsp.window.close())' 'for _, k in ipairs(keys) do hl.bind(k, action) end' 'require("my-keys")' \
    'local bind = hl.bind; bind("SUPER + C", action)' 'hl["bind"]("SUPER + C", action)'; do
    hypr_lua_config "$computed"
    check "$([ -z "$SUMMON_KEY" ] && ! grep -q avatarctl "$hl" && grep -qF "$computed" "$tmp/out" &&
        grep -q 'cannot determine Lua keys without live hyprctl' "$tmp/out" && echo 0)" "Lua: unresolved keys/imports need a human without live binds: $computed"
done
HYPR_TEST_BINDS='[{"modmask":64,"key":"c","dispatcher":"killactive","arg":""}]' \
    hypr_lua_config 'hl.bind(key("C"), hl.dsp.window.close())'
check "$([ "$SUMMON_KEY" = Super+Alt+C ] && grep -q 'hyprctl:' "$tmp/out" && echo 0)" "Lua: live modmask/key catches helper collision"
HYPR_TEST_BINDS='[{"modmask":64,"key":"C"},{"modmask":72,"key":"c"}]' \
    hypr_lua_config 'require("my-keys")'
check "$([ -z "$SUMMON_KEY" ] && ! grep -q avatarctl "$hl" && echo 0)" "Lua: live bindings protect both keys, including imported bindings"
HYPR_TEST_BINDS='[{"modmask":64,"key":"","keycode":54},{"modmask":68,"key":"C"}]' \
    hypr_lua_config 'hl.bind(key("C"), hl.dsp.window.close())'
check "$([ "$SUMMON_KEY" = Super+Alt+C ] && echo 0)" "Lua: live keycode binding protects C, other modifiers don't claim Alt+C"
HYPR_TEST_BINDS='[]' hypr_lua_config 'hl.bind("SUPER + C", hl.dsp.window.close())'
check "$([ "$SUMMON_KEY" = Super+C ] && echo 0)" "Lua: live state is primary (a text binding can be disabled)"
HYPR_TEST_BINDS='not JSON' hypr_lua_config 'local closeWindowBind = hl.bind(mainMod .. " + C", hl.dsp.window.close())'
check "$([ "$SUMMON_KEY" = Super+Alt+C ] && echo 0)" "Lua: invalid hyprctl response falls back to text"
HYPR_TEST_BINDS='[{"modmask":64,"key":"C","dispatcher":"HL.Dispatcher(exec_cmd)","arg":123,"description":"synaps-avatar summon"}, {"modmask":64,"key":"C","dispatcher":"HL.Dispatcher(exec_cmd)","arg":124,"release":true,"description":"synaps-avatar summon"}, {"modmask":68,"key":"C","dispatcher":"HL.Dispatcher(exec_cmd)","arg":125,"description":"synaps-avatar summon"}]' \
    VOICE=1 hypr_lua_config '-- reinstall'
check "$([ "$SUMMON_KEY" = Super+C ] && echo 0)" "Lua: live 0.57.2 installer bindings don't collide on reinstall"
# Reconstruct the compositor's opaque Lua entries from each install, including descriptions.
# Re-read after EVERY run, so a key flip (or missing tag) cannot hide behind a fixed JSON fixture.
hypr_lua_live_snapshot() {
    python3 - "$hl" "$1" <<'PYSNAPSHOT'
import json, re, sys
binds = json.loads(sys.argv[2])
for line in open(sys.argv[1]):
    if not re.search(r" talk (press|release|toggle)", line):
        continue
    keys = re.search(r'hl.bind\("([^"]+)"', line).group(1).split(" + ")
    description = re.search(r'description = "([^"]+)"', line)
    binds.append({"modmask": sum({"SUPER": 64, "ALT": 8, "CTRL": 4}[k] for k in keys[:-1]),
                  "key": keys[-1], "dispatcher": "HL.Dispatcher(exec_cmd)", "arg": 100 + len(binds),
                  "release": "release = true" in line,
                  "description": description.group(1) if description else ""})
print(json.dumps(binds))
PYSNAPSHOT
}
for style in helpers stock; do
    if [ "$style" = helpers ]; then
        user_binds='[{"modmask":64,"key":"Q","dispatcher":"HL.Dispatcher(exec_cmd)","arg":90}]'
        config='local function key(k) return "SUPER + " .. k end; hl.bind(key("Q"), hl.dsp.exec_cmd("kitty"))'
        expected=Super+C
    else
        user_binds='[{"modmask":64,"key":"C","dispatcher":"HL.Dispatcher(window.close)","arg":91,"description":"Close window"}]'
        config='local closeWindowBind = hl.bind(mainMod .. " + C", hl.dsp.window.close())'
        expected=Super+Alt+C
    fi
    HYPR_TEST_BINDS=$user_binds VOICE=1 hypr_lua_config "$config"
    cp "$hl" "$tmp/lua-stable"
    check "$([ "$SUMMON_KEY" = "$expected" ] && [ "$(grep -c 'description = "synaps-avatar summon"' "$hl")" = 3 ] && echo 0)" "Lua $style: first install tags press, release and toggle"
    for rerun in 1 2; do
        live=$(hypr_lua_live_snapshot "$user_binds")
        SUMMON='' SUMMON_KEY=''
        HYPR_TEST_BINDS=$live VOICE=1 step_summon >"$tmp/out" 2>&1
        check "$([ "$SUMMON_KEY" = "$expected" ] && cmp -s "$hl" "$tmp/lua-stable" && echo 0)" "Lua $style: live reinstall $rerun keeps $expected and identical config"
    done
done
# Mic chords are checked with the summon chord; fall back as a pair, or leave BOTH untouched.
VOICE=1 hypr_lua_config 'hl.bind("CONTROL + SUPER + C", hl.dsp.no_op())'
check "$([ "$SUMMON_KEY" = Super+Alt+C ] && grep -q 'Super+Ctrl+C is taken' "$tmp/out" &&
    grep -q 'SUPER + ALT + CTRL + C' "$hl" && echo 0)" "Lua --voice: offline Ctrl collision moves summon and mic to Alt"
VOICE=1 hypr_lua_config 'hl.bind("SUPER + C", hl.dsp.no_op())' 'hl.bind("CTRL + ALT + SUPER + C", hl.dsp.no_op())'
check "$([ -z "$SUMMON_KEY" ] && ! grep -q avatarctl "$hl" && grep -q 'Super+Alt+Ctrl+C is taken too' "$tmp/out" && echo 0)" "Lua --voice: offline fallback mic collision leaves config alone"
for mask in 68 76; do
    if [ "$mask" = 68 ]; then extra='' expected=Super+Alt+C; else extra=',{"modmask":64,"key":"C"}' expected=''; fi
    HYPR_TEST_BINDS="[{\"modmask\":$mask,\"key\":\"C\",\"dispatcher\":\"HL.Dispatcher(exec_cmd)\",\"arg\":42,\"description\":\"His shortcut\"}$extra]" \
        VOICE=1 hypr_lua_config 'require("my-keys")'
    check "$([ "$SUMMON_KEY" = "$expected" ] && grep -q 'Ctrl+C is taken' "$tmp/out" && echo 0)" "Lua --voice: live mask $mask protects mic chord (not her description)"
done
HYPR_TEST_BINDS='[{"modmask":68,"key":"C"},{"modmask":76,"key":"C"}]' \
    VOICE=1 hypr_lua_config '-- both mic chords taken'
check "$([ -z "$SUMMON_KEY" ] && ! grep -q avatarctl "$hl" && echo 0)" "Lua --voice: both mic chords occupied, no conflicting binds written"
hypr_lua_config 'hl.bind("SUPER + CTRL + C", hl.dsp.no_op())'
check "$([ "$SUMMON_KEY" = Super+C ] && ! grep -q 'talk toggle' "$hl" && echo 0)" "Lua: mic collision irrelevant without --voice"
# Offline stock still names its known collision, suggests Alt, and caps unresolved helper lines.
helpers=()
for i in {1..12}; do helpers+=("hl.bind(key$i(\"Q\"), action)"); done
hypr_lua_config 'local closeWindowBind = hl.bind(mainMod .. " + C", hl.dsp.window.close())' "${helpers[@]}"
check "$([ -z "$SUMMON_KEY" ] && ! grep -q avatarctl "$hl" && grep -q 'Super+C is taken: local closeWindowBind' "$tmp/out" &&
    grep -q 'hl.bind("SUPER + ALT + C"' "$tmp/out" && ! grep -q 'hl.bind("SUPER + C"' "$tmp/out" &&
    [ "$(grep -c 'hl.bind(key' "$tmp/out")" = 5 ] && grep -q 'more unresolved lines omitted' "$tmp/out" && echo 0)" "Lua offline stock: Alt example, known clash named, only five helper lines"
# Hand-written block from bella, never claimed/removed by the installer.
hypr_lua_config '-- >>> capri-syn summon >>>' \
    "hl.bind(key(\"C\"), hl.dsp.exec_cmd(\"$BIN/capri-ctl talk press\"))" \
    '-- <<< capri-syn summon <<<'
check "$([ "$SUMMON" = 'your own binding' ] && ! grep -q 'synaps-avatar (install.sh)' "$hl" && echo 0)" "Lua: hand-written capri-syn summon block is already bound"
hypr_lua_config 'local mainMod = "ALT"' 'hl.bind(mainMod .. " + C", hl.dsp.window.close())'
check "$([ -z "$SUMMON_KEY" ] && grep -q 'cannot determine Lua keys' "$tmp/out" && echo 0)" "Lua: reassigned mainMod isn't assumed constant offline"
# Search each Lua location before legacy conf, including XDG_CONFIG_DIRS then /etc/xdg.
hypr_search_check() {
    local HOME=$tmp/config-search/home CONF=$tmp/config-search/xdg XDG_CONFIG_DIRS=$tmp/config-search/system
    local etc=$tmp/config-search/etc
    mkdir -p "$CONF/hypr" "$HOME/.config/hypr" "$XDG_CONFIG_DIRS/hypr" "$etc/hypr"
    touch "$CONF/hypr/hyprland.conf" "$etc/hypr/hyprland.lua"
    [ "$(hypr_config "$etc")" = "$etc/hypr/hyprland.lua" ] || return 1
    touch "$XDG_CONFIG_DIRS/hypr/hyprland.lua"
    [ "$(hypr_config "$etc")" = "$XDG_CONFIG_DIRS/hypr/hyprland.lua" ] || return 1
    touch "$HOME/.config/hypr/hyprland.lua"
    [ "$(hypr_config "$etc")" = "$HOME/.config/hypr/hyprland.lua" ] || return 1
    touch "$CONF/hypr/hyprland.lua"
    [ "$(hypr_config "$etc")" = "$CONF/hypr/hyprland.lua" ]
}
hypr_search_check
check $? "Lua: config search order XDG_CONFIG_HOME, HOME, XDG_CONFIG_DIRS, /etc/xdg, then .conf"
printf '%s' 'local mainMod = "SUPER" -- no final newline' >"$hl"
SUMMON='' SUMMON_KEY=''
step_summon >"$tmp/out" 2>&1
check "$([ "$SUMMON_KEY" = Super+C ] && grep -qxF -- '-- >>> synaps-avatar (install.sh): summon >>>' "$hl" && echo 0)" "Lua: missing final newline separated from marker"
if command -v luac >/dev/null 2>&1; then
    printf '%s\n' 'return {}' >"$hl"
    cp "$hl" "$tmp/lua-return"
    receipt_before=$(cat "$RECEIPT")
    SUMMON='' SUMMON_KEY=''
    step_summon >"$tmp/out" 2>&1
    check "$(cmp -s "$hl" "$tmp/lua-return" && [ -z "$SUMMON_KEY" ] &&
        [ "$(cat "$RECEIPT")" = "$receipt_before" ] && grep -q 'Lua syntax check failed; restored' "$tmp/out" && echo 0)" "Lua: top-level return restores pre-edit backup, no binding/receipt claimed"
fi
(
    # An older luac cannot judge the compositor's newer Lua syntax. Never run the post-check
    # if it rejects the original; this stand-in must be called once, before the new block exists.
    # shellcheck disable=SC2317,SC2329 # block_put calls this indirectly
    luac() { grep -q 'synaps-avatar (install.sh)' "$2" && echo after >>"$tmp/luac-calls"; echo before >>"$tmp/luac-calls"; return 1; }
    : >"$tmp/luac-calls"
    hypr_lua_config '-- syntax newer than local luac'
    [ "$SUMMON_KEY" = Super+C ] && [ "$(cat "$tmp/luac-calls")" = before ] &&
        grep -q 'skipping its syntax check' "$tmp/out" && ! grep -q 'restored' "$tmp/out"
)
check $? "Lua: unsupported original skips post-check instead of misleading rollback"
CONFIRM_RESULT=1 hypr_lua_config '-- his config'
check "$([ -z "$SUMMON_KEY" ] && ! grep -q avatarctl "$hl" && grep -q 'hl.bind("SUPER + C"' "$tmp/out" && echo 0)" "Lua: no consent, only shows valid Lua to add himself"
DRY=1 hypr_lua_config '-- his config'
check "$(! grep -q avatarctl "$hl" && echo 0)" "Lua: dry run leaves config alone"
hypr_lua_config "hl.bind(\"SUPER + X\", hl.dsp.exec_cmd(\"$BIN/avatarctl talk press\"))"
check "$([ "$SUMMON" = 'your own binding' ] && ! grep -q 'synaps-avatar (install.sh)' "$hl" && echo 0)" "Lua: his own summon binding is kept"
# Path quoting: a shell-quoted executable inside a Lua-quoted command, not executable Lua.
old_bin=$BIN
BIN="$tmp/My \"Apps\"/back\\slash"
VOICE=1 hypr_lua_config '-- his config'
check "$(grep -qxF "hl.bind(\"SUPER + CTRL + C\", hl.dsp.exec_cmd($(lua_string "$(q "$BIN/avatarctl") talk toggle")), { description = \"synaps-avatar summon\" })" "$hl" && echo 0)" "Lua: spaces, quotes and backslashes in the binary path escaped"
BIN=$old_bin
# Changing WMCONF also affects autostart: don't append exec-once (invalid Lua) after good binds.
step_autostart >"$tmp/out" 2>&1
check "$(grep -qF 'hl.on("hyprland.start", function() hl.exec_cmd(' "$hl" &&
    ! grep -q '^exec-once' "$hl" && [ -z "$(wm_grep 'hyprland.start' || true)" ] && echo 0)" "Lua: autostart uses the startup event, both marker blocks excluded from scans"
if command -v lua >/dev/null 2>&1; then
    lua - "$hl" <<'LUA'
local commands, releases, starts = {}, 0, 0
local started
hl = { dsp = { exec_cmd = function(cmd) return cmd end }, exec_cmd = function(cmd) started = cmd end }
hl.bind = function(keys, cmd, opts)
    assert(not keys:find("key(", 1, true))
    table.insert(commands, cmd)
    if cmd:find(" talk ", 1, true) then assert(opts.description == "synaps-avatar summon") end
    if opts and opts.release then releases = releases + 1 end
end
hl.on = function(event, callback) assert(event == "hyprland.start"); starts = starts + 1; callback() end
assert(loadfile(arg[1]))()
assert(#commands == 4 and releases == 1 and starts == 1)
assert(started and started:find("synaps-avatar", 1, true))
assert(commands[2]:find('My "Apps"/back\\slash/avatarctl', 1, true))
LUA
    check $? "Lua: generated config parses and dispatches with exact path bytes on a stub API, autostart callback runs"
else
    check 1 "Lua parse/runtime tests require lua (install lua5.4)"
fi
if ! command -v luac >/dev/null 2>&1; then
    check 1 "Lua syntax/rollback tests require luac (install lua5.4)"
fi
hypr_lua_config 'hl.bind("SUPER + C", hl.dsp.no_op())'
step_autostart >"$tmp/out" 2>&1
unset CAPRI_DESKTOP

# --uninstall: the app-menu and autostart entries, her GNOME shortcut (his stays), her Hyprland block.
# First with gsettings keeping nothing: her shortcut is still there, so its line stays in the receipt.
GS_LOST=1
uninstall >"$tmp/out" 2>&1
GS_LOST=0
check "$([ ! -e "$app" ] && [ ! -e "$CONF/autostart/synaps-avatar.desktop" ] && echo 0)" "--uninstall: both desktop entries gone"
check "$(! grep -q avatarctl "$hc" && grep -qx 'bind = $mainMod, C, killactive,' "$hc" && echo 0)" "her Hyprland block out, his lines kept"
check "$(! grep -qE 'avatarctl|hyprland.start|synaps-avatar \(install.sh\)' "$hl" &&
    grep -qxF 'hl.bind("SUPER + C", hl.dsp.no_op())' "$hl" && echo 0)" "--uninstall: Lua summon/autostart blocks removed via receipt, his lines kept"
case $(cat "$tmp/out") in
    *"didn't take her GNOME shortcut out"*"custom-keybindings '[\"$C0\"]'"*"reset-recursively $GSCHEMA.custom-keybinding:$GKEY"*'kept in the receipt'*) r=0 ;;
    *) r=1 ;;
esac
check "$([ $r = 0 ] && [ "$(cat "$RECEIPT")" = "gkey"$'\t'"$GKEY" ] && echo 0)" \
    "gsettings kept nothing: read back, he's told, the commands by hand, her line alone left in the receipt"
check "$([ "$(gsettings get "$GSCHEMA" custom-keybindings)" = "['$C0', '$GKEY']" ] && echo 0)" "and her shortcut still there (nothing was kept)"
uninstall >"$tmp/out" 2>&1
check "$([ "$(gsettings get "$GSCHEMA" custom-keybindings)" = "['$C0']" ] && [ "$(gk "$GKEY" command)" = "''" ] && [ "$(gk "$C0" command)" = "'kgx'" ] && echo 0)" \
    "--uninstall again: her GNOME shortcut out of his list and reset, his own kept: $(gsettings get "$GSCHEMA" custom-keybindings)"
check "$([ ! -e "$RECEIPT" ] && echo 0)" "and the receipt"

# His own entry where her app-menu entry was, and an --uninstall --purge with no gsettings: his entry
# stays; her GNOME shortcut's line stays in the receipt (for an --uninstall where gsettings runs), and
# with it the folders this made that are still there (his entry is in one) and where she went (its
# prefixes, which the next one reads), in the receipt's order.
rm -rf "$DATA/applications" "$CONF/autostart"
pf1=$'prefix\t'"$tmp/pfx 1" pf2=$'prefix\t'"$tmp/pfx 2"
printf '%s\n' "$pf1" "$pf2" >"$RECEIPT"
out=$(AUTOSTART=1 DESKTOP=other step_autostart 2>&1)
rec gkey "$GKEY"
printf '%s\n' '[Desktop Entry]' Type=Application 'Name=His own' Exec=true >"$app"
(
    # shellcheck disable=SC2317,SC2329 # uninstall's have
    have() { [ "$1" != gsettings ] && command -v "$1" >/dev/null 2>&1; }
    PURGE=1 uninstall
) >"$tmp/out" 2>&1
case $(cat "$tmp/out") in *"applications/synaps-avatar.desktop isn't the one this wrote any more: left as it is"*) r=0 ;; *) r=1 ;; esac
check "$([ $r = 0 ] && grep -qx 'Name=His own' "$app" && [ ! -e "$CONF/autostart/synaps-avatar.desktop" ] && echo 0)" \
    "--uninstall: an entry that isn't hers any more stays, hers goes"
case $(cat "$tmp/out") in *"no gsettings here: the GNOME shortcut $GKEY is left as it is"*'kept in the receipt'*) r=0 ;; *) r=1 ;; esac
check "$([ $r = 0 ] && [ "$(cat "$RECEIPT")" = "$pf1"$'\n'"$pf2"$'\n'"rmdir"$'\t'"$DATA/applications"$'\n'"gkey"$'\t'"$GKEY" ] && [ ! -d "$CONF/autostart" ] && echo 0)" \
    "no gsettings: her shortcut's line kept, with the folder this made that's still there (the empty one gone) and her prefixes, in order"
check "$(grep -qx '   left in ~/.local/share/synaps-avatar, not hers: packs ' "$tmp/out" && ! grep -q 'not hers:.*install.receipt' "$tmp/out" && echo 0)" \
    "--purge: her receipt, kept, isn't called not hers"
uninstall >"$tmp/out" 2>&1
check "$([ ! -e "$RECEIPT" ] && grep -qx 'Name=His own' "$app" && echo 0)" "--uninstall again, gsettings there: done, his entry and its folder kept"

# --voice: PipeWire's pw-record and pw-play (voice_check), and the Python her voice gets (voice_python,
# and tools/capri-voice-setup's own pick). kokoro-onnx wants Python 3.10 to 3.13 and a rolling distro's
# python3 is 3.14: a stand-in python3 that says 3.14 must end in uv fetching 3.13, or in one line that
# says what to install; python3 first when it is one (the distro's own keeps its pywhispercpp). A
# python that isn't CPython isn't one. Only stand-ins run: PATH is a folder of them (and mkdir, rm); the
# real uv never.
# shellcheck disable=SC2030,SC2031 # this group's own count of failures, added to the outer one
(
    fails=0
    v=$tmp/voice
    mkdir -p "$v/tools" "$v/nopw"
    for t in mkdir rm; do ln -s "$(command -v "$t")" "$v/tools/$t"; done
    # standin DIR NAME OUTPUT: an executable NAME in DIR that prints OUTPUT, whatever it is asked.
    standin() { mkdir -p "$1"; printf '#!/bin/sh\necho %s\n' "$3" >"$1/$2"; chmod +x "$1/$2"; }
    standin "$v/pw" pw-record x
    standin "$v/pw" pw-play x
    standin "$v/halfpw" pw-record x

    r=$(PATH=$v/pw PM=apt DRY=0 voice_check 2>&1)
    rc=$?
    check "$([ "$rc" = 0 ] && [ -z "$r" ] && echo 0)" "--voice with pw-record and pw-play there: nothing said, nothing stops"
    # The package of each family (Arch's `pipewire` has neither tool: `pipewire-audio` does).
    for pm in apt:pipewire-bin pacman:pipewire-audio dnf:pipewire-utils zypper:pipewire-tools; do
        r=$(PATH=$v/halfpw PM=${pm%%:*} DRY=0 voice_check 2>&1)
        rc=$?
        case $r in *"needs PipeWire's pw-record and pw-play (missing: pw-play): install ${pm#*:}, then run this again"*) w=0 ;; *) w=1 ;; esac
        check "$([ "$rc" = 1 ] && [ $w = 0 ] && echo 0)" "--voice without pw-play on ${pm%%:*}: stops, naming ${pm#*:}"
    done
    r=$(PATH=$v/nopw PM='' DRY=0 voice_check 2>&1)
    rc=$?
    case $r in *"(missing: pw-record pw-play)"*"pipewire-bin"*"pipewire-utils"*"pipewire-audio"*"pipewire-tools"*) w=0 ;; *) w=1 ;; esac
    check "$([ "$rc" = 1 ] && [ $w = 0 ] && echo 0)" "no package manager known: every family's package named"
    r=$(PATH=$v/nopw PM=pacman DRY=1 voice_check 2>&1)
    rc=$?
    case $r in *"pipewire-audio"*"a real run stops here"*) w=0 ;; *) w=1 ;; esac
    check "$([ "$rc" = 0 ] && [ $w = 0 ] && echo 0)" "a dry run says it and goes on"
    check "$(grep -A1 '^    preflight$' "$here/install.sh" | grep -q '^    \[ "\$VOICE" = 0 \] || voice_check$' && echo 0)" "the check comes right after preflight: before anything is installed"

    # voice_python: the venv of an earlier run, then python3, then python3.13 .. python3.10; else uv's fetch.
    DATA=$v/data
    standin "$v/py314" python3 3.14
    r=$(PATH=$v/py314 voice_python 2>&1)
    case $r in *"no Python 3.10–3.13 here (you have 3.14)"*"uv fetches 3.13"*"$v/data/capri-voice/python"*) w=0 ;; *) w=1 ;; esac
    check $w "python3 3.14 and nothing else: uv fetches 3.13 (said): $r"
    standin "$v/py312" python3.12 3.12
    standin "$v/py312" python3 3.14
    r=$(PATH=$v/py312 voice_python 2>&1)
    case $r in *"her voice's Python: 3.12 ("*"/python3.12)"*) w=0 ;; *) w=1 ;; esac
    check $w "python3.12 beside a python3 of 3.14: python3.12 is it: $r"
    standin "$v/py313" python3.13 3.13
    r=$(PATH=$v/py313:$v/py312 voice_python 2>&1)
    case $r in *"her voice's Python: 3.13 ("*) w=0 ;; *) w=1 ;; esac
    check $w "python3.13 and python3.12: the newer: $r"
    standin "$v/py3" python3 3.11
    r=$(PATH=$v/py3 voice_python 2>&1)
    case $r in *"her voice's Python: 3.11 ("*"/python3)"*) w=0 ;; *) w=1 ;; esac
    check $w "only a python3 of 3.11: that one: $r"
    # Ubuntu 24.04 with deadsnakes: the distro's python3 (3.12, with its pywhispercpp) over a python3.13.
    standin "$v/py3" python3 3.12
    r=$(PATH=$v/py3:$v/py313 voice_python 2>&1)
    case $r in *"her voice's Python: 3.12 ("*"/py3/python3)"*) w=0 ;; *) w=1 ;; esac
    check $w "a python3 of 3.12 and a python3.13: the python3, not the newer: $r"
    # Not CPython (PyPy 3.10 passes a version check, and onnxruntime has no wheel for it).
    standin "$v/pypy" python3 "pypy 3.10"
    r=$(PATH=$v/pypy voice_python 2>&1)
    case $r in *"no Python 3.10–3.13 here (you have pypy 3.10)"*"uv fetches 3.13"*) w=0 ;; *) w=1 ;; esac
    check $w "a python3 that is PyPy 3.10: not one, said as it is: $r"
    # The version check ignores his PYTHON* and site-packages (-I -S), whatever python3 it asks: a stand-in
    # that answers only when it is asked that way.
    mkdir -p "$v/strict"
    printf '#!/bin/sh\ncase "$1 $2 $3" in "-I -S -c") echo 3.11 ;; esac\n' >"$v/strict/python3"
    chmod +x "$v/strict/python3"
    r=$(PATH=$v/strict voice_python 2>&1)
    case $r in *"her voice's Python: 3.11 ("*) w=0 ;; *) w=1 ;; esac
    check $w "it asks python3 with -I -S: $r"
    standin "$DATA/capri-voice/venv/bin" python 3.13
    r=$(PATH=$v/py314 voice_python 2>&1)
    case $r in *"her voice's Python: 3.13 ("*"/venv/bin/python)"*) w=0 ;; *) w=1 ;; esac
    check $w "a venv of an earlier run on 3.13 stays: $r"
    rm -rf "$DATA"

    # tools/capri-voice-setup itself. uv is a stand-in that says what it was asked and stops at `venv`
    # or `pip` (exit 42; `python install` it lets through), with the folders it was told to use.
    # setup PATH DIR [VAR=VALUE]: an environment of its own (his PATH, and the one variable, if any).
    setup() { env -i PATH="$1" HOME="$tmp/home" ${3:+"$3"} "$BASH" "$here/tools/capri-voice-setup" "$2" 2>&1; }
    uvlog=$v/uv.log
    mkuv() {
        mkdir -p "$1"
        # shellcheck disable=SC2016 # the stand-in's own $@, $1, $2
        printf '#!/bin/sh\necho "uv $*" >>%s\necho "env ${UV_PYTHON_INSTALL_DIR:-} ${UV_PYTHON_INSTALL_BIN:-} ${UV_PYTHON_PREFERENCE:-}" >>%s\ncase $1 in python) [ "$2" = install ] && exit 0 ;; esac\nexit 42\n' "$uvlog" "$uvlog" >"$1/uv"
        chmod +x "$1/uv"
    }
    r=$(setup "$v/py314:$v/tools" "$v/h1")
    rc=$?
    check "$([ "$rc" = 1 ] && [ "$r" = "capri-voice-setup: her voice needs Python 3.10–3.13; you have 3.14: install uv (Arch: pacman -S uv) or python3.13 (Arch: python313 from the AUR)" ] && [ ! -e "$v/h1" ] && echo 0)" \
        "setup with a python3 of 3.14 and no uv: one line says what to install, nothing made (exit $rc): $r"
    mkdir -p "$v/nothing"
    r=$(setup "$v/nothing" "$v/h1")
    rc=$?
    case $r in *"you have no python3: install uv (Arch: pacman -S uv) or python3.13"*) w=0 ;; *) w=1 ;; esac
    check "$([ "$rc" = 1 ] && [ $w = 0 ] && echo 0)" "and with no python at all: the same line: $r"

    mkuv "$v/uv"
    : >"$uvlog"
    r=$(setup "$v/py314:$v/uv:$v/tools" "$v/h2")
    rc=$?
    check "$([ "$rc" = 42 ] && [ "$(cat "$uvlog")" = "uv python install -q 3.13
env $v/h2/python 0 only-managed
uv venv -q --system-site-packages -p 3.13 venv
env $v/h2/python 0 only-managed" ] && echo 0)" "setup with a python3 of 3.14 and uv: uv fetches 3.13 into its own folder (no link in ~/.local/bin), then the venv is on it, whatever python-preference he has (exit $rc): $(tr '\n' ' ' <"$uvlog")"
    case $r in *"uv fetches 3.13 into $v/h2/python"*) w=0 ;; *) w=1 ;; esac
    check $w "and says so: $r"
    # His own UV_PYTHON_PREFERENCE=only-system is overridden for the fetch (it would not find the one fetched).
    : >"$uvlog"
    r=$(setup "$v/py314:$v/uv:$v/tools" "$v/h2b" UV_PYTHON_PREFERENCE=only-system)
    check "$([ "$(grep -c ' only-managed$' "$uvlog")" = 2 ] && echo 0)" "with UV_PYTHON_PREFERENCE=only-system in his environment: uv still gets only-managed"

    : >"$uvlog"
    r=$(setup "$v/py313:$v/py312:$v/py314:$v/uv:$v/tools" "$v/h3")
    check "$([ "$(head -n1 "$uvlog")" = "uv venv -q --system-site-packages -p $v/py313/python3.13 venv" ] && ! grep -q 'python install' "$uvlog" && echo 0)" \
        "setup with python3.13 and python3.12 on PATH: the venv is on python3.13, nothing fetched: $(head -n1 "$uvlog")"
    : >"$uvlog"
    r=$(setup "$v/py312:$v/uv:$v/tools" "$v/h4")
    check "$([ "$(head -n1 "$uvlog")" = "uv venv -q --system-site-packages -p $v/py312/python3.12 venv" ] && echo 0)" "only python3.12 (beside a python3 of 3.14): that one"

    # A venv of an earlier run on 3.14 never got kokoro-onnx: made again. One on 3.13 is kept.
    : >"$uvlog"
    standin "$v/h5/venv/bin" python 3.14
    r=$(setup "$v/py312:$v/uv:$v/tools" "$v/h5")
    case $r in *"the venv isn't on Python 3.10–3.13: made again"*) w=0 ;; *) w=1 ;; esac
    check "$([ $w = 0 ] && [ ! -e "$v/h5/venv" ] && [ "$(head -n1 "$uvlog")" = "uv venv -q --system-site-packages -p $v/py312/python3.12 venv" ] && echo 0)" "a venv on 3.14 from an earlier run: removed, made again on python3.12"
    : >"$uvlog"
    standin "$v/h6/venv/bin" python 3.13
    r=$(setup "$v/py314:$v/uv:$v/tools" "$v/h6")
    check "$([ -e "$v/h6/venv/bin/python" ] && [ "$(head -n1 "$uvlog")" = "uv pip install -q kokoro-onnx==0.6.1" ] && ! grep -q 'venv -q\|python install' "$uvlog" && echo 0)" \
        "a venv on 3.13 from an earlier run: kept, nothing fetched, even with a python3 of 3.14"
    # A venv whose python is gone (its base Python was removed): not a Python at all, made again.
    : >"$uvlog"
    mkdir -p "$v/h7/venv/bin"
    ln -s "$v/gone/python3.13" "$v/h7/venv/bin/python"
    r=$(setup "$v/py312:$v/uv:$v/tools" "$v/h7")
    check "$([ ! -e "$v/h7/venv" ] && [ "$(head -n1 "$uvlog")" = "uv venv -q --system-site-packages -p $v/py312/python3.12 venv" ] && echo 0)" \
        "a venv whose python is a dangling link: removed, made again on python3.12"

    # python3 first, as install.sh's voice_python: a python3 of 3.12 over a python3.13 (Ubuntu with deadsnakes).
    : >"$uvlog"
    mkdir -p "$v/both"
    ln -s "$v/py312/python3" "$v/both/python3"
    ln -s "$v/py313/python3.13" "$v/both/python3.13"
    standin "$v/py312d" python3 3.12
    r=$(setup "$v/py312d:$v/py313:$v/uv:$v/tools" "$v/h8")
    check "$([ "$(head -n1 "$uvlog")" = "uv venv -q --system-site-packages -p $v/py312d/python3 venv" ] && echo 0)" \
        "a python3 of 3.12 and a python3.13 on PATH: the venv is on python3 (the distro's: its pywhispercpp)"
    # The version is asked with -I -S here too (the stand-in only answers to that).
    : >"$uvlog"
    r=$(setup "$v/strict:$v/uv:$v/tools" "$v/h8b")
    check "$([ "$(head -n1 "$uvlog")" = "uv venv -q --system-site-packages -p $v/strict/python3 venv" ] && echo 0)" "the setup asks python3 for its version with -I -S"
    # PyPy isn't one (the line says what he has).
    r=$(setup "$v/pypy:$v/tools" "$v/h9")
    case $r in *"you have pypy 3.10: install uv"*) w=0 ;; *) w=1 ;; esac
    check "$([ $w = 0 ] && [ ! -e "$v/h9" ] && echo 0)" "a python3 that is PyPy 3.10: not used: $r"

    # The installer's own step: tools/capri-voice-setup's PATH (his own first: the installer's venv, with its
    # python3.N links, last), and $BIN/capri-voice running on the voice's venv. Stand-ins for the setup and
    # for the script; step_extras runs here for real (no systemd, no uv: the installer puts its own in a venv).
    x=$v/x
    mkdir -p "$x/lib/tools/systemd" "$x/bin" "$x/conf" "$x/venv/bin" "$x/cache"
    # the installer's own venv: its python takes `-m pip install uv` (logged)
    printf '#!/bin/sh\necho "$*" >>%s/pip.log\n' "$x" >"$x/venv/bin/python"
    chmod +x "$x/venv/bin/python"
    printf '#!/bin/sh\necho "setup: $PATH" >%s/setup.log\nmkdir -p "$1/venv/bin"\nprintf "#!/bin/sh\\necho \\"on the venv: \\$*\\"\\n" >"$1/venv/bin/python"\nchmod +x "$1/venv/bin/python"\n' "$x" >"$x/lib/tools/capri-voice-setup"
    printf '#!/usr/bin/env python3\n# the sidecar\n' >"$x/lib/tools/capri-voice"
    chmod +x "$x/lib/tools/capri-voice"
    cp "$here/tools/systemd/capri-voice.service" "$x/lib/tools/systemd/"
    extras() { # DATA: step_extras on stand-ins, its output in $x/out, VOICE_HOW after
        (
            DATA=$1 BIN=$x/bin LIB=$x/lib CONF=$x/conf VENV=$x/venv CACHE_BASE=$x/cache UNITS=$x/units
            RECEIPT=$x/receipt VOICE=1 WATCH=0 DRY=0 SYSTEMD=0 YES=1 STEP_N=0 STEPS=7
            PATH=$v/tools:/usr/bin:/bin
            # shellcheck disable=SC2317,SC2329 # step_extras' own
            have() { return 1; }   # no uv of his
            step_extras >"$x/out" 2>&1
            echo "$VOICE_HOW" >"$x/how"
        )
    }
    extras "$x/data"
    check "$([ "$(cat "$x/setup.log")" = "setup: $v/tools:/usr/bin:/bin:$x/venv/bin" ] && echo 0)" \
        "no uv of his: the setup's PATH is his own, then the installer's venv last: $(cat "$x/setup.log")"
    read -r -a uvp <<<"${M_pip_uv:-uv}"
    check "$([ "$(cat "$x/pip.log")" = "-m pip install -q --disable-pip-version-check --no-cache-dir ${uvp[*]}" ] && echo 0)" "and it put uv in that venv: $(cat "$x/pip.log")"
    check "$([ "$(head -n1 "$x/bin/capri-voice")" = "#!$x/data/capri-voice/venv/bin/python" ] && [ "$(sed -n 2p "$x/bin/capri-voice")" = "# the sidecar" ] && [ -x "$x/bin/capri-voice" ] && echo 0)" \
        "capri-voice's first line is the voice's venv python, the rest as it was, still executable"
    check "$([ "$("$x/bin/capri-voice" a b)" = "on the venv: $x/bin/capri-voice a b" ] && echo 0)" "started by hand (no systemd), it runs on the venv's python, not the system's"
    check "$([ "$(cat "$x/how")" = "capri-voice (start it: capri-voice)" ] && grep -qx "file	$x/bin/capri-voice" "$x/receipt" && echo 0)" "the summary's line is true, and the file is in the receipt (--uninstall removes it): $(cat "$x/how")"
    # A path a shebang can't hold (a space): the script is as it was, and the summary names the command that works.
    rm -rf "$x/bin/capri-voice" "$x/receipt"
    extras "$x/my data"
    check "$([ "$(head -n1 "$x/bin/capri-voice")" = "#!/usr/bin/env python3" ] && echo 0)" "a data folder with a space: the shebang is left alone (the kernel would split it)"
    check "$([ "$(cat "$x/how")" = "capri-voice (start it: '$x/my data/capri-voice/venv/bin/python' $x/bin/capri-voice)" ] && echo 0)" "and the summary says the command that runs: $(cat "$x/how")"
    # With systemd the unit runs the venv's python on the script (its shebang is not read).
    check "$(! voice_shebang_ok "$(printf 'a/%0130d' 0)" && voice_shebang_ok /home/me/.local/share/capri-voice/venv/bin/python && ! voice_shebang_ok "/home/a b/python" && echo 0)" \
        "a shebang line: no space, and short enough for older kernels"
    rm -rf "$v"
    exit "$fails"
) || fails=$((fails + $?))

# Her brain (brain): --brain and the picker, on stand-ins. A synaps that signs in as 0.12's does (its
# stdin must be a pipe: a terminal there would ask to open its app; a browser sign-in fails at once at
# its end, and once signed in waits on it till it ends) and takes a key on its stdin (kept for the
# check, never on its command line); her own --providers and --set; Ollama and LM Studio stand-ins on
# loopback, at ports of their own (SERVERS); his terminal a file (TTYIN).
(
    b=$tmp/brain
    # The real curl here (above, a stand-in that fetches nothing): to the stand-in servers only.
    unset -f curl
    mkdir -p "$b/bin" "$b/synaps-cli" "$b/ollama/v1" "$b/lmstudio/v1"
    TMP=$b BIN=$b/bin SYNAPS=$b/synaps DRY=0 YES=0 TTYOUT=/dev/null SYNAPS_NEW=1 CONF=$b/conf
    export SYNAPS_BASE_DIR=$b/synaps-cli
    cat >"$SYNAPS" <<'SYNAPS'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$SYNAPS_BASE_DIR/args"
case "$1 $2" in
    'login --provider')
        if [ -t 0 ]; then echo 'stdin is a terminal' >&2; exit 3; fi
        if [ "$3" = groq ]; then
            IFS= read -r k
            printf '%s' "$k" >"$SYNAPS_BASE_DIR/key"
        elif [ "$3" != github-copilot ]; then
            # A browser sign-in: stdin read from the start (a pasted redirect), its end before the
            # callback a failure. FAKE_NEVER: he never signs in. Once signed in, it waits on that
            # read till stdin ends (10 s at most here: a pipe never closed shows as slow, not hung).
            sleep 0.3
            if read -r -t 0 && ! read -r _; then echo 'Error: Manual input channel closed' >&2; exit 1; fi
            if [ -n "${FAKE_NEVER:-}" ]; then exec cat >/dev/null; fi
            echo '{"signed": true}' >"$SYNAPS_BASE_DIR/auth.json"
            echo '✓ Login successful' >&2
            if timeout 10 cat >/dev/null; then echo closed >"$SYNAPS_BASE_DIR/stdin"; else echo held >"$SYNAPS_BASE_DIR/stdin"; fi
            exit 0
        fi
        echo '{"signed": true}' >"$SYNAPS_BASE_DIR/auth.json" ;;
    'daemon status') exit 0 ;;
    'daemon reload') echo reloaded >>"$SYNAPS_BASE_DIR/args" ;;
esac
SYNAPS
    cat >"$BIN/synaps-avatar" <<'HER'
#!/usr/bin/env bash
case $1 in
    # Her table (src/provider.rs ALL), not in the order main had it: the picker's is hers; a hint
    # (its last field) on ChatGPT's line, none on the rest (1.1.0's lines had no such field).
    # HER_OLD: a build before --providers.
    --providers)
        [ -z "${HER_OLD:-}" ] || exit 1
        printf 'openai-codex\topenai-codex\taccount\topenai-codex/gpt-5.6-terra\tChatGPT\tbrowser\tfree account works\n'
        printf 'anthropic\tclaude\taccount\tclaude-sonnet-4-6\tClaude\tbrowser\t\n'
        printf 'github-copilot\tgithub-copilot\taccount\tgithub-copilot/claude-sonnet-4.6\tCopilot\tcode\n'
        printf 'google-gemini\tgoogle-gemini\taccount\tgoogle-gemini/gemini-2.5-pro\tGemini\tbrowser\n'
        printf 'local\tlocal\tlocal\t\tLocal\tnone\n'
        printf 'groq\tgroq\tkey\tgroq/llama-3.3-70b-versatile\tGroq\tkey\n'
        ;;
    --set) printf '%s\n' "$2" >>"$SYNAPS_BASE_DIR/her" ;;
esac
HER
    chmod +x "$SYNAPS" "$BIN/synaps-avatar"
    printf '{"object":"list","data":[{"id":"qwen3:8b","object":"model"},{"id":"nomic-embed-text:latest","object":"model"}]}' >"$b/ollama/v1/models"
    printf '{"data": [{"id": "qwen/qwen3-8b", "object": "model"}]}' >"$b/lmstudio/v1/models"
    # A stand-in server for folder $1, on a free loopback port: its port, once it listens.
    serve() {
        python3 -u -m http.server --bind 127.0.0.1 --directory "$1" 0 >"$1.log" 2>&1 &
        echo $! >>"$b/servers"
        for _ in $(seq 100); do
            port=$(sed -n 's/.*port \([0-9]*\).*/\1/p' "$1.log" | head -1)
            if [ -n "$port" ]; then
                printf '%s' "$port"
                return
            fi
            sleep 0.1
        done
    }
    po=$(serve "$b/ollama")
    pl=$(serve "$b/lmstudio")
    SERVERS="Ollama|http://localhost:11434/v1|http://127.0.0.1:$po/v1/models
LM Studio|http://localhost:1234/v1|http://127.0.0.1:$pl/v1/models"
    again() { rm -f "$SYNAPS_BASE_DIR/auth.json" "$SYNAPS_BASE_DIR/args" "$SYNAPS_BASE_DIR/her" "$SYNAPS_BASE_DIR/key" "$SYNAPS_BASE_DIR/config" "$SYNAPS_BASE_DIR/stdin"; }
    me=$BASHPID
    ms() { echo $((($(date +%s%N) - t0) / 1000000)); }
    brains() { brain >"$b/out" 2>&1 </dev/null; }
    said() { tr '\n' ' ' <"$b/out"; }

    check "$([ "$(local_models)" = "Ollama|http://localhost:11434/v1|qwen3:8b"$'\n'"LM Studio|http://localhost:1234/v1|qwen/qwen3-8b" ] && echo 0)" \
        "local_models: Ollama's and LM Studio's, no embedding model ($(local_models | tr '\n' ' '))"
    rows=$(brains_here)
    check "$([ "$rows" = $'openai-codex|openai-codex|ChatGPT|browser|free account works\nanthropic|claude|Claude|browser|\ngithub-copilot|github-copilot|Copilot|code|\ngroq|groq|Groq|key|' ] && echo 0)" \
        "her brains are her table's (--providers), in its order, its hints, no local server, no Gemini without its Google client ($(tr '\n' ' ' <<<"$rows"))"
    rows=$(SYNAPS_GOOGLE_GEMINI_CLIENT_ID=x brains_here)
    check "$(case $'\n'$rows$'\n' in *$'\n''google-gemini|google-gemini|Gemini|browser|'$'\n'*) echo 0 ;; esac)" "Gemini with it"

    # No --brain, no terminal: later, nothing run.
    again
    TTYIN=$b/none BRAIN_PICK='' brains
    check "$([ ! -e "$SYNAPS_BASE_DIR/args" ] && [ ! -e "$SYNAPS_BASE_DIR/her" ] && grep -q 'her brain: later' "$b/out" && echo 0)" "no terminal and no --brain: later, nothing run"

    # --brain openai-codex: its sign-in (stdin a pipe), then her model its default (her table's).
    # Signed in, its stdin is closed and it ends: no wait on the pipe's holder (gone too).
    again
    t0=$(date +%s%N)
    TTYIN=$b/none BRAIN_PICK=openai-codex brains
    took=$(ms)
    check "$([ "$(cat "$SYNAPS_BASE_DIR/args")" = 'login --provider openai-codex' ] && [ "$(cat "$SYNAPS_BASE_DIR/her")" = 'session.model=openai-codex/gpt-5.6-terra' ] && echo 0)" \
        "--brain openai-codex: signed in, her model its default: $(said)"
    check "$([ "$(cat "$SYNAPS_BASE_DIR/stdin" 2>/dev/null)" = closed ] && [ "$took" -lt 5000 ] && ! command pgrep -P "$me" -fx 'sleep 900' >/dev/null && echo 0)" \
        "signed in: synaps' stdin closed, so it ends (${took} ms; its stdin $(cat "$SYNAPS_BASE_DIR/stdin" 2>/dev/null || echo '?'); the holder $(command pgrep -P "$me" -fx 'sleep 900' >/dev/null && echo 'still there' || echo gone))"
    # --brain claude: by its login's name too.
    again
    TTYIN=$b/none BRAIN_PICK=claude brains
    check "$([ "$(cat "$SYNAPS_BASE_DIR/args")" = 'login --provider claude' ] && [ "$(cat "$SYNAPS_BASE_DIR/her")" = 'session.model=claude-sonnet-4-6' ] && echo 0)" "--brain claude: Claude's login, its default"

    # A key: off his terminal, unshown, trimmed, on synaps' stdin; never in its arguments or on screen.
    again
    printf '  gsk_test123secret  \n' >"$b/tty"
    TTYIN=$b/tty BRAIN_PICK=groq brains
    check "$([ "$(cat "$SYNAPS_BASE_DIR/key")" = 'gsk_test123secret' ] && [ "$(cat "$SYNAPS_BASE_DIR/args")" = 'login --provider groq' ] &&
        [ "$(cat "$SYNAPS_BASE_DIR/her")" = 'session.model=groq/llama-3.3-70b-versatile' ] && ! grep -q 'gsk_' "$b/out" && echo 0)" \
        "--brain groq: the key on synaps' stdin, never in its arguments or on screen; her model Groq's default: $(said)"
    # No terminal to ask on: later, nothing run.
    again
    TTYIN=$b/none BRAIN_PICK=groq brains
    check "$([ ! -e "$SYNAPS_BASE_DIR/args" ] && grep -q 'no terminal to ask on' "$b/out" && echo 0)" "--brain groq with no terminal: not asked, said"

    # A model on this machine: Ollama's (Synaps' default server: its config untouched, no reload).
    again
    TTYIN=$b/none BRAIN_PICK=local:qwen3:8b brains
    check "$([ "$(cat "$SYNAPS_BASE_DIR/her")" = 'session.model=local/qwen3:8b' ] && [ ! -e "$SYNAPS_BASE_DIR/config" ] && [ ! -e "$SYNAPS_BASE_DIR/args" ] && echo 0)" \
        "--brain local:qwen3:8b: her model local/qwen3:8b, Synaps' config untouched (Ollama's is its default): $(said)"
    # LM Studio's: provider.local.url, his other lines kept, an old one replaced; the daemon reloaded.
    again
    printf '# his\nmodel = claude-opus-5-5\nprovider.local.url = http://old:9/v1\n' >"$SYNAPS_BASE_DIR/config"
    TTYIN=$b/none BRAIN_PICK=local:qwen/qwen3-8b brains
    check "$([ "$(cat "$SYNAPS_BASE_DIR/config")" = $'# his\nmodel = claude-opus-5-5\nprovider.local.url = http://localhost:1234/v1' ] &&
        [ "$(cat "$SYNAPS_BASE_DIR/her")" = 'session.model=local/qwen/qwen3-8b' ] && grep -qx reloaded "$SYNAPS_BASE_DIR/args" && echo 0)" \
        "--brain local:qwen/qwen3-8b (LM Studio): provider.local.url replaced, his lines kept, the daemon reloaded: $(said)"
    # One that isn't running: later, said. A name she doesn't know: later, said.
    again
    TTYIN=$b/none BRAIN_PICK=local:llama9 brains
    check "$([ ! -e "$SYNAPS_BASE_DIR/her" ] && grep -q 'no model llama9 running' "$b/out" && echo 0)" "--brain local:MODEL not running: later, said"
    again
    TTYIN=$b/none BRAIN_PICK=nope brains
    check "$([ ! -e "$SYNAPS_BASE_DIR/args" ] && grep -q "isn't one she knows" "$b/out" && echo 0)" "--brain with a name she doesn't know: later, said"

    # The picker, on his terminal: 2 (a model here), then 2 (LM Studio's).
    again
    printf '2\n2\n' >"$b/tty"
    TTYIN=$b/tty BRAIN_PICK='' brains
    check "$([ "$(cat "$SYNAPS_BASE_DIR/her")" = 'session.model=local/qwen/qwen3-8b' ] && echo 0)" "the picker: a model on this machine, the second: $(said)"
    # 1 (an account), 3 (the third in her table: ChatGPT, Claude, Copilot).
    again
    printf '1\n3\n' >"$b/tty"
    TTYIN=$b/tty BRAIN_PICK='' brains
    check "$([ "$(cat "$SYNAPS_BASE_DIR/args")" = 'login --provider github-copilot' ] && echo 0)" "the picker: an account, Copilot: $(said)"
    # Its accounts as he sees them (his terminal a pipe here, stderr: the picker's number is read
    # off its stdout): ChatGPT's hint, her table's, beside it and no other's; Copilot's code. Then
    # Enter: later.
    again
    printf '1\n\n' >"$b/tty"
    TTYOUT=/dev/stderr TTYIN=$b/tty BRAIN_PICK='' brain </dev/null 2>&1 | cat >"$b/shown"
    accts=$(LC_ALL=C grep -oE '[0-9]  [A-Z].*' "$b/shown")
    check "$([ "$accts" = $'1  ChatGPT  (free account works)\n2  Claude\n3  Copilot  (your browser, and a code to type there)' ] && [ ! -e "$SYNAPS_BASE_DIR/args" ] && echo 0)" \
        "the picker's accounts: ChatGPT's hint beside it, no other's ($(tr '\n' '/' <<<"$accts"))"
    # Enter (or the end of his input): later.
    again
    printf '\n' >"$b/tty"
    TTYIN=$b/tty BRAIN_PICK='' brains
    check "$([ ! -e "$SYNAPS_BASE_DIR/args" ] && [ ! -e "$SYNAPS_BASE_DIR/her" ] && grep -q 'her brain: later' "$b/out" && echo 0)" "the picker: Enter is later"
    # A build of her before --providers: an account can't be looked up; later, said, nothing run.
    again
    HER_OLD=1 TTYIN=$b/none BRAIN_PICK=claude brains
    check "$([ ! -e "$SYNAPS_BASE_DIR/args" ] && [ ! -e "$SYNAPS_BASE_DIR/her" ] && grep -q "can't list her brains" "$b/out" && echo 0)" "no table from her: later, said: $(said)"
    # Ctrl-C while he signs in (he never does): synaps and its stdin's holder go with install.sh (its
    # EXIT trap, on_exit), though a background job here ignores SIGINT. As a terminal sends it: to
    # its process group (a session of its own here, SIGINT not ignored).
    again
    mkdir -p "$b/int"
    cat >"$b/interrupted.sh" <<'INT'
source <(sed '$d' "$1")
set +e
ui
TMP=$2 BIN=$3 SYNAPS=$4 CONF=$5 DRY=0 YES=0 TTYIN=/dev/null TTYOUT=/dev/null SYNAPS_NEW=1 BRAIN_PICK=claude SERVERS=''
trap on_exit EXIT
brain
INT
    FAKE_NEVER=1 python3 -c 'import os, signal, sys; os.setsid(); signal.signal(signal.SIGINT, signal.SIG_DFL); os.execvp(sys.argv[1], sys.argv[1:])' \
        bash "$b/interrupted.sh" "$here/install.sh" "$b/int" "$BIN" "$SYNAPS" "$b/conf" >"$b/int.out" 2>&1 &
    ipid=$!
    for _ in $(seq 50); do if [ -s "$SYNAPS_BASE_DIR/args" ]; then break; fi; sleep 0.1; done
    sleep 0.5
    before=$(command pgrep -g "$ipid" -fx 'sleep 900|cat' | wc -l)
    kill -INT -- -"$ipid" 2>/dev/null
    wait "$ipid"
    irc=$?
    sleep 0.3
    left=$(command pgrep -g "$ipid" -a | tr '\n' ' ')
    check "$([ "$before" = 2 ] && [ -z "$left" ] && [ ! -e "$b/int" ] && echo 0)" \
        "Ctrl-C while signing in: synaps and its stdin's holder stopped, her scratch gone (exit $irc; before: $before of them; left: ${left:-none})"
    command pkill -g "$ipid" 2>/dev/null
    # A login already: nothing asked.
    again
    echo '{}' >"$SYNAPS_BASE_DIR/auth.json"
    printf '1\n1\n' >"$b/tty"
    TTYIN=$b/tty BRAIN_PICK='' brains
    check "$([ ! -e "$SYNAPS_BASE_DIR/args" ] && echo 0)" "a login already: nothing asked"
    # --dry-run: through run (said, not run), nothing else.
    again
    RAN=() RUN_REAL=0
    DRY=1 TTYIN=$b/none BRAIN_PICK=openai-codex brains
    check "$([ ! -e "$SYNAPS_BASE_DIR/args" ] && [ ! -e "$SYNAPS_BASE_DIR/her" ] && [ "${RAN[0]}" = "$SYNAPS login --provider openai-codex" ] &&
        [ "${RAN[1]}" = "$BIN/synaps-avatar --set session.model=openai-codex/gpt-5.6-terra" ] && echo 0)" "--dry-run: the sign-in and her model through run, not run: $([ -e "$SYNAPS_BASE_DIR/args" ] && echo "(synaps ran: $(cat "$SYNAPS_BASE_DIR/args")) ")$([ -e "$SYNAPS_BASE_DIR/her" ] && echo "(her set: $(cat "$SYNAPS_BASE_DIR/her")) ")${RAN[*]}"
    xargs kill <"$b/servers" 2>/dev/null
    exit "$fails"
) || fails=$((fails + $?))

# Her pack (step_pack), on stand-ins: a venv python that is make-pack (FAIL=upscale|mochi|trace|all
# fails that run as make-pack does; pip fails when its arguments hold PIP_FAIL), her binary's --check
# (CHECK_FAIL=mochi: a pack with mochi's art doesn't pass), a cached CapriCore. The full pack is the
# default; the x2 strips and mochi's art never fail the install; a pack built before mochi's art was
# the default is built again with it; a rerun keeps what was asked; --uninstall takes the half-made
# ones beside it too.
(
    p=$tmp/pack-home
    HOME=$p DATA=$p/.local/share CACHE_BASE=$p/.cache CACHE_ROOT=$p/.cache/synaps-avatar CACHE=$p/.cache/synaps-avatar/installer
    PACK=$DATA/synaps-avatar/packs/capri RECEIPT=$DATA/synaps-avatar/install.receipt VENV=$CACHE/venv BIN=$p/bin LIB=$p/lib
    art='numpy==2.2.6;python_version<"3.12" numpy==2.5.5;python_version>="3.12" Pillow==12.3.0'
    M_pip_pack=fonttools==4.65.0 M_pip_local_art=$art M_pip_upscale='torch==2.15.0 scipy' M_pip_upscale_index=https://download.pytorch.org/whl/cpu
    M_capricore_repo=CapriChooms/CapriCore M_capricore_commit=bc93892c751a73fdce14355b477dcdc775e46fd9 M_pack_upscale=off
    RUN_REAL=1 LOCAL_ART='' UPSCALE=''
    # The receipt's meta lines for real here (above, a stand-in that keeps nothing).
    eval "$(sed -n '/^rec_meta() {$/,/^}$/p' "$here/install.sh")"
    mkdir -p "$VENV/bin" "$BIN" "$LIB/tools" "$LIB/packs/capri" "$CACHE/CapriCore-bc93892/docs" "$DATA/synaps-avatar"
    touch "$CACHE/CapriCore-bc93892/docs/character-assets.md" "$LIB/tools/make-pack" "$LIB/tools/upscale_art.py" "$LIB/tools/lachy-x2.sha256"
    printf '%s\n' "$M_pip_pack" >"$VENV/.capri-pip"
    printf '%s\n' "$M_pip_local_art" >"$VENV/.capri-pip-art"
    cat >"$VENV/bin/python" <<'PY'
#!/usr/bin/env bash
if [ "$1" = -m ]; then
    echo "pip $*" >>"$CALLS"
    if [ -n "${PIP_FAIL:-}" ] && [[ " $* " == *"$PIP_FAIL"* ]]; then echo "ERROR: No matching distribution found for $PIP_FAIL"; exit 1; fi
    exit 0
fi
[ "$1" != -B ] || shift
shift; echo "make-pack $*" >>"$CALLS"
out=$2; args=" $* "
case ${FAIL:-}:$args in
    all:*) echo "make-pack: $1 is not CapriCore v1.0.4 (3 file(s) differ)"; exit 1 ;;
    upscale:*' --upscale '*) echo 'make-pack: + x2 strips'; echo 'make-pack: --upscale: 2 strip(s) differ from tools/lachy-x2.sha256 (another torch or CPU?)'; exit 1 ;;
    mochi:*' --mochi '*) echo 'make-pack: --mochi auto: cannot download https://raw.githubusercontent.com/mochiiwuff/capri-desktop/c011764/LICENSE (HTTP Error 404)'; exit 1 ;;
    trace:*' --mochi '*) printf 'make-pack: Capri -> %s\nTraceback (most recent call last):\n  File "mochi_hair.py", line 80, in recolour\nKeyError: %s\n' "$out" "'intro-wave1.webp'"; exit 1 ;;
esac
mkdir -p "$out/anim/lachy"; echo '{}' >"$out/sprites.json"
echo 'Required Notice: Copyright 2026 Lachy (https://github.com/lachydotmcg)' >"$out/anim/lachy/LICENSE.md"
case $args in *' --mochi '*)
    mkdir -p "$out/anim/mochi"
    echo 'Required Notice: Copyright (c) 2026 mochiiwuff (https://github.com/mochiiwuff/capri-desktop)' >"$out/anim/mochi/LICENSE"
    printf '## Images from mochiiwuff/capri-desktop\n\nOn these images, its README, "Credits and rights":\n\n> Its rights paragraph.\n' >"$out/NOTICE.md" ;;
esac
case $args in *' --upscale '*) mkdir -p "$out/anim/lachy/x2" ;; esac
echo "make-pack: Capri -> $out"
PY
    cat >"$BIN/synaps-avatar" <<'SH'
#!/bin/sh
if [ "${CHECK_FAIL:-}" = mochi ] && [ -d "$2/anim/mochi" ]; then echo 'synaps-avatar: pack: anim/mochi/intro-wave1.webp: its sha256 is not its pin'; exit 1; fi
echo 'pack `Capri` ok: 69 frames'
SH
    chmod +x "$VENV/bin/python" "$BIN/synaps-avatar"
    export CALLS=$p/calls
    # One step_pack: its output in $p/out, its make-pack and pip runs in $p/calls, its exit code in PRC.
    pack() { : >"$CALLS"; (set -e; step_pack) >"$p/out" 2>&1; PRC=$?; }
    builds() { grep -c '^make-pack ' "$CALLS"; }
    last() { grep '^make-pack ' "$CALLS" | tail -n1; }
    said() { grep -qF -- "$1" "$p/out"; }
    forget() { sed -i '/^meta\tpack_choice\t/d; /^meta\tpack_x2_failed\t/d' "$RECEIPT"; }
    full=" --lachy auto --mochi auto"

    pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 1 ] && last | grep -q -- "$full\$" && [ -d "$PACK/anim/mochi" ] && [ ! -d "$PACK/anim/lachy/x2" ] &&
        [[ $(meta pack) == *-1-x0 ]] && echo 0)" "the default: one make-pack, lachy's and mochi's art, no x2 ($(last | grep -o -- ' --.*'); receipt: $(meta pack))"
    check "$(said "with mochi's art" && ! grep -qi 'personal use' "$p/out" && ! grep -qi 'redistribut\|artists. repo' "$p/out" &&
        said "downloaded onto your machine from the repositories that publish it (lachydotmcg/capri-desktop, mochiiwuff/capri-desktop); the art files are not in capri-syn's repository" &&
        said "mochi's art, its README (\"Credits and rights\"): Its rights paragraph." && said 'Required Notice: Copyright (c) 2026 mochiiwuff' && echo 0)" \
        "and it says where the art came from, its README's own words and each Required Notice; no personal-use warning"
    pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 0 ] && said 'up to date' && echo 0)" "again: up to date, nothing built"
    M_pip_upscale='torch==2.16.0 scipy' pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 1 ] && echo 0)" "the manifest's pip pins are in its fingerprint: another pin, built again"
    # An install before this one: its pack without mochi's art, and the fingerprint it recorded then.
    rm -rf "$PACK/anim/mochi"
    sed -i 's/^\(meta\tpack\t\).*/\1f00dcafe0badf00-0/' "$RECEIPT"
    pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 1 ] && [ -d "$PACK/anim/mochi" ] && [[ $(meta pack) == *-1-x0 ]] && echo 0)" \
        "an older install's pack (no mochi, its fingerprint): built again, the full pack ($(meta pack))"
    LOCAL_ART=0 pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 1 ] && last | grep -q ' --lachy auto$' && [ ! -d "$PACK/anim/mochi" ] && [[ $(meta pack) == *-0-x0 ]] && echo 0)" \
        "--no-local-art: without mochi's art ($(meta pack); asked: $(meta pack_choice))"
    pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 0 ] && [ ! -d "$PACK/anim/mochi" ] && said "without mochi's art, as asked last time" && echo 0)" \
        "a plain rerun keeps it out: up to date, said"
    LOCAL_ART=1 pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 1 ] && [ -d "$PACK/anim/mochi" ] && [ "$(meta pack_choice)" = 'mochi=1 x2=-' ] && echo 0)" \
        "--with-local-art puts it back ($(meta pack_choice))"
    # The x2 strips: their pip, then the full pack again with them; a failed upscale warns and keeps the one without.
    rm -f "$VENV/.capri-pip-x2"
    UPSCALE=1 pack
    check "$([ "$PRC" = 0 ] && grep -q -- '--index-url https://download.pytorch.org/whl/cpu torch==2.15.0$' "$CALLS" && ! grep -q 'pypi.org' "$CALLS" &&
        grep -qxF -- "pip -m pip install -q --disable-pip-version-check --no-cache-dir --only-binary :all: scipy $art" "$CALLS" && [ "$(builds)" = 2 ] && last | grep -q -- "$full --upscale\$" &&
        [ -d "$PACK/anim/lachy/x2" ] && [[ $(meta pack) == *-1-x1 ]] && echo 0)" \
        "--upscale: torch 2.15.0 from its CPU index alone, scipy, numpy and Pillow from PyPI (wheels only), then the full pack with the x2 strips ($(meta pack))"
    pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 0 ] && [ -d "$PACK/anim/lachy/x2" ] && said 'as asked last time' && echo 0)" "a plain rerun keeps them: up to date"
    UPSCALE=1 FAIL=upscale pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 0 ] && echo 0)" "--upscale again: up to date"
    rm -rf "$PACK"
    UPSCALE=1 FAIL=upscale pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 2 ] && [ -d "$PACK/anim/mochi" ] && [ ! -d "$PACK/anim/lachy/x2" ] && [ ! -e "$PACK.x2" ] &&
        said "the x2 upscale didn't work (make-pack: --upscale: 2 strip(s) differ" && [[ $(meta pack) == *-1-x0 ]] && [ -n "$(meta pack_x2_failed)" ] && echo 0)" \
        "a failed upscale: said, and her pack is the full one without the x2 strips; the install goes on ($(meta pack))"
    pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 0 ] && said 'the x2 upscale failed last time on these same files: her pack stays without it' &&
        ! said "with lachy's strips at x2, as asked last time" && echo 0)" \
        "a plain rerun doesn't spend those 45 minutes again on the same files: up to date, said once (not \"as asked last time\" too)"
    # Built again without trying x2 (--no-local-art here), the failure stays noted for the next.
    LOCAL_ART=0 pack
    pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 0 ] && ! grep -q -- '--upscale' "$CALLS" && said 'the x2 upscale failed last time on these same files' && echo 0)" \
        "(and a rebuild that doesn't try x2 keeps that noted)"
    rm -f "$VENV/.capri-pip-x2"
    UPSCALE=1 PIP_FAIL=torch pack
    check "$([ "$PRC" = 0 ] && ! grep -q -- '--upscale' "$CALLS" && said "pip couldn't install torch==2.15.0 scipy" && [ ! -f "$VENV/.capri-pip-x2" ] &&
        said 'her pack is built without the x2 upscale (--no-upscale stops asking for it)' && echo 0)" \
        "torch's pip failing: said, and how to stop asking; no upscale tried, her pack as it was"
    # Python 3.10 (Ubuntu 22.04's): torch 2.15 has no wheel for it, said.
    printf 'home = /usr/bin\nversion = 3.10.12\n' >"$VENV/pyvenv.cfg"
    UPSCALE=1 PIP_FAIL=torch pack
    check "$([ "$PRC" = 0 ] && said "No matching distribution found for torch; torch 2.15.0 needs Python 3.11 or newer, this venv's is 3.10): her pack is built without the x2 upscale (--no-upscale stops asking for it)" && echo 0)" \
        "torch's pip failing on Python 3.10: why, and --no-upscale"
    rm -f "$VENV/pyvenv.cfg"
    forget
    M_pack_upscale=on PIP_FAIL=torch pack
    check "$([ "$PRC" = 0 ] && grep -q 'download.pytorch.org' "$CALLS" && echo 0)" "pack_upscale=on in the manifest: the x2 strips by default"
    UPSCALE=0 M_pack_upscale=on pack
    check "$([ "$PRC" = 0 ] && ! grep -q 'pytorch' "$CALLS" && echo 0)" "and --no-upscale says no"
    # --no-local-art --upscale: the x2 pip brings numpy and Pillow (upscale_art.py's) without mochi's.
    rm -f "$VENV/.capri-pip-x2" "$VENV/.capri-pip-art"
    LOCAL_ART=0 UPSCALE=1 pack
    check "$([ "$PRC" = 0 ] && grep -qF -- "--only-binary :all: scipy $art" "$CALLS" && ! grep -q -- '--mochi' "$CALLS" &&
        last | grep -q -- ' --lachy auto --upscale$' && [ -d "$PACK/anim/lachy/x2" ] && [ ! -d "$PACK/anim/mochi" ] && [[ $(meta pack) == *-0-x1 ]] && echo 0)" \
        "--no-local-art --upscale: Pillow and numpy come with torch; lachy's strips at x2, no mochi ($(meta pack))"
    # numpy and Pillow failing: mochi's close-ups as drawn, and the next run tries again.
    forget
    UPSCALE=0 PIP_FAIL=Pillow pack
    check "$([ "$PRC" = 0 ] && said "pip couldn't install $art (ERROR: No matching distribution found for Pillow): mochi's close-ups keep her hair as drawn" &&
        last | grep -q -- "$full --mochi-hair original\$" && [ -d "$PACK/anim/mochi" ] && [[ $(meta pack) == *-1o-x0 ]] && [ ! -f "$VENV/.capri-pip-art" ] && echo 0)" \
        "numpy/Pillow's pip failing: said, mochi's art with her hair as drawn ($(meta pack))"
    UPSCALE=0 pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 1 ] && last | grep -q -- "$full\$" && [[ $(meta pack) == *-1-x0 ]] &&
        grep -qxF -- "pip -m pip install -q --disable-pip-version-check --no-cache-dir --only-binary :all: $art" "$CALLS" && echo 0)" \
        "and once they install (exact pins, wheels only), her hair is repainted ($(meta pack))"
    # Anything failing with mochi's art: said, and her pack built without it. CapriCore failing fails the install.
    rm -rf "$PACK"
    FAIL=mochi pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 2 ] && last | grep -q ' --lachy auto$' && [ -f "$PACK/sprites.json" ] && [ ! -d "$PACK/anim/mochi" ] &&
        said "her pack with mochi's art didn't work (make-pack: --mochi auto: cannot download" && [[ $(meta pack) == *-0-x0 ]] && echo 0)" \
        "mochi's art failing to download: said, and her pack built without it ($(meta pack))"
    rm -rf "$PACK"
    FAIL=mochi UPSCALE=1 pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 3 ] && last | grep -q -- ' --lachy auto --upscale$' && [ -d "$PACK/anim/lachy/x2" ] && [ ! -d "$PACK/anim/mochi" ] &&
        ! said "the x2 upscale didn't work" && [[ $(meta pack) == *-0-x1 ]] && echo 0)" \
        "mochi's art failing with --upscale: mochi's is dropped, the x2 strips kept ($(meta pack))"
    # Again (its repo still down), --upscale asked last time: that pack is the one in place already.
    touch "$PACK/kept"
    FAIL=mochi pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 1 ] && [ -e "$PACK/kept" ] && [ -d "$PACK/anim/lachy/x2" ] && [ ! -e "$PACK.new" ] && [[ $(meta pack) == *-0-x1 ]] &&
        said "her pack with mochi's art didn't work" && said 'her pack without it is in place already and passes her --check: kept' && echo 0)" \
        "mochi's art failing again, x2 asked last time: the pack in place (no mochi, x2) is kept, not built again (1 make-pack, not 3)"
    # Its files changed (a new release): built again.
    echo '#' >>"$LIB/tools/make-pack"
    FAIL=mochi pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 3 ] && [ ! -e "$PACK/kept" ] && [ -d "$PACK/anim/lachy/x2" ] && echo 0)" \
        "(one built from other files is built again)"
    rm -rf "$PACK"
    FAIL=trace UPSCALE=0 pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 2 ] && [ -f "$PACK/sprites.json" ] && [ ! -d "$PACK/anim/mochi" ] &&
        said "her pack with mochi's art didn't work (KeyError: 'intro-wave1.webp')" && echo 0)" \
        "a traceback in mochi's path: its error said, and her pack built without it"
    UPSCALE=0 pack
    check "$([ "$PRC" = 0 ] && [ -d "$PACK/anim/mochi" ] && echo 0)" "(and the next run has it again)"
    touch "$PACK/old-pack"
    CHECK_FAIL=mochi UPSCALE=0 pack
    check "$([ "$PRC" = 0 ] && [ "$(builds)" = 2 ] && [ ! -d "$PACK/anim/mochi" ] && [ ! -e "$PACK/old-pack" ] &&
        said "her pack with mochi's art didn't work (it doesn't pass her --check: synaps-avatar: pack: anim/mochi/intro-wave1.webp" && echo 0)" \
        "her --check runs on the new pack before it goes in: failing with mochi's art, the pack without it goes in"
    touch "$PACK/old-pack"
    sed -i 's/^\(meta\tpack\t\).*/\1f00dcafe0badf00-0-x0/' "$RECEIPT"
    UPSCALE=1 FAIL=all pack
    r=$PRC
    check "$([ "$r" != 0 ] && said "make-pack couldn't build her pack" && [ -f "$PACK/old-pack" ] && [ ! -e "$PACK.new" ] && [ "$(builds)" = 2 ] && echo 0)" \
        "CapriCore failing: the install stops (exit $r) after trying without mochi's art, and the pack there stays as it was"
    # --uninstall: her pack, and the half-made ones beside it (a build stopped part way, never recorded).
    mkdir -p "$PACK.new" "$PACK.x2" "$PACK.old"
    CONF=$p/.config STATE=$p/.local/state PURGE=0 PREFIXES=("$p")
    uninstall >"$p/out" 2>&1
    check "$([ ! -e "$PACK" ] && [ ! -e "$PACK.new" ] && [ ! -e "$PACK.x2" ] && [ ! -e "$PACK.old" ] && echo 0)" \
        "--uninstall: her pack goes, and $(tilde "$PACK").new, .x2 and .old with it"
    exit "$fails"
); fails=$((fails + $?))

if [ "$fails" -gt 0 ]; then
    echo "$fails install.sh unit check(s) failed"
    exit 1
fi
echo 'all install.sh unit checks passed'
