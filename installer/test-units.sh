#!/usr/bin/env bash
# install.sh's start of her, on its own (CI's installer job): which running instances it quits,
# where it won't start her, her log kept once, her working folder; the Synaps it installs (the
# manifest's pin) and the daemon's protocol; her app-menu entry, her summon key on GNOME and
# Hyprland, and --uninstall taking them away; on stand-ins. install.sh's functions are
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
# Only the stand-ins listed in FAKE, and only while alive (a reaped or zombie one has no cmdline).
pgrep() {
    local p
    for p in "${FAKE[@]}"; do [ -n "$(tr -d '\000' <"/proc/$p/cmdline" 2>/dev/null)" ] && echo "$p"; done
    return 0
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
# each away again. A home of its own under $tmp; gsettings is a stand-in (its values in GS), and
# nothing here quits or kills anything.
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
GS_WM=''
gsettings() {
    case $1 in
        writable) echo true ;;
        get)
            if [ -n "${GS["$2 $3"]:-}" ]; then
                echo "${GS["$2 $3"]}"
            elif [ "$3" = custom-keybindings ]; then echo '@as []'; else echo "''"; fi
            ;;
        # A list reads back in single quotes, as gsettings prints one.
        set) if [ "$3" = custom-keybindings ]; then GS["$2 $3"]=${4//\"/\'}; else GS["$2 $3"]=$4; fi ;;
        reset-recursively) for k in "${!GS[@]}"; do case $k in "$2 "*) unset "GS[$k]" ;; esac; done ;;
        list-recursively) if [ "$2" = org.gnome.desktop.wm.keybindings ]; then printf '%s\n' "$2 close ['<Alt>F4']" "$GS_WM"; fi ;;
    esac
}
gk() { gsettings get "$GSCHEMA.custom-keybinding:$1" "$2"; }
C0=/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/

# The app-menu entry: written with --no-autostart, shown, her pack's icon, her start command.
app=$DATA/applications/synaps-avatar.desktop
out=$(AUTOSTART=0 DESKTOP=other step_autostart 2>&1)
check "$([ -f "$app" ] && [ ! -e "$CONF/autostart/synaps-avatar.desktop" ] && echo 0)" "--no-autostart: her app-menu entry all the same, no autostart one"
check "$(grep -q NoDisplay "$app" || echo 0)" "it shows in the menu (no NoDisplay)"
check "$(grep -qx "Icon=$PACK/icon.png" "$app" && echo 0)" "her pack's icon"
check "$(grep -qx "Exec=env SA_BACKEND=x11 $BIN/synaps-avatar" "$app" && grep -qx "TryExec=$BIN/synaps-avatar" "$app" && echo 0)" "her start command (XWayland off Hyprland and i3): $(grep '^Exec=' "$app")"
check "$(grep -qxF "file	$app" "$RECEIPT" && echo 0)" "in the receipt"
mkdir -p "$CONF/synaps-avatar"
: >"$CONF/synaps-avatar/icon.png"
out=$(AUTOSTART=1 DESKTOP=other step_autostart 2>&1)
check "$(grep -qx "Icon=$CONF/synaps-avatar/icon.png" "$app" && echo 0)" "his own icon wins, as for her window"
check "$(grep -qx NoDisplay=true "$CONF/autostart/synaps-avatar.desktop" && echo 0)" "autostart: its own entry, hidden from the menu"

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
check "$([ "$(gk "$GKEY" command)" = "\"$BIN/avatarctl talk tap\"" ] && [ "$(gk "$GKEY" binding)" = '"<Super><Alt>c"' ] && echo 0)" "running avatarctl talk tap on <Super><Alt>c"
check "$(grep -qxF "gkey	$GKEY" "$RECEIPT" && echo 0)" "in the receipt"
SUMMON='' SUMMON_KEY=''
step_summon >"$tmp/out" 2>&1
case $(cat "$tmp/out") in *asked:*) r=1 ;; *'already a GNOME shortcut: <Super><Alt>c runs'*) r=0 ;; *) r=1 ;; esac
check "$([ $r = 0 ] && [ "$SUMMON_KEY" = Super+Alt+C ] && echo 0)" "run again: already there, not asked"
GS_WM="org.gnome.desktop.wm.keybindings toggle-maximized ['<Alt><Super>c']"
GS["$GSCHEMA.custom-keybinding:$GKEY command"]="'/usr/bin/true'"
step_summon >"$tmp/out" 2>&1
case $(cat "$tmp/out") in *asked:*) r=1 ;; *'Super+Alt+C is taken: org.gnome.desktop.wm.keybindings toggle-maximized'*) r=0 ;; *) r=1 ;; esac
check $r "Super+Alt+C taken (any modifier order): not asked"
GS["$GSCHEMA.custom-keybinding:$GKEY command"]="\"$BIN/avatarctl talk tap\"" GS_WM=''
unset XDG_CURRENT_DESKTOP

# Hyprland: its stock Super+C closes a window, so Super+Alt+C (Windows' key), when that's free.
export CAPRI_DESKTOP=hyprland
mkdir -p "$CONF/hypr"
hc=$CONF/hypr/hyprland.conf
hypr() {
    printf '%s\n' '$mainMod = SUPER' "$@" 'bind = $mainMod, Q, exec, kitty' >"$hc"
    SUMMON='' SUMMON_KEY=''
    step_summon >"$tmp/out" 2>&1
}
confirm() { echo "asked: $1"; return 0; }
hypr 'bind = $mainMod, C, killactive,'
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
hypr 'bind = $mainMod, C, killactive,'
unset CAPRI_DESKTOP

# --uninstall: the app-menu and autostart entries, her GNOME shortcut (his stays), her Hyprland block.
uninstall >"$tmp/out" 2>&1
check "$([ ! -e "$app" ] && [ ! -e "$CONF/autostart/synaps-avatar.desktop" ] && echo 0)" "--uninstall: both desktop entries gone"
check "$([ "$(gsettings get "$GSCHEMA" custom-keybindings)" = "['$C0']" ] && [ "$(gk "$GKEY" command)" = "''" ] && [ "$(gk "$C0" command)" = "'kgx'" ] && echo 0)" \
    "her GNOME shortcut out of his list and reset, his own kept: $(gsettings get "$GSCHEMA" custom-keybindings)"
check "$(! grep -q avatarctl "$hc" && grep -qx 'bind = $mainMod, C, killactive,' "$hc" && echo 0)" "her Hyprland block out, his lines kept"
check "$([ ! -e "$RECEIPT" ] && echo 0)" "and the receipt"

if [ "$fails" -gt 0 ]; then
    echo "$fails install.sh unit check(s) failed"
    exit 1
fi
echo 'all install.sh unit checks passed'
