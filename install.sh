#!/usr/bin/env bash
# capri-syn's installer for Linux. One script: Synaps if it's missing, her binary, her art pack, the
# Synaps daemon, the summon key, autostart, and her voice or capri-watch when asked. Every step is
# idempotent: run it again to upgrade. Windows: install.ps1, the same steps and flags.
#
#   curl -fsSL https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/main/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/main/install.sh | bash -s -- --yes
#   ./install.sh --dry-run          # from a checkout (builds that checkout when there's no release)
#
# The steps, the flags and what each one touches: docs/INSTALL.md. Pins and URLs: installer/manifest.
# sudo runs only for system packages, the command printed first, after you say yes (or --yes).
# shellcheck disable=SC2154  # the M_* variables are installer/manifest's keys (set with printf -v)
# shellcheck disable=SC2016  # single quotes around regexes and $mod are meant
set -euo pipefail

REPO_DEFAULT=HaseebKhalid1507/capri-syn
STEPS=7

usage() {
    cat <<'EOF'
usage: install.sh [--yes] [--dry-run] [--uninstall [--purge]] [--voice] [--watch] [--no-autostart]
                  [--from-source] [--prefix DIR] [--with-local-art] [--artifact FILE] [--allow-root]

  --yes             don't ask: sudo for system packages, edits to your window manager's config,
                    enabling capri-watch
  --dry-run         print every action, touch nothing
  --uninstall       remove what this script installed; your settings and her state stay
  --purge           with --uninstall: remove those too (only her own files in those folders)
  --voice           also her voice: capri-voice (a user venv, ~320 MB of models, its user unit)
  --watch           also capri-watch (its user unit; enabled only with --yes)
  --no-autostart    don't start her at login (she still starts now, at the end)
  --from-source     build her from source even when a release has a binary
  --prefix DIR      where bin/ and lib/synaps-avatar/ go (default ~/.local)
  --with-local-art  also mochi's art: LOCAL PERSONAL USE ONLY, never publish or share it
  --artifact FILE   install this release asset (the Linux tarball) instead of downloading one
  --allow-root      install her for root (as root, with root's own HOME); otherwise root is refused

With no flags, $CAPRI_INSTALL_FLAGS is read instead (space-separated, said in the banner); this
script never passes it on. Prefer `bash -s -- FLAGS`: an exported one would go on applying.
Steps and details: docs/INSTALL.md (https://github.com/HaseebKhalid1507/capri-syn)
EOF
}

# ---- ui ---------------------------------------------------------------------------------------

ui() {
    if [ -t 1 ] && [ -z "${NO_COLOR:-}" ] && [ "${TERM:-dumb}" != dumb ]; then
        case ${COLORTERM:-} in
            truecolor | 24bit) R=$'\e[38;2;255;20;56m' D=$'\e[38;2;168;160;155m' T=$'\e[38;2;244;240;234m' ;;
            *) R=$'\e[38;5;197m' D=$'\e[38;5;248m' T=$'\e[38;5;255m' ;;
        esac
        B=$'\e[1m' N=$'\e[0m'
    else
        R='' D='' T='' B='' N=''
    fi
}
info() { printf '   %s%s%s\n' "$D" "$*" "$N"; }
good() { printf '   %s✓%s %s\n' "$R" "$N" "$*"; }
warn() { printf '   %s! %s%s\n' "$R$B" "$*" "$N" >&2; }
die() {
    printf '\n %s✗ %s%s\n' "$R$B" "$*" "$N" >&2
    exit 1
}
step() {
    STEP_N=$((STEP_N + 1))
    printf '\n %s▌%s%s %s/%s %s%s%s%s\n' "$R" "$N" "$D" "$STEP_N" "$STEPS" "$N" "$B$T" "$1" "$N"
}
# A command as you'd type it.
q() {
    local out='' a
    for a in "$@"; do
        case $a in
            '' | *[!A-Za-z0-9_./:=@%+,~-]*) out+=" '${a//\'/\'\\\'\'}'" ;;
            *) out+=" $a" ;;
        esac
    done
    printf '%s' "${out# }"
}
# Every action goes through here: printed, then run unless --dry-run.
run() {
    printf '   %s$ %s%s\n' "$D" "$(q "$@")" "$N"
    [ "$DRY" = 1 ] || "$@"
}
# A long, chatty command: printed, its output into LOG; on a terminal one line shows it's alive
# (time, the last line of the log); on failure the log's tail, then out.
run_logged() {
    local log=$1 pid rc t0=$SECONDS last
    shift
    printf '   %s$ %s%s\n' "$D" "$(q "$@")" "$N"
    [ "$DRY" = 1 ] && return 0
    mkdir -p "$(dirname "$log")"
    "$@" >"$log" 2>&1 &
    pid=$!
    if [ -t 1 ]; then
        while kill -0 "$pid" 2>/dev/null; do
            last=$(tail -n1 "$log" 2>/dev/null | tr -d '\r' | sed 's/^ *//' | cut -c1-60)
            printf '\r\033[K     %s%dm%02ds  %s%s' "$D" $(((SECONDS - t0) / 60)) $(((SECONDS - t0) % 60)) "$last" "$N"
            sleep 1
        done
        printf '\r\033[K'
    fi
    rc=0
    wait "$pid" || rc=$?
    if [ "$rc" != 0 ]; then
        tail -n 25 "$log" | sed 's/^/     /' >&2
        die "that failed (exit $rc); the whole log: $log"
    fi
}
# Yes with --yes; else asked on the terminal (curl | bash too); no terminal: no.
confirm() {
    if [ "$YES" = 1 ]; then return 0; fi
    if [ "$DRY" = 1 ]; then
        info "(asks first: $1)"
        return 0
    fi
    if (: </dev/tty) 2>/dev/null; then
        local a=''
        printf '   %s?%s %s [y/N] ' "$R$B" "$N" "$1" >/dev/tty
        read -r a </dev/tty || a=''
        case $a in y | Y | yes | YES) return 0 ;; esac
        return 1
    fi
    info "no terminal to ask on: skipped ($1; --yes does it)"
    return 1
}

# ---- small tools ------------------------------------------------------------------------------

have() { command -v "$1" >/dev/null 2>&1; }
fetch() { curl -fsSL --proto '=https' --tlsv1.2 --retry 3 --retry-delay 2 --connect-timeout 20 -o "$2" "$1"; }
# A download, printed as the curl it is.
get() {
    printf '   %s$ curl -fsSL -o %s %s%s\n' "$D" "$(q "$2")" "$(q "$1")" "$N"
    [ "$DRY" = 1 ] || fetch "$1" "$2" || die "download failed: $1"
}
fetch_text() { curl -fsSL --proto '=https' --tlsv1.2 --retry 2 --connect-timeout 20 "$@"; }
sha256() { if have sha256sum; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }
verify() {
    local got
    got=$(sha256 "$1")
    [ "$got" = "$2" ] || die "$(basename "$1"): sha256 $got is not the expected $2"
    good "sha256 checked: $(basename "$1")"
}
ver_ge() { [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n1)" = "$2" ]; }
tilde() { printf '%s' "${1/#$HOME/\~}"; }
# A path as he'd type it: ~ for his home (outside any quotes, so it expands), the rest quoted as needed.
# shellcheck disable=SC2088 # a ~ to print, not to expand
tq() {
    case $1 in
        "$HOME"/*) printf '~/%s' "$(q "${1#"$HOME"/}")" ;;
        *) q "$1" ;;
    esac
}
# Write a file (content on stdin): printed with --dry-run.
put() {
    local f=$1 body
    body=$(cat)
    printf '   %s> %s%s\n' "$D" "$(tilde "$f")" "$N"
    if [ "$DRY" = 1 ]; then
        printf '%s\n' "$body" | sed "s/^/       $D│$N /"
        return 0
    fi
    mkp "$(dirname "$f")" >/dev/null
    printf '%s\n' "$body" >"$f"
}

# The receipt: what this script installed, one entry per line, its fields tab-separated and the path
# last: `prefix PATH` (each --prefix she went to), `file PATH`, `dir PATH`, `rmdir PATH`, `backup PATH`,
# `block TAG FILE`, `unit NAME FILE`, `meta KEY VALUE`. --uninstall reads it backwards (and removes only paths in her folders: removable).
rec() {
    [ "$DRY" = 1 ] && return 0
    local IFS=$'\t' line
    line="$*"
    case $line in *$'\n'*) die "a path with a newline can't go in the receipt: $line" ;; esac
    mkdir -p "$(dirname "$RECEIPT")"
    grep -qxF -- "$line" "$RECEIPT" 2>/dev/null || printf '%s\n' "$line" >>"$RECEIPT"
}
rec_meta() {
    [ "$DRY" = 1 ] && return 0
    mkdir -p "$(dirname "$RECEIPT")"
    { awk -F'\t' -v k="$1" '!($1 == "meta" && $2 == k)' "$RECEIPT" 2>/dev/null || true; printf 'meta\t%s\t%s\n' "$1" "$2"; } >"$RECEIPT.tmp"
    mv "$RECEIPT.tmp" "$RECEIPT"
}
meta() { awk -F'\t' -v k="$1" '$1 == "meta" && $2 == k { sub(/^[^\t]*\t[^\t]*\t/, ""); v = $0 } END { printf "%s", v }' "$RECEIPT" 2>/dev/null || true; }
# Whether the receipt has this entry (KIND PATH).
recorded() { [ -f "$RECEIPT" ] && grep -qxF -- "$1"$'\t'"$2" "$RECEIPT"; }
# May --uninstall remove PATH? Absolute, no newline and no `..`, not / or $HOME, and in one of her
# folders (each prefix, $DATA, $CACHE_ROOT, $CONF), or onnxruntime's id folder (--voice's import check
# makes it). An rmdir (only ever an empty folder) may also be a folder on the way to one of them
# (one this script made: `--prefix /opt/x/y` made /opt/x).
removable() {
    local p=${1%/} kind=${2:-} r ort=${CACHE_BASE%/}/Microsoft/DeveloperTools/.onnxruntime
    case $p in /*) ;; *) return 1 ;; esac
    case $p in *$'\n'* | */../* | */.. | */./* | */.) return 1 ;; esac
    [ -n "$p" ] && [ "$p" != "${HOME%/}" ] || return 1
    [ "$p" = "$ort" ] && return 0
    for r in "${PREFIXES[@]}" "$DATA" "$CACHE_ROOT" "$CONF" "$ort"; do
        r=${r%/}
        [ -n "$r" ] || continue
        if [ "$r" != "$ort" ]; then
            case $p in "$r"/*) return 0 ;; esac
        fi
        if [ "$kind" = rmdir ]; then
            case $r/ in "$p"/*) return 0 ;; esac
        fi
    done
    return 1
}
# The folders this script makes whole (`dir`, removed recursively): only these, by name.
known_dir() {
    local p=${1%/} r
    for r in "${PREFIXES[@]}"; do [ "$p" != "${r%/}/lib/synaps-avatar" ] || return 0; done
    case $p in
        "$PACK" | "$CACHE" | "$CACHE_ROOT/capri-desktop" | "$DATA/capri-voice" | "${CACHE_BASE%/}/Microsoft/DeveloperTools/.onnxruntime") return 0 ;;
    esac
    return 1
}
# mkdir -p, each folder it makes under $HOME recorded: --uninstall removes it if it's empty then.
mkp() {
    local d=$1 todo=()
    while [ ! -d "$d" ] && [ "$d" != / ] && [ "$d" != "$HOME" ]; do
        todo=("$d" "${todo[@]}")
        d=$(dirname "$d")
    done
    [ ${#todo[@]} -gt 0 ] || return 0
    run mkdir -p "$1"
    for d in "${todo[@]}"; do rec rmdir "$d"; done
}
# A folder that is all ours: --uninstall removes it whole (only if this made it).
mkdir_rec() {
    if [ ! -d "$1" ]; then
        mkp "$(dirname "$1")"
        run mkdir -p "$1"
        rec dir "$1"
    fi
}
# The receipt's own folder, before the first line goes in.
receipt_init() {
    local d x todo=()
    d=$(dirname "$RECEIPT")
    if [ "$DRY" = 1 ] || [ -d "$d" ]; then return 0; fi
    x=$d
    while [ ! -d "$x" ] && [ "$x" != / ] && [ "$x" != "$HOME" ]; do
        todo=("$x" "${todo[@]}")
        x=$(dirname "$x")
    done
    mkdir -p "$d"
    for x in "${todo[@]}"; do rec rmdir "$x"; done
}

# ---- marked blocks in someone else's config file ----------------------------------------------

bmark() { printf '# %s synaps-avatar (install.sh): %s %s' "$1" "$2" "$1"; }
block_get() { sed -n "/^$(bmark '>>>' "$2")\$/,/^$(bmark '<<<' "$2")\$/p" "$1" 2>/dev/null | sed '1d;$d'; }
block_drop() { sed "/^$(bmark '>>>' "$2")\$/,/^$(bmark '<<<' "$2")\$/d" "$1"; }
# One backup per file per run, before its first change: the file as it was.
BACKED_UP=''
backup() {
    case $BACKED_UP in *"|$1|"*) return 0 ;; esac
    BACKED_UP+="|$1|"
    local b n=1
    b="$1.synaps-avatar-$(date +%Y%m%d-%H%M%S).bak"
    while [ -e "$b" ]; do
        n=$((n + 1))
        b="$1.synaps-avatar-$(date +%Y%m%d-%H%M%S)-$n.bak"
    done
    run cp -p "$1" "$b"
    if [ "$UNINSTALL" = 1 ]; then info "backup: $(tilde "$b")"; else rec backup "$b"; fi
}
# Put BODY in FILE between our markers: the old block replaced, a backup first. Written through
# (a symlinked dotfile stays a symlink). Not in a pipeline: the backup bookkeeping is this shell's.
block_put() {
    local f=$1 tag=$2 body=$3 cur
    cur=$(block_get "$f" "$tag")
    if [ "$cur" = "$body" ]; then
        good "already in $(tilde "$f")"
        return 0
    fi
    if [ -z "$(printf '%s' "$body" | tr -d '[:space:]')" ]; then
        warn "nothing to put in $(tilde "$f") ($tag): left as it is"
        return 0
    fi
    backup "$f"
    printf '   %s> %s (%s)%s\n' "$D" "$(tilde "$f")" "$tag" "$N"
    printf '%s\n' "$body" | sed "s/^/       $D│$N /"
    rec block "$tag" "$f"
    [ "$DRY" = 1 ] && return 0
    {
        block_drop "$f" "$tag"
        bmark '>>>' "$tag"
        printf '\n%s\n' "$body"
        bmark '<<<' "$tag"
        printf '\n'
    } >"$TMP/block"
    block_write "$f"
}
block_rm() {
    local f=$1 tag=$2
    [ -f "$f" ] && grep -qxF "$(bmark '>>>' "$tag")" "$f" || return 0
    if [ -z "$(block_drop "$f" "$tag" | tr -d '[:space:]')" ]; then
        warn "$(tilde "$f") would be empty without her block ($tag): left as it is (take it out by hand)"
        return 0
    fi
    backup "$f"
    printf '   %s- %s (%s)%s\n' "$D" "$(tilde "$f")" "$tag" "$N"
    [ "$DRY" = 1 ] && return 0
    block_drop "$f" "$tag" >"$TMP/block"
    block_write "$f"
}
# $TMP/block over FILE, never an empty one (a config is never written blank).
block_write() {
    [ -n "$(tr -d '[:space:]' <"$TMP/block")" ] || die "refusing to write an empty $(tilde "$1") (it is as it was; its backup is beside it)"
    cat "$TMP/block" >"$1"
}

# ---- setup ------------------------------------------------------------------------------------

args() {
    YES=0 DRY=0 UNINSTALL=0 PURGE=0 VOICE=0 WATCH=0 AUTOSTART=1 FROM_SOURCE=0 LOCAL_ART=0 ALLOW_ROOT=0
    PREFIX=$HOME/.local ARTIFACT='' PREFIX_GIVEN=0
    while [ $# -gt 0 ]; do
        case $1 in
            -y | --yes) YES=1 ;;
            -n | --dry-run) DRY=1 ;;
            --uninstall) UNINSTALL=1 ;;
            --purge) PURGE=1 ;;
            --voice) VOICE=1 ;;
            --watch) WATCH=1 ;;
            --no-autostart) AUTOSTART=0 ;;
            --from-source) FROM_SOURCE=1 ;;
            --with-local-art) LOCAL_ART=1 ;;
            --allow-root) ALLOW_ROOT=1 ;;
            --prefix) [ $# -ge 2 ] || die "--prefix needs a directory"; PREFIX=$2 PREFIX_GIVEN=1; shift ;;
            --prefix=*) PREFIX=${1#*=} PREFIX_GIVEN=1 ;;
            --artifact) [ $# -ge 2 ] || die "--artifact needs a file"; ARTIFACT=$2; shift ;;
            --artifact=*) ARTIFACT=${1#*=} ;;
            -h | --help) usage; exit 0 ;;
            *) usage >&2; die "unknown flag: $1" ;;
        esac
        shift
    done
    [ "$PURGE" = 0 ] || [ "$UNINSTALL" = 1 ] || die "--purge goes with --uninstall"
    PREFIX=${PREFIX/#\~/$HOME}
    case $PREFIX in *$'\n'*) die "--prefix: a folder name with a newline in it" ;; esac
    case $PREFIX in /*) ;; *) PREFIX=$(pwd)/$PREFIX ;; esac
    # `..` and `.` resolved as written (symlinks kept): the receipt's paths never carry them.
    if have realpath; then PREFIX=$(realpath -ms -- "$PREFIX") || die "--prefix: can't resolve $PREFIX"; fi
    case /$PREFIX/ in */../* | */./*) die "--prefix: give it without .. or . ($PREFIX)" ;; esac
    while [ "${PREFIX%/}" != "$PREFIX" ]; do PREFIX=${PREFIX%/}; done
    [ -n "$PREFIX" ] || die "--prefix /: give her a folder of her own (/opt/capri, ~/.local)"
    if [ -n "$ARTIFACT" ]; then
        [ -f "$ARTIFACT" ] || die "--artifact: no file $ARTIFACT"
        ARTIFACT=$(cd "$(dirname "$ARTIFACT")" && pwd)/$(basename "$ARTIFACT")
    fi
}

# From a checkout (install.sh beside installer/manifest and Cargo.toml) or an unpacked release
# tarball (beside bin/ and lib/synaps-avatar/); piped from curl: neither.
locate() {
    CHECKOUT='' BUNDLE=''
    local s=${BASH_SOURCE[0]:-} d
    if [ -n "$s" ] && [ -f "$s" ]; then
        d=$(cd "$(dirname "$s")" && pwd)
        if [ -f "$d/installer/manifest" ] && [ -f "$d/Cargo.toml" ]; then CHECKOUT=$d; fi
        if [ -f "$d/lib/synaps-avatar/installer/manifest" ] && [ -x "$d/bin/synaps-avatar" ]; then BUNDLE=$d; fi
    fi
}

manifest() {
    local f k v line
    REF=${CAPRI_INSTALL_REF:-main}
    if [ -n "$CHECKOUT" ]; then
        f=$CHECKOUT/installer/manifest
    elif [ -n "$BUNDLE" ]; then
        f=$BUNDLE/lib/synaps-avatar/installer/manifest
    else
        f=$TMP/manifest
        fetch "https://raw.githubusercontent.com/$REPO_DEFAULT/$REF/installer/manifest" "$f" ||
            die "can't fetch the installer's manifest from GitHub ($REF)"
    fi
    while IFS= read -r line || [ -n "$line" ]; do
        case $line in '' | '#'*) continue ;; esac
        k=${line%%=*} v=${line#*=}
        case $k in '' | *[!a-z0-9_]*) die "manifest: bad key '$k'" ;; esac
        printf -v "M_$k" '%s' "$v"
    done <"$f"
    for k in repo synaps_repo synaps_min synaps_protocol synaps_pin synaps_installer_sh_sha256 asset_linux \
        linux_glibc_min rustup_dist capricore_repo capricore_commit pip_pack; do
        v=M_$k
        [ -n "${!v:-}" ] || die "manifest: no $k"
    done
}

paths() {
    DATA=${XDG_DATA_HOME:-$HOME/.local/share}
    CONF=${XDG_CONFIG_HOME:-$HOME/.config}
    STATE=${XDG_STATE_HOME:-$HOME/.local/state}
    CACHE_BASE=${XDG_CACHE_HOME:-$HOME/.cache}
    CACHE_ROOT=$CACHE_BASE/synaps-avatar
    CACHE=$CACHE_ROOT/installer
    BIN=$PREFIX/bin
    LIB=$PREFIX/lib/synaps-avatar
    PACK=$DATA/synaps-avatar/packs/capri
    VENV=$CACHE/venv
    UNITS=$CONF/systemd/user
    RECEIPT=$DATA/synaps-avatar/install.receipt
    CARGO_BIN=${CARGO_HOME:-$HOME/.cargo}/bin
}

systemd_user() { have systemctl && systemctl --user show-environment >/dev/null 2>&1; }

banner() {
    local what=install
    [ "$UNINSTALL" = 1 ] && what=uninstall
    printf '\n %s▌capri%s %s▸ %s%s' "$R$B" "$N" "$D" "$what" "$N"
    [ "$DRY" = 1 ] && printf '  %s(dry run: nothing is touched)%s' "$R" "$N"
    printf '\n'
    if [ -n "$CHECKOUT" ]; then
        info "from the checkout $(tilde "$CHECKOUT")"
    elif [ -n "$BUNDLE" ]; then
        info "from the release in $(tilde "$BUNDLE")"
    fi
    case ${FLAGS_ENV:-} in
        used:*) info "flags from CAPRI_INSTALL_FLAGS: ${FLAGS_ENV#used:}" ;;
        ignored:*) info "CAPRI_INSTALL_FLAGS (${FLAGS_ENV#ignored:}) ignored: flags were given" ;;
    esac
}

preflight() {
    [ "$(uname -s)" = Linux ] || die "this is the Linux installer (Windows: install.ps1)"
    have curl || die "curl is needed (install it with your package manager, then run this again)"
    have tar || die "tar is needed"
    ARCH=$(uname -m)
    PM=''
    for p in apt-get pacman dnf zypper; do
        if have $p; then PM=${p%-get}; break; fi
    done
    if [ "$(id -u)" = 0 ]; then SUDO=(); else SUDO=(sudo); fi
}

# Root: never with someone else's HOME (sudo keeps it: what went there would be root's), and only
# with --allow-root (she'd be root's companion, not his).
root_check() {
    [ "$(id -u)" = 0 ] || return 0
    local rh
    rh=$(getent passwd 0 2>/dev/null | cut -d: -f6 || true)
    [ -n "$rh" ] || rh=$(awk -F: '$3 == 0 { print $6; exit }' /etc/passwd 2>/dev/null || true)
    [ -n "$rh" ] || rh=/root
    if [ "${HOME%/}" != "${rh%/}" ]; then
        die "running as root with HOME=$HOME, which is not root's home ($rh): what went there would be owned by root. Run it as that user, without sudo (it asks for sudo itself, for system packages only)"
    fi
    [ "$ALLOW_ROOT" = 1 ] ||
        die "running as root: she'd be installed for root, not for you. Run it as yourself, without sudo (it asks for sudo itself, for system packages only); --allow-root if she is really for root"
    warn "running as root (--allow-root): she is installed for root"
}

# ---- system packages --------------------------------------------------------------------------

pkg_installed() {
    case $PM in
        apt) dpkg-query -W -f='${db:Status-Abbrev}' "$1" 2>/dev/null | grep -q '^ii' ;;
        pacman) pacman -Qq "$1" >/dev/null 2>&1 ;;
        dnf | zypper) rpm -q --whatprovides "$1" >/dev/null 2>&1 ;;
    esac
}
# The packages of these manifest lists (base, build, run) that aren't installed.
pkgs_missing() {
    local list='' k v p
    for k in "$@"; do
        v=M_${k}_$PM
        list+=" ${!v:-}"
    done
    for p in $list; do
        pkg_installed "$p" || printf '%s ' "$p"
    done
}
pkgs_install() {
    local -a pk
    read -r -a pk <<<"$1"
    [ ${#pk[@]} -gt 0 ] || return 0
    if [ ${#SUDO[@]} -gt 0 ] && ! have sudo; then
        die "missing packages: ${pk[*]}. Install them as root, then run this again."
    fi
    info "missing system packages: ${pk[*]}"
    if ! confirm "install them with ${SUDO[*]:+${SUDO[*]} }$PM"; then
        case $PM in
            apt) info "$(q "${SUDO[@]}" apt-get install -y --no-install-recommends "${pk[@]}")" ;;
            pacman) info "$(q "${SUDO[@]}" pacman -S --needed "${pk[@]}")" ;;
            dnf) info "$(q "${SUDO[@]}" dnf install -y "${pk[@]}")" ;;
            zypper) info "$(q "${SUDO[@]}" zypper --non-interactive install "${pk[@]}")" ;;
        esac
        die "run that (or this script with --yes), then run this again"
    fi
    # The password prompt, if any, in the foreground; the logged commands reuse it.
    if [ ${#SUDO[@]} -gt 0 ]; then run sudo -v; fi
    case $PM in
        apt)
            run_logged "$TMP/apt.log" "${SUDO[@]}" apt-get update -q
            run_logged "$TMP/apt.log" "${SUDO[@]}" env DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends "${pk[@]}"
            ;;
        pacman) run_logged "$TMP/pacman.log" "${SUDO[@]}" pacman -S --needed --noconfirm "${pk[@]}" ;;
        dnf) run_logged "$TMP/dnf.log" "${SUDO[@]}" dnf install -y "${pk[@]}" ;;
        zypper) run_logged "$TMP/zypper.log" "${SUDO[@]}" zypper --non-interactive install "${pk[@]}" ;;
    esac
    [ "$DRY" = 1 ] || good "installed: ${pk[*]}"
}
packages() {
    if [ -z "$PM" ]; then
        local k v l="$M_base_apt"
        for k in "$@"; do v=M_${k}_apt; l+=" ${!v:-}"; done
        info "no apt, pacman, dnf or zypper here: make sure you have (Debian's names): $l"
        return 0
    fi
    local lists=(base "$@")
    pkgs_install "$(pkgs_missing "${lists[@]}")"
}

vulkan_check() {
    local d f
    for d in /usr/share/vulkan/icd.d /etc/vulkan/icd.d /usr/local/share/vulkan/icd.d "$DATA/vulkan/icd.d"; do
        for f in "$d"/*.json; do
            if [ -e "$f" ] && ! grep -q 'lvp_icd\|libvulkan_lvp' "$f" 2>/dev/null; then
                good "Vulkan driver: $(basename "$f" .json)"
                return 0
            fi
        done
    done
    local v=M_vulkan_${PM:-apt}
    warn "no Vulkan driver found. She renders through Vulkan: install your GPU's driver (${!v:-mesa-vulkan-drivers}). --check works without one."
}

# ---- 1. synaps --------------------------------------------------------------------------------

synaps_version() { "$1" --version 2>/dev/null | awk '{print $2; exit}'; }
# The shell files Synaps' installer (cargo-dist) adds its PATH line to: none when its bin folder is
# on PATH already (or its NO_MODIFY_PATH / UNMANAGED_INSTALL say so); else ~/.profile, the bash
# ones that exist, a zsh one, and fish's conf.d.
synaps_rc_edits() {
    case ":$PATH:" in *":$CARGO_BIN:"*) return 0 ;; esac
    [ -z "${SYNAPS_UNMANAGED_INSTALL:-}" ] || return 0
    [ "${SYNAPS_NO_MODIFY_PATH:-${INSTALLER_NO_MODIFY_PATH:-0}}" = 0 ] || return 0
    local f l z=.zshrc
    l=$(tilde "$HOME/.profile")
    for f in .bashrc .bash_profile .bash_login; do if [ -f "$HOME/$f" ]; then l+=", $(tilde "$HOME/$f")"; fi; done
    if [ ! -f "$HOME/.zshrc" ] && [ -f "$HOME/.zshenv" ]; then z=.zshenv; fi
    printf '%s, %s, %s' "$l" "$(tilde "$HOME/$z")" "$(tilde "$HOME/.config/fish/conf.d/synaps.env.fish")"
}

step_synaps() {
    step "Synaps"
    local cur='' v=''
    cur=$(command -v synaps 2>/dev/null || true)
    [ -z "$cur" ] && [ -x "$CARGO_BIN/synaps" ] && cur=$CARGO_BIN/synaps
    if [ -n "$cur" ]; then
        v=$(synaps_version "$cur")
        if [ -n "$v" ] && ver_ge "$v" "$M_synaps_min"; then
            SYNAPS=$cur SYNAPS_V=$v
            good "synaps $v ($(tilde "$cur"))"
            return 0
        fi
        info "synaps ${v:-?} at $(tilde "$cur") is older than $M_synaps_min: installing $M_synaps_pin"
        SYNAPS_NEW=2
    else
        info "her brain runs on Synaps, an open-source agent runtime (github.com/$M_synaps_repo)"
        info "not installed yet: installing $M_synaps_pin with its own installer (yours to keep; --uninstall says how to remove it)"
        SYNAPS_NEW=1
    fi
    # The manifest's pin and its installer's sha256, never releases/latest: her client speaks
    # synaps_protocol exactly, and a newer Synaps may speak another.
    local tag=v$M_synaps_pin sha=$M_synaps_installer_sh_sha256
    local url=https://github.com/$M_synaps_repo/releases/download/$tag/synaps-installer.sh rc
    rc=$(synaps_rc_edits)
    if [ -n "$rc" ]; then
        info "its installer puts $(tilde "$CARGO_BIN") on your PATH, with a line sourcing ~/.cargo/env (where there isn't one) in: $rc"
    else
        info "its installer edits none of your shell files"
    fi
    if ! confirm "install Synaps ${tag#v} with its installer${rc:+ (and those edits)}"; then
        info "or install it yourself, your shell files untouched: curl -fsSL $url | SYNAPS_NO_MODIFY_PATH=1 sh"
        die "she needs Synaps: say yes (or --yes), or install it as above, then run this again"
    fi
    get "$url" "$TMP/synaps-installer.sh"
    if [ "$DRY" = 1 ]; then info "(checks its sha256: $sha)"; else verify "$TMP/synaps-installer.sh" "$sha"; fi
    run sh "$TMP/synaps-installer.sh" --quiet
    SYNAPS=$CARGO_BIN/synaps SYNAPS_V=${tag#v}
    # Hers to mention removing only if there was none: an upgrade of his stays his.
    if [ "$SYNAPS_NEW" = 1 ]; then
        rec_meta synaps "$SYNAPS"
        rec_meta synaps_rc "${rc:-none}"
    fi
    if [ "$DRY" = 0 ]; then
        [ -x "$SYNAPS" ] || die "the synaps installer ran, but there's no $SYNAPS"
        SYNAPS_V=$(synaps_version "$SYNAPS")
        good "synaps $SYNAPS_V ($(tilde "$SYNAPS"))"
    fi
    # An upgrade: a daemon the old Synaps started runs it until it's stopped. Said now, so he can
    # stop it while the rest installs (step 4 starts the new one, and stops if it speaks another).
    if [ "$SYNAPS_NEW" = 2 ] && [ -x "$SYNAPS" ]; then
        case $("$SYNAPS" daemon status 2>&1 || true) in
        *"daemon: running"*) warn "a daemon is running, from the Synaps before this one: stop it now, \`$(tilde "$SYNAPS") daemon stop\`" ;;
        esac
    fi
    local first
    first=$(command -v synaps 2>/dev/null || true)
    if [ -n "$first" ] && [ "$first" != "$SYNAPS" ]; then
        warn "$(tilde "$first") comes first on your PATH: put $(tilde "$CARGO_BIN") before it, or upgrade that one"
    fi
}

# ---- 2. her binary ----------------------------------------------------------------------------

glibc() { ldd --version 2>&1 | head -n1 | grep -o '[0-9]\+\.[0-9]\+$' || true; }

# Copy an unpacked layout (installer/stage.sh's) into the prefix.
# Every file's sha256 under a folder (no diff or cmp on a minimal system).
tree_sum() { (cd "$1" 2>/dev/null && find . -type f -exec sha256sum {} + | sort) || true; }

install_tree() {
    local d=$1
    if [ "$DRY" = 0 ] && [ -x "$BIN/synaps-avatar" ] && [ -d "$LIB" ] &&
        [ "$(sha256 "$d/bin/synaps-avatar")" = "$(sha256 "$BIN/synaps-avatar")" ] &&
        [ "$(sha256 "$d/bin/avatarctl")" = "$(sha256 "$BIN/avatarctl" 2>/dev/null)" ] &&
        [ "$(tree_sum "$d/lib/synaps-avatar")" = "$(tree_sum "$LIB")" ]; then
        good "up to date: $(tilde "$BIN/synaps-avatar") and $(tilde "$LIB") are this build's"
        return 0
    fi
    mkp "$BIN"
    run install -m 755 "$d/bin/synaps-avatar" "$BIN/synaps-avatar"
    run install -m 755 "$d/bin/avatarctl" "$BIN/avatarctl"
    rec file "$BIN/synaps-avatar"
    rec file "$BIN/avatarctl"
    mkp "$(dirname "$LIB")"
    run rm -rf "$LIB.new"
    run cp -R "$d/lib/synaps-avatar" "$LIB.new"
    run rm -rf "$LIB"
    run mv "$LIB.new" "$LIB"
    rec dir "$LIB"
    if [ "$DRY" = 0 ]; then old_run_note; fi
}

# A run of hers going while her binary is replaced: what the end of the install does with it
# (start_her's rules), said now.
old_run_note() {
    local why
    her_runs
    if [ -z "$HIS" ] && [ ${#BARE[@]} = 0 ]; then return 0; fi
    if why=$(no_desktop); then
        info "she's running the old binary: $why, so she isn't restarted from here"
    elif [ -n "$HIS" ]; then
        info "she's running the old binary, as you started her: left be (quit her, then start her, for the new one)"
    else
        info "she's running the old binary: restarted at the end"
    fi
}

install_tarball() {
    local tb=$1 x=$TMP/x
    run mkdir -p "$x"
    run tar -xzf "$tb" -C "$x"
    local top=$x/${M_asset_linux%.tar.gz}
    [ "$DRY" = 1 ] || [ -x "$top/bin/synaps-avatar" ] || die "$(basename "$tb") isn't her release tarball (no bin/synaps-avatar)"
    install_tree "$top"
    [ "$DRY" = 1 ] || BIN_VER=$(cat "$top/lib/synaps-avatar/VERSION")
}

step_binary() {
    step "Her binary"
    BIN_VER=''
    # Given, or the tarball this script came in.
    if [ -n "$ARTIFACT" ] || [ -n "$BUNDLE" ]; then
        packages run
        if [ -n "$ARTIFACT" ]; then
            info "from $(tilde "$ARTIFACT")"
            install_tarball "$ARTIFACT"
        else
            install_tree "$BUNDLE"
            BIN_VER=$(cat "$BUNDLE/lib/synaps-avatar/VERSION")
        fi
        rec_meta binary "${BIN_VER:-artifact}"
        good "synaps-avatar ${BIN_VER} ($(tilde "$BIN/synaps-avatar"))"
        return 0
    fi
    # A release's tarball.
    local rel=https://github.com/$M_repo/releases/latest/download sum='' g
    g=$(glibc)
    if [ "$FROM_SOURCE" = 1 ]; then
        info "--from-source: building her"
    elif [ "$ARCH" != x86_64 ]; then
        info "no release binary for $ARCH: building her from source"
    elif [ -z "$g" ] || ! ver_ge "$g" "$M_linux_glibc_min"; then
        info "the release binary needs glibc $M_linux_glibc_min (here: ${g:-none}): building her from source"
    elif sum=$(fetch_text "$rel/$M_asset_linux.sha256" 2>/dev/null) && sum=$(printf '%s' "$sum" | grep -o '^[0-9a-f]\{64\}'); then
        if [ "$(meta tarball)" = "$sum" ] && [ -x "$BIN/synaps-avatar" ] && [ -d "$LIB" ]; then
            good "up to date: the latest release ($(cat "$LIB/VERSION" 2>/dev/null))"
            BIN_VER=$(cat "$LIB/VERSION" 2>/dev/null || true)
            packages run
            return 0
        fi
        packages run
        get "$rel/$M_asset_linux" "$TMP/$M_asset_linux"
        if [ "$DRY" = 1 ]; then info "(checks its sha256: $sum)"; else verify "$TMP/$M_asset_linux" "$sum"; fi
        install_tarball "$TMP/$M_asset_linux"
        rec_meta tarball "$sum"
        good "synaps-avatar $BIN_VER, the latest release ($(tilde "$BIN/synaps-avatar"))"
        return 0
    else
        info "no release yet: building her from source"
    fi
    build_from_source
}

build_from_source() {
    # The source: this checkout, else GitHub's tarball of $REF at its commit.
    local src target sha=''
    if [ -n "$CHECKOUT" ]; then
        src=$CHECKOUT target=$CHECKOUT/target
        info "source: the checkout $(tilde "$src")"
    else
        sha=$(fetch_text -H 'Accept: application/vnd.github.sha' "https://api.github.com/repos/$M_repo/commits/$REF" 2>/dev/null || true)
        if ! [[ $sha =~ ^[0-9a-f]{40}$ ]]; then
            sha=''
            if [[ $REF =~ ^[0-9a-f]{40}$ ]]; then sha=$REF; fi
        fi
        target=$CACHE/target
        if [ -n "$sha" ]; then
            src=$CACHE/src/capri-syn-${sha:0:12}
            info "source: $M_repo @ ${sha:0:12} ($REF)"
        else
            # Not a commit (GitHub's API didn't answer): this run's own copy, never reused.
            sha=$REF src=$TMP/src
            info "source: $M_repo @ $REF (which commit, GitHub's API didn't say: not cached)"
        fi
    fi
    packages build
    vulkan_check

    # rustup (user-local, ~/.cargo and ~/.rustup), then the toolchain rust-toolchain.toml pins.
    local rustup=$CARGO_BIN/rustup
    if [ ! -x "$rustup" ] && have rustup; then rustup=$(command -v rustup); fi
    if [ -x "$rustup" ]; then
        good "rustup ($(tilde "$rustup"))"
    else
        local t=$ARCH-unknown-linux-gnu want
        info "no rustup: installing it (user-local, --no-modify-path)"
        get "$M_rustup_dist/$t/rustup-init" "$TMP/rustup-init"
        if [ "$DRY" = 0 ]; then
            want=$(fetch_text "$M_rustup_dist/$t/rustup-init.sha256" | grep -o '^[0-9a-f]\{64\}') || die "no sha256 for rustup-init"
            verify "$TMP/rustup-init" "$want"
        fi
        run chmod +x "$TMP/rustup-init"
        run_logged "$TMP/rustup-init.log" "$TMP/rustup-init" -y -q --no-modify-path --profile minimal --default-toolchain none
        rustup=$CARGO_BIN/rustup
        rec_meta rustup "$rustup"
    fi

    if [ -z "$CHECKOUT" ] && [ ! -f "$src/Cargo.toml" ]; then
        mkdir_rec "$CACHE"
        if [ "$src" = "$TMP/src" ]; then run mkdir -p "$src"; else mkp "$src"; fi
        get "https://codeload.github.com/$M_repo/tar.gz/$sha" "$TMP/src.tar.gz"
        run tar -xzf "$TMP/src.tar.gz" -C "$src" --strip-components=1
    fi
    local cargo
    cargo=$(dirname "$rustup")/cargo
    info "in $(tilde "$src"): the toolchain rust-toolchain.toml pins, then her build"
    if [ "$DRY" = 0 ]; then
        (cd "$src" && "$rustup" show active-toolchain >/dev/null 2>&1) || (cd "$src" && run_logged "$TMP/toolchain.log" "$rustup" toolchain install)
        good "toolchain $(cd "$src" && "$rustup" show active-toolchain | cut -d' ' -f1)"
    else
        run "$rustup" toolchain install
    fi
    info "building (release; the first build takes a while, later ones only what changed)"
    local t0=$SECONDS
    local blog=$CACHE/build.log
    if [ "$DRY" = 1 ]; then
        run_logged "$blog" env CARGO_TARGET_DIR="$target" nice -n 10 "$cargo" build --release --locked --bin synaps-avatar
    else
        (cd "$src" && run_logged "$blog" env CARGO_TARGET_DIR="$target" nice -n 10 "$cargo" build --release --locked --bin synaps-avatar)
    fi
    [ "$DRY" = 1 ] || good "built in $(((SECONDS - t0) / 60)) min $(((SECONDS - t0) % 60)) s"
    local ver
    ver=$(git -C "$src" describe --tags --always --dirty 2>/dev/null || echo "${sha:0:12}")
    run bash "$src/installer/stage.sh" "$src" "$target/release/synaps-avatar" "$TMP/stage" "${ver:-source}"
    install_tree "$TMP/stage"
    BIN_VER=${ver:-source}
    rec_meta binary "$BIN_VER"
    [ "$DRY" = 1 ] || good "synaps-avatar $BIN_VER ($(tilde "$BIN/synaps-avatar"))"
}

# ---- 3. her art pack --------------------------------------------------------------------------

step_pack() {
    step "Her art pack"
    if ! have python3; then
        [ "$DRY" = 1 ] || die "python3 is needed for her pack (install it, then run this again)"
        info "(python3: from the system packages above)"
    fi
    # make-pack's dependencies in the installer's own venv.
    if [ ! -x "$VENV/bin/python" ]; then
        mkdir_rec "$CACHE"
        run python3 -m venv "$VENV"
    fi
    local -a want extra
    read -r -a want <<<"$M_pip_pack"
    if [ "$LOCAL_ART" = 1 ]; then
        read -r -a extra <<<"${M_pip_local_art:-}"
        want+=("${extra[@]}")
    fi
    if [ "$(cat "$VENV/.capri-pip" 2>/dev/null)" = "${want[*]}" ]; then
        good "make-pack's Python: ${want[*]} (venv $(tilde "$VENV"))"
    else
        run "$VENV/bin/python" -m pip install -q --disable-pip-version-check --no-cache-dir "${want[@]}"
        [ "$DRY" = 1 ] || printf '%s\n' "${want[*]}" >"$VENV/.capri-pip"
    fi

    # CapriCore at the pinned commit (make-pack checks every file it copies).
    local cc=$CACHE/CapriCore-${M_capricore_commit:0:7}
    if [ ! -f "$cc/docs/character-assets.md" ]; then
        mkdir_rec "$CACHE"
        get "https://codeload.github.com/$M_capricore_repo/tar.gz/$M_capricore_commit" "$TMP/capricore.tar.gz"
        mkp "$cc"
        run tar -xzf "$TMP/capricore.tar.gz" -C "$cc" --strip-components=1
    else
        good "CapriCore ${M_capricore_commit:0:7} (cached)"
    fi

    # Up to date: same make-pack, pins, moods, persona and flags as last time, and it still checks.
    local fp
    fp=$({ cat "$LIB/tools/make-pack" "$LIB"/tools/*.sha256 "$LIB/tools/desktop-art.json" "$LIB/packs/capri/moods.json" \
        "$LIB/packs/capri/persona.md" 2>/dev/null || true; } | sha256sum | cut -c1-16)-$LOCAL_ART
    if [ "$DRY" = 0 ] && [ "$(meta pack)" = "$fp" ] && [ -f "$PACK/sprites.json" ] &&
        "$BIN/synaps-avatar" --pack "$PACK" --check >/dev/null 2>&1; then
        good "up to date ($(tilde "$PACK"))"
        notices
        return 0
    fi
    local -a side=(--lachy auto)
    if [ "$LOCAL_ART" = 1 ]; then
        side+=(--mochi auto)
        warn "--with-local-art: mochi's art is for LOCAL PERSONAL USE ONLY: never commit, publish or share it"
    fi
    [ -d "$CACHE_ROOT/capri-desktop" ] || rec dir "$CACHE_ROOT/capri-desktop"
    mkp "$(dirname "$PACK")"
    # A pack there that this script didn't make (his own build): moved aside, and never recorded.
    if [ -d "$PACK" ] && ! recorded dir "$PACK"; then
        local aside
        aside=$PACK.pre-install-$(date +%Y%m%d-%H%M%S)
        warn "$(tilde "$PACK") is a pack this script didn't make: it goes to $(tilde "$aside") (yours: --uninstall leaves it)"
        run mv "$PACK" "$aside"
    fi
    run rm -rf "$PACK.new"
    run "$VENV/bin/python" "$LIB/tools/make-pack" "$cc" "$PACK.new" "${side[@]}"
    run rm -rf "$PACK.old"
    if [ -d "$PACK" ]; then run mv "$PACK" "$PACK.old"; fi
    run mv "$PACK.new" "$PACK"
    run rm -rf "$PACK.old"
    rec dir "$PACK"
    rec_meta pack "$fp"
    notices
    if [ "$DRY" = 0 ]; then
        local out
        out=$("$BIN/synaps-avatar" --pack "$PACK" --check 2>&1) || {
            printf '%s\n' "$out" | tail -n5 >&2
            die "her pack doesn't check: synaps-avatar --check failed"
        }
        good "$(printf '%s\n' "$out" | grep -o 'pack `[^`]*` ok: [0-9]* frames[^,]*' | head -n1) (--check)"
    else
        run "$BIN/synaps-avatar" --pack "$PACK" --check
    fi
}

notices() {
    local l=$PACK/anim/lachy/LICENSE.md line
    [ -f "$l" ] || return 0
    info "lachy's desktop art: PolyForm Noncommercial 1.0.0 (https://polyformproject.org/licenses/noncommercial/1.0.0)"
    grep '^Required Notice:' "$l" | while IFS= read -r line; do
        printf '     %s%s%s\n' "$T" "$line" "$N"
    done
}

# ---- 4. the daemon and her session --------------------------------------------------------------

ours() { [ -f "$1" ] && head -n3 "$1" | grep -q "capri-syn's install.sh wrote this"; }

daemon_unit() {
    cat <<EOF
# capri-syn's install.sh wrote this (install.sh --uninstall removes it): SynapsCLI's
# deploy/synaps-daemon.service, so Capri finds her session when she starts. One change: a daemon
# already running (one \`synaps --attach\` started) makes this a clean skip, not a failed unit.
# Start it at boot, not only at login: loginctl enable-linger "\$USER"
[Unit]
Description=synaps daemon (sessions; pinned sessions come back on start)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=%h
# The user manager's PATH is short: sessions' tools want the user's own bin dirs.
Environment=PATH=%h/.local/bin:%h/.cargo/bin:/usr/local/bin:/usr/bin:/bin
ExecCondition=/bin/sh -c '! "$SYNAPS" daemon status >/dev/null 2>&1'
ExecStart="$SYNAPS" daemon start --foreground
Restart=on-failure
RestartSec=5
RestartPreventExitStatus=3
# Ending sessions saves them: the daemon's own shutdown budget.
TimeoutStopSec=45
KillMode=mixed

[Install]
WantedBy=default.target
EOF
}

step_daemon() {
    step "Synaps daemon and her session"
    SYSTEMD=0
    systemd_user && SYSTEMD=1
    local u=$UNITS/synaps-daemon.service
    if [ "$SYSTEMD" = 1 ]; then
        if systemctl --user cat synaps-daemon.service >/dev/null 2>&1 && ! ours "$u"; then
            info "your own synaps-daemon.service: left as it is"
            if ! systemctl --user is-enabled -q synaps-daemon.service 2>/dev/null; then
                warn "it isn't enabled, so the daemon won't start at login"
                confirm "enable it (systemctl --user enable --now synaps-daemon)" &&
                    run systemctl --user enable --now synaps-daemon.service
            fi
        else
            daemon_unit | put "$u"
            rec unit synaps-daemon.service "$u"
            run systemctl --user daemon-reload
            run systemctl --user enable --now synaps-daemon.service
        fi
        DAEMON_HOW="systemd user unit, at login"
    else
        info "no systemd user manager: the daemon starts now, and with her at login"
        if [ "$DRY" = 1 ] || ! "$SYNAPS" daemon status >/dev/null 2>&1; then
            run "$SYNAPS" daemon --detach
        fi
        DAEMON_HOW="started with her"
    fi
    if [ "$DRY" = 1 ]; then
        DAEMON_STATE="(dry run)"
        return 0
    fi
    local s=''
    for _ in $(seq 1 20); do
        s=$("$SYNAPS" daemon status 2>&1) && break
        sleep 0.5
    done
    if ! printf '%s' "$s" | grep -q 'daemon: running'; then
        warn "the daemon isn't answering yet: $(printf '%s' "$s" | head -n1)"
        DAEMON_STATE="not running"
        return 0
    fi
    # Never "running" unless it's her protocol: she'd start and never attach. A daemon still up from
    # an older Synaps keeps its protocol until it's stopped (the unit's ExecCondition, or a restart
    # of it, leaves a running one be): `daemon stop` ends any (its sessions are saved).
    local p stop
    p=$(printf '%s' "$s" | grep -o 'protocol: [0-9]*' | grep -o '[0-9]*$' || true)
    stop="\`$(tilde "$SYNAPS") daemon stop\` (--force if it won't)"
    if [ -z "$p" ]; then
        die "the daemon doesn't say which protocol it speaks (an older Synaps?); her client speaks $M_synaps_protocol, exactly: stop it, $stop, then run this again"
    fi
    # This run put Synaps in (new, or an upgrade): the daemon still up is another one; installing
    # the pin again won't change it.
    if [ "$p" != "$M_synaps_protocol" ] && [ "${SYNAPS_NEW:-0}" != 0 ]; then
        die "synaps $SYNAPS_V is installed, but the daemon still running is another one (protocol $p; hers is $M_synaps_protocol): stop it, $stop, then run this again"
    fi
    if [ "$p" != "$M_synaps_protocol" ]; then
        die "the daemon speaks protocol $p; her client speaks $M_synaps_protocol (exact match): she can't attach. Install synaps $M_synaps_pin (curl -fsSL https://github.com/$M_synaps_repo/releases/download/v$M_synaps_pin/synaps-installer.sh | sh), stop the daemon, $stop, and run this again; or use a capri-syn that speaks $p"
    fi
    DAEMON_STATE="running (protocol $p)"
    good "daemon $DAEMON_STATE: $DAEMON_HOW"
    info "her session: \`capri\`, attached if live, else resumed, else made when she starts"
}

# ---- 5. summon key ----------------------------------------------------------------------------

first_file() {
    local f
    for f in "$@"; do [ -f "$f" ] && { printf '%s' "$f"; return 0; }; done
    return 0
}

desktop() {
    local de
    DESKTOP=${CAPRI_DESKTOP:-}
    if [ -z "$DESKTOP" ]; then
        de=$(printf '%s:%s' "${XDG_CURRENT_DESKTOP:-}" "${XDG_SESSION_DESKTOP:-}" | tr '[:upper:]' '[:lower:]')
        if [ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ] || [[ $de == *hyprland* ]]; then
            DESKTOP=hyprland
        elif [ -n "${SWAYSOCK:-}" ] || [[ $de == *sway* ]]; then
            DESKTOP=sway
        elif [ -n "${I3SOCK:-}" ] || [[ $de == *i3* ]] || pgrep -u "$(id -u)" -x i3 >/dev/null 2>&1; then
            DESKTOP=i3
        else
            DESKTOP=other
        fi
    fi
    case $DESKTOP in
        i3) WMCONF=$(first_file "$CONF/i3/config" "$HOME/.i3/config") ;;
        sway) WMCONF=$(first_file "$CONF/sway/config" "$HOME/.sway/config") ;;
        hyprland) WMCONF=$(first_file "$CONF/hypr/hyprland.conf") ;;
        *) WMCONF='' ;;
    esac
}

# Lines in WMCONF, outside our blocks, that match a regex.
wm_grep() { [ -n "$WMCONF" ] && block_drop "$WMCONF" summon | block_drop /dev/stdin autostart | grep -Ei -- "$1"; }

# The summon key's modifier: MOD_WORD as the lines write it ($mod when the config sets one),
# MOD_RAW its value, MOD_NAME what he presses (Super, Alt...).
wm_mod() {
    local v=''
    MOD_WORD=Mod4 MOD_RAW=Mod4 MOD_NAME=Super
    case $DESKTOP in i3 | sway) ;; *) return 0 ;; esac
    v=$(grep -E '^\s*set\s+\$mod\s' "$WMCONF" 2>/dev/null | tail -n1 | awk '{print $3}' || true)
    [ -n "$v" ] || return 0
    MOD_WORD='$mod' MOD_RAW=$v
    case $(printf '%s' "$v" | tr '[:upper:]' '[:lower:]') in
        mod4 | super) MOD_NAME=Super ;;
        mod1 | alt) MOD_NAME=Alt ;;
        control | ctrl) MOD_NAME=Ctrl ;;
        shift) MOD_NAME=Shift ;;
        *) MOD_NAME=$v ;;
    esac
}

summon_lines() {
    local ctl
    ctl=$(q "$BIN/avatarctl")
    case $DESKTOP in
        i3 | sway)
            printf 'bindsym %s+c exec --no-startup-id %s talk press\n' "$MOD_WORD" "$ctl"
            printf 'bindsym --release %s+c exec --no-startup-id %s talk release' "$MOD_WORD" "$ctl"
            if [ "$VOICE" = 1 ]; then printf '\nbindsym %s+Ctrl+c exec --no-startup-id %s talk toggle' "$MOD_WORD" "$ctl"; fi
            ;;
        hyprland)
            printf 'bind = SUPER, C, exec, %s talk press\n' "$ctl"
            printf 'bindr = SUPER, C, exec, %s talk release' "$ctl"
            if [ "$VOICE" = 1 ]; then printf '\nbind = SUPER CTRL, C, exec, %s talk toggle' "$ctl"; fi
            ;;
    esac
    return 0
}

step_summon() {
    step "Summon key"
    desktop
    if [ "$DESKTOP" = other ]; then
        info "desktop: ${XDG_CURRENT_DESKTOP:-unknown} (not i3, sway or Hyprland): bind a key yourself in its keyboard settings to"
        info "  $(tilde "$BIN/avatarctl") talk tap        (a tap opens the Line beside her)"
        SUMMON="a key you bind to \`avatarctl talk tap\`"
        return 0
    fi
    local lines
    wm_mod
    lines=$(summon_lines)
    if [ -z "$WMCONF" ]; then
        info "$DESKTOP runs its system-wide config (no user config to add to). Add these to your own:"
        printf '%s\n' "$lines" | sed "s/^/       $D│$N /"
        SUMMON="not bound (see above)"
        return 0
    fi
    info "desktop: $DESKTOP ($(tilde "$WMCONF"))"
    if wm_grep 'talk (press|tap)' >/dev/null; then
        good "already bound in your config: $(wm_grep 'talk (press|tap)' | head -n1 | sed 's/^\s*//')"
        SUMMON="your own binding"
        return 0
    fi
    local clash
    case $DESKTOP in
        i3 | sway) clash=$(wm_grep "^\\s*bindsym\\s+(--\\S+\\s+)*(\\\$mod|$MOD_RAW)\\+c\\s" || true) ;;
        hyprland) clash=$(wm_grep '^\s*bind[a-z]*\s*=\s*(\$mainmod|\$mod|super)\s*,\s*c\s*,' || true) ;;
    esac
    if [ -n "$clash" ]; then
        warn "$MOD_NAME+C is taken: $(printf '%s' "$clash" | head -n1 | sed 's/^\s*//')"
        info "pick another key and add these to $(tilde "$WMCONF") with it:"
        printf '%s\n' "$lines" | sed "s/^/       $D│$N /"
        SUMMON="not bound ($MOD_NAME+C is taken)"
        return 0
    fi
    if confirm "add the summon key ($MOD_NAME+C) to $(tilde "$WMCONF") (a backup first)"; then
        block_put "$WMCONF" summon "$lines"
        SUMMON="$MOD_NAME+C ($DESKTOP)" SUMMON_KEY="$MOD_NAME+C"
        wm_reload
    else
        info "add these yourself:"
        printf '%s\n' "$lines" | sed "s/^/       $D│$N /"
        SUMMON="not bound (see above)"
    fi
}

wm_reload() {
    case $DESKTOP in
        i3) if [ -n "${I3SOCK:-}" ] && have i3-msg; then run i3-msg -q reload; fi ;;
        sway) if [ -n "${SWAYSOCK:-}" ] && have swaymsg; then run swaymsg -q reload; fi ;;
    esac
}

# ---- 6. autostart -----------------------------------------------------------------------------

# A path for a desktop entry's Exec: double quotes when it needs any (its spec has no single ones).
xq() {
    local a=$1
    case $a in
        *[!A-Za-z0-9_./:=@+,~-]*)
            a=${a//\"/\\\"} a=${a//\`/\\\`} a=${a//\$/\\\$}
            printf '"%s"' "$a"
            ;;
        *) printf '%s' "$a" ;;
    esac
}
# Her start command, for a WM config's exec (sh -c reads it), a desktop entry's Exec (FORM xdg),
# or as he'd type it (FORM show: ~ for his home).
launch_cmd() {
    local form=${1:-sh} cmd syn
    case $form in
        xdg) if [ "$SYSTEMD" = 1 ]; then cmd=$(xq "$BIN/synaps-avatar"); else cmd=$(q "$BIN/synaps-avatar"); fi ;;
        show) cmd=$(tq "$BIN/synaps-avatar") ;;
        *) cmd=$(q "$BIN/synaps-avatar") ;;
    esac
    # Off Hyprland, Wayland desktops run her through XWayland (a no-op on X11).
    case $DESKTOP in hyprland | i3) ;; *) cmd="env SA_BACKEND=x11 $cmd" ;; esac
    if [ "$SYSTEMD" = 0 ]; then
        if [ "$form" = show ]; then syn=$(tq "$SYNAPS"); else syn=$(q "$SYNAPS"); fi
        cmd="sh -c \"$syn daemon status >/dev/null 2>&1 || $syn daemon --detach; exec $cmd\""
    fi
    if [ "$form" = xdg ]; then cmd=${cmd//%/%%}; fi
    printf '%s' "$cmd"
}

step_autostart() {
    step "Autostart"
    if [ "$AUTOSTART" = 0 ]; then
        info "skipped (--no-autostart)"
        AUTOSTART_HOW="off"
        return 0
    fi
    local cmd
    cmd=$(launch_cmd)
    if [ -n "$WMCONF" ] && wm_grep 'synaps-avatar|capri-bin' | grep -Evq 'avatarctl|^\s*#'; then
        good "your config already starts her: $(wm_grep 'synaps-avatar|capri-bin' | grep -Ev 'avatarctl|^\s*#' | head -n1 | sed 's/^\s*//')"
        AUTOSTART_HOW="your own"
        return 0
    fi
    case $DESKTOP in
        i3 | sway | hyprland)
            if [ -z "$WMCONF" ]; then
                info "no user config for $DESKTOP: add this to yours:"
                info "  exec $cmd"
                AUTOSTART_HOW="not set (see above)"
                return 0
            fi
            local line
            case $DESKTOP in
                i3) line="exec --no-startup-id $cmd" ;;
                sway) line="exec $cmd" ;;
                hyprland) line="exec-once = $cmd" ;;
            esac
            if confirm "start her at login from $(tilde "$WMCONF") (a backup first)"; then
                info "she starts at login from your $DESKTOP config (--no-autostart to skip)"
                block_put "$WMCONF" autostart "$line"
                AUTOSTART_HOW="$DESKTOP config"
            else
                info "add it yourself: $line"
                AUTOSTART_HOW="not set (see above)"
            fi
            ;;
        *)
            local f=$CONF/autostart/synaps-avatar.desktop
            cmd=$(launch_cmd xdg)
            info "she starts at login: an XDG autostart entry (--no-autostart to skip)"
            put "$f" <<EOF
[Desktop Entry]
# capri-syn's install.sh wrote this (install.sh --uninstall removes it).
Type=Application
Name=synaps-avatar
Comment=Capri, your desktop companion
Exec=$cmd
Terminal=false
NoDisplay=true
X-GNOME-Autostart-enabled=true
EOF
            rec file "$f"
            AUTOSTART_HOW="XDG autostart ($(tilde "$f"))"
            ;;
    esac
}

# ---- 7. voice and watch -----------------------------------------------------------------------

# A path in a unit file: `%` doubled (systemd's specifiers), in double quotes when it has a space.
uq() {
    local a=${1//%/%%}
    case $a in
        *[[:space:]\"\\]*) a=${a//\\/\\\\} a=${a//\"/\\\"}; printf '"%s"' "$a" ;;
        *) printf '%s' "$a" ;;
    esac
}
# A line of a tools/systemd unit, with her real folders: %h/.local/bin is $BIN, capri-voice's
# venv and models are in $DATA/capri-voice.
unit_line() {
    local l=$1 t out='' b
    local -a words
    case $l in
        ExecStart=*)
            read -r -a words <<<"${l#ExecStart=}"
            for t in "${words[@]}"; do
                case $t in
                    %h/.local/bin/*) t=$(uq "$BIN/${t#%h/.local/bin/}") ;;
                    %h/.local/share/capri-voice/*) t=$(uq "$DATA/capri-voice/${t#%h/.local/share/capri-voice/}") ;;
                esac
                out+=" $t"
            done
            printf 'ExecStart=%s\n' "${out# }"
            ;;
        Environment=PATH=%h/.local/bin:*)
            b=${BIN//%/%%}
            case $b in
                *[[:space:]]*) printf 'Environment="PATH=%s:%s"\n' "$b" "${l#Environment=PATH=%h/.local/bin:}" ;;
                *) printf 'Environment=PATH=%s:%s\n' "$b" "${l#Environment=PATH=%h/.local/bin:}" ;;
            esac
            ;;
        '[Service]')
            printf '%s\n' "$l"
            if [ "$DATA" != "$HOME/.local/share" ] && [ "$UNIT_NAME" = capri-voice.service ]; then
                printf 'Environment=%s\n' "$(uq "CAPRI_VOICE_MODELS=$DATA/capri-voice/models")"
            fi
            ;;
        *) printf '%s\n' "$l" ;;
    esac
}

user_unit() { # SRC NAME: a tools/systemd unit with her folders, marked as ours
    local src=$1 name=$2 t
    # A dry run hasn't installed $LIB: the checkout's or the tarball's copy, else just where it goes.
    if [ ! -f "$src" ]; then
        for t in "$CHECKOUT/tools/systemd/$name" "$BUNDLE/lib/synaps-avatar/tools/systemd/$name" ''; do
            [ -n "$t" ] && [ -f "$t" ] && break
        done
        if [ -z "$t" ]; then
            [ "$DRY" = 1 ] || die "no $src"
            printf '   %s> %s%s (tools/systemd/%s)\n' "$D" "$(tilde "$UNITS/$name")" "$N" "$name"
            return 0
        fi
        src=$t
    fi
    local l
    UNIT_NAME=$name
    {
        printf "# capri-syn's install.sh wrote this (install.sh --uninstall removes it), from tools/systemd/%s.\n" "$name"
        sed -n '/^\[Unit\]/,$p' "$src" | while IFS= read -r l || [ -n "$l" ]; do unit_line "$l"; done
    } | put "$UNITS/$name"
    rec unit "$name" "$UNITS/$name"
    if [ "$SYSTEMD" = 1 ]; then run systemctl --user daemon-reload; fi
}

step_extras() {
    step "Voice and watch"
    if [ "$VOICE" = 0 ] && [ "$WATCH" = 0 ]; then
        info "skipped (--voice, --watch)"
        return 0
    fi
    if [ "$VOICE" = 1 ]; then
        local path=$PATH
        if ! have uv; then
            read -r -a uvp <<<"${M_pip_uv:-uv}"
            run "$VENV/bin/python" -m pip install -q --disable-pip-version-check --no-cache-dir "${uvp[@]}"
            path=$VENV/bin:$PATH
        fi
        [ -d "$DATA/capri-voice" ] || rec dir "$DATA/capri-voice"
        # onnxruntime's telemetry id, written by the setup's import check (outermost recorded first).
        local d
        for d in "$CACHE_BASE/Microsoft" "$CACHE_BASE/Microsoft/DeveloperTools"; do
            [ -d "$d" ] || rec rmdir "$d"
        done
        [ -d "$CACHE_BASE/Microsoft/DeveloperTools/.onnxruntime" ] || rec dir "$CACHE_BASE/Microsoft/DeveloperTools/.onnxruntime"
        # uv's downloads in a throwaway cache: nothing of the setup's outside its own folder.
        run env PATH="$path" UV_NO_CACHE=1 bash "$LIB/tools/capri-voice-setup" "$DATA/capri-voice"
        run install -m 755 "$LIB/tools/capri-voice" "$BIN/capri-voice"
        rec file "$BIN/capri-voice"
        user_unit "$LIB/tools/systemd/capri-voice.service" capri-voice.service
        if [ "$SYSTEMD" = 1 ]; then run systemctl --user enable --now capri-voice.service; fi
        local s=$CONF/synaps-avatar/settings.toml
        if [ ! -f "$s" ]; then
            printf '[voice]\nenabled = true' | put "$s"
        elif ! grep -Eq '^\s*enabled\s*=\s*true' <(sed -n '/^\[voice\]/,/^\[/p' "$s"); then
            info "turn her voice on in $(tilde "$s"):  [voice] enabled = true"
        fi
        VOICE_HOW="capri-voice ($([ "$SYSTEMD" = 1 ] && echo "user unit" || echo "start it: capri-voice"))"
    fi
    if [ "$WATCH" = 1 ]; then
        run install -m 755 "$LIB/tools/capri-watch/capri-watch" "$BIN/capri-watch"
        rec file "$BIN/capri-watch"
        user_unit "$LIB/tools/systemd/capri-watch.service" capri-watch.service
        # Its config loads (a watch.toml from before v1 is an error now): say so here, not 30 s after "enabled".
        local py=/usr/bin/python3 bad=''
        [ -x "$py" ] || py=python3
        if [ "$DRY" != 1 ] && ! bad=$("$py" "$BIN/capri-watch" --print-config 2>&1 >/dev/null); then
            bad=${bad#capri-watch: }
            [ -n "$bad" ] || bad="its config does not load"
            warn "capri-watch won't start: $bad (docs/WATCH.md)"
            WATCH_HOW="capri-watch (installed, not enabled: fix $(tilde "$HOME/.config/synaps-avatar/watch.toml"))"
        elif [ "$SYSTEMD" = 1 ] && [ "$YES" = 1 ]; then
            run systemctl --user enable --now capri-watch.service
            WATCH_HOW="capri-watch (enabled)"
        else
            info "not enabled: systemctl --user enable --now capri-watch (or run this with --yes)"
            WATCH_HOW="capri-watch (installed, not enabled)"
        fi
    fi
}

# ---- summary ----------------------------------------------------------------------------------

synaps_home() { printf '%s' "${SYNAPS_BASE_DIR:-$HOME/.synaps-cli}"; }
# A model to talk through: a login (OAuth or API key, both in auth.json) or a local model (in
# Synaps' config or her own [session]).
has_brain() {
    local h
    h=$(synaps_home)
    [ -s "$h/auth.json" ] || grep -Eq '^\s*model\s*=\s*"?local/' "$h/config" "$CONF/synaps-avatar/settings.toml" 2>/dev/null
}

# Her brain: she talks through Synaps, and Synaps needs a model. Signing in is hers to walk him
# through, not a picker here: until Synaps has one her tag reads NO BRAIN YET, and her answer to
# his first message ends on the command (a chip; a click copies it).
brain() {
    if has_brain; then
        if [ "${SYNAPS_NEW:-0}" = 0 ]; then info "she found your Synaps. her session is \`capri\`: it'll show up in /sessions."; fi
        return 0
    fi
    info "her brain: Synaps has no model yet. say hi to her: she'll tell you how to give her one."
}

# Her runs now (his user's), from their command lines (/proc/PID/cmdline): BARE, the pids started
# with no arguments (as autostart starts her: the old binary, quit and started again), and HIS, the
# command line of one started with arguments (--socket, --profile, --session...: his own run, never
# quit here).
her_runs() {
    local p n
    BARE=() HIS=''
    for p in $(pgrep -u "$(id -u)" -x synaps-avatar 2>/dev/null || true); do
        # Its arguments, NUL-terminated (a path with spaces is still one).
        n=$(tr -cd '\000' <"/proc/$p/cmdline" 2>/dev/null | wc -c) || n=0
        if [ "$n" -gt 1 ]; then
            HIS=$(tr '\000' ' ' <"/proc/$p/cmdline" 2>/dev/null) || true
            HIS=$(tilde "${HIS% }")
        else
            BARE+=("$p")
        fi
    done
}

# Why she isn't started from this terminal, if she isn't: an ssh session (with `ssh -X` she'd open
# on the ssh client's screen and die with the tunnel), or no desktop at all.
no_desktop() {
    if [ -n "${SSH_CONNECTION:-}${SSH_TTY:-}" ]; then
        printf 'an ssh session (she would open on the far screen and close with it)'
    elif [ -z "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ]; then
        printf 'no desktop in this terminal (no DISPLAY or WAYLAND_DISPLAY)'
    else
        return 1
    fi
}

# Her, now: the first launch is the install's, not the next login's (with --no-autostart too: that
# one is about login). Started the way autostart starts her (XWayland off Hyprland and i3), from his
# home folder, in a session of her own (closing this terminal leaves her be); up once her log says
# her first frame is painted. Only with a desktop here to show on. One that runs already as
# autostart starts her is the old binary: quit, then started again; one he started with his own
# arguments is left be. Her log is this run's (the last one's kept as launch.old.log). STARTED=1
# when she's up.
start_her() {
    local cmd log pid i why
    STARTED=0
    cmd=$(launch_cmd)
    log=$STATE/synaps-avatar/launch.log
    if [ "$DRY" = 1 ]; then
        printf '   %s$ %s &%s\n' "$D" "$cmd" "$N"
        return 0
    fi
    her_runs
    if why=$(no_desktop); then
        if [ -n "$HIS" ] || [ ${#BARE[@]} -gt 0 ]; then
            info "$why: she's still running the old binary (\`avatarctl quit\`, then start her from your desktop)"
        elif [ "$AUTOSTART" = 1 ]; then
            info "$why: not started here. she starts at your next login"
        else
            info "$why: not started here. start her from your desktop"
        fi
        return 0
    fi
    if [ -n "$HIS" ]; then
        info "she's running as you started her ($HIS): left be. to run the new one, \`avatarctl quit\`, then start her"
        return 0
    fi
    if [ ${#BARE[@]} -gt 0 ]; then
        # Its pids read again just before a kill: the quit may have taken some of them already.
        if ! run "$BIN/avatarctl" quit; then
            her_runs
            if [ ${#BARE[@]} -gt 0 ]; then run kill "${BARE[@]}" || true; fi
        fi
        for i in $(seq 50); do
            her_runs
            [ ${#BARE[@]} -gt 0 ] || [ -n "$HIS" ] || break
            sleep 0.1
        done
        if [ ${#BARE[@]} -gt 0 ] || [ -n "$HIS" ]; then
            warn "she didn't quit: \`avatarctl quit\`, then start her with the line below"
            return 0
        fi
    fi
    mkdir -p "$(dirname "$log")"
    if [ -f "$log" ]; then mv -f "$log" "${log%.log}.old.log"; fi
    printf '   %s$ %s &%s\n' "$D" "$cmd" "$N"
    if have setsid; then
        setsid sh -c "cd && exec $cmd" </dev/null >"$log" 2>&1 &
    else
        nohup sh -c "cd && exec $cmd" </dev/null >"$log" 2>&1 &
    fi
    pid=$!
    # Up to 20 s (a cold start reads her pack); gone for good (her pid and every synaps-avatar) after 2 s.
    for i in $(seq 100); do
        if grep -q 'first-frame: painted' "$log" 2>/dev/null; then
            STARTED=1
            break
        fi
        if [ "$i" -gt 10 ] && ! kill -0 "$pid" 2>/dev/null && ! pgrep -u "$(id -u)" -x synaps-avatar >/dev/null 2>&1; then break; fi
        sleep 0.2
    done
    if [ "$STARTED" = 1 ]; then
        good "she's up ($(awk '/first-frame: painted/ { print $3 " ms"; exit }' "$log") to her first frame)"
    elif pgrep -u "$(id -u)" -x synaps-avatar >/dev/null 2>&1; then
        info "she's still starting: her log is $(tilde "$log")"
    else
        warn "she didn't start. the end of her log ($(tilde "$log")):"
        tail -n 4 "$log" 2>/dev/null | sed "s/^/       $D│$N /"
    fi
}

# What stays of Synaps after --uninstall, where his login lives, and how to remove it too.
# MINE: the synaps this script installed (there was none before), RC: the shell files its installer
# edited, RUSTUP: the rustup it installed. A Synaps he had (or one it upgraded) is his: only a pointer.
synaps_stays() {
    local mine=$1 rc=$2 rustup=$3 s h
    s=${mine:-$(command -v synaps 2>/dev/null || true)}
    [ -n "$s" ] || { [ -x "$CARGO_BIN/synaps" ] && s=$CARGO_BIN/synaps; } || true
    h=$(tilde "$(synaps_home)")
    if [ -n "$s" ]; then
        info "synaps stays installed (her daemon's other clients may use it): $(tilde "$s")"
        info "your Synaps login and sessions live in $h"
        if [ -n "$mine" ]; then
            info "to remove Synaps too (this installed it): $(q "$(tilde "$s")") daemon stop, then delete $(tilde "$s") and $(tilde "$CONF/synaps") ($h only if you want your login and sessions gone)"
            if [ -n "$rc" ] && [ "$rc" != none ]; then
                info "  its installer also added a line sourcing ~/.cargo/env to: $rc (keep it if you use Rust)"
            fi
        else
            info "to remove Synaps too: synaps daemon stop, then uninstall it the way you installed it"
        fi
    fi
    if [ -n "$rustup" ]; then info "rustup stays too (this installed it for her build): \`rustup self uninstall\` removes it"; fi
}

summary() {
    local w=14
    printf '\n %s▌installed%s%s\n' "$R$B" "$N" "$([ "$DRY" = 1 ] && printf '  %s(dry run: nothing was)%s' "$R" "$N")"
    printf '   %-*s %s\n' $w synaps "${SYNAPS_V:-?}  $(tilde "$SYNAPS")"
    printf '   %-*s %s\n' $w synaps-avatar "${BIN_VER:-?}  $(tilde "$BIN/synaps-avatar")"
    printf '   %-*s %s\n' $w "her pack" "capri (CapriCore v1.0.4 + lachy$([ "$LOCAL_ART" = 1 ] && echo " + mochi, local only"))  $(tilde "$PACK")"
    printf '   %-*s %s\n' $w daemon "${DAEMON_STATE:-?}, ${DAEMON_HOW:-?}"
    printf '   %-*s %s\n' $w summon "${SUMMON:-?}"
    printf '   %-*s %s\n' $w autostart "${AUTOSTART_HOW:-?}"
    if [ -n "${VOICE_HOW:-}" ]; then printf '   %-*s %s\n' $w voice "$VOICE_HOW"; fi
    if [ -n "${WATCH_HOW:-}" ]; then printf '   %-*s %s\n' $w watch "$WATCH_HOW"; fi
    local how='click her to talk'
    if [ -n "${SUMMON_KEY:-}" ]; then how="tap $SUMMON_KEY (or click her) to talk"; fi
    case ${SUMMON:-} in 'your own'*) how='your key summons her' ;; esac
    brain
    printf '\n'
    start_her
    if [ "${STARTED:-0}" = 1 ]; then
        printf '\n   %sshe'"'"'s on your screen:%s %s\n' "$B$T" "$N" "$how"
        info "start her again: $(launch_cmd show) &"
    else
        printf '\n   %sstart her:%s %s &      %sthen %s%s\n' "$B$T" "$N" "$(launch_cmd show)" "$D" "$how" "$N"
    fi
    case ":$PATH:" in *":$BIN:"*) ;; *) info "$(tilde "$BIN") isn't on your PATH: add it to use \`synaps-avatar\` and \`avatarctl\` by name" ;; esac
    info "remove her: $(self_cmd) --uninstall   (synaps stays)"
}

# This script as he'd run it again: the checkout's or the tarball's file, else the piped form.
self_cmd() {
    if [ -n "$CHECKOUT" ]; then
        q "$(tilde "$CHECKOUT/install.sh")"
    elif [ -n "$BUNDLE" ]; then
        q "$(tilde "$BUNDLE/install.sh")"
    else
        printf 'curl -fsSL https://raw.githubusercontent.com/%s/%s/install.sh | bash -s --' "$REPO_DEFAULT" "${REF:-main}"
    fi
}

# ---- uninstall --------------------------------------------------------------------------------

# --purge: her own files in those folders, and only those (another pack, his own files stay). Her
# pack isn't one: the receipt's `dir` entry removes it when this script made it, never otherwise.
purge() {
    local f
    for f in "$CONF/synaps-avatar/settings.toml" "$CONF/synaps-avatar/settings.toml.tmp" \
        "$STATE/synaps-avatar/state.json" "$STATE/synaps-avatar/state.json.tmp" "$STATE/synaps-avatar/launch.log" \
        "$STATE/synaps-avatar/launch.old.log" \
        "$STATE/synaps-avatar/sessions.json" "$STATE/synaps-avatar/sessions.json.tmp"; do
        if [ -f "$f" ]; then run rm -f "$f"; fi
    done
    for f in "$CACHE_ROOT/installer" "$CACHE_ROOT/capri-desktop"; do
        if [ -d "$f" ]; then run rm -rf "$f"; fi
    done
}

# A WM config a block may be taken out of.
editable() { case $1 in "${CONF%/}"/* | "$HOME/.i3/config" | "$HOME/.sway/config") [ -f "$1" ] ;; *) return 1 ;; esac; }
skipped() { warn "left alone: $1 (not hers to remove: the receipt is wrong there)"; }

uninstall() {
    STEPS=1 STEP_N=0
    step "Remove what install.sh installed"
    local d rmdirs=() syn_mine syn_rc rust_mine
    # Read now: the receipt is gone by the end.
    syn_mine=$(meta synaps) syn_rc=$(meta synaps_rc) rust_mine=$(meta rustup)
    if [ -f "$RECEIPT" ]; then
        if pgrep -u "$(id -u)" -x synaps-avatar >/dev/null 2>&1; then
            if [ -x "$BIN/avatarctl" ]; then run "$BIN/avatarctl" quit || true; else run pkill -u "$(id -u)" -x synaps-avatar || true; fi
        fi
        local line kind rest tag f reload=0
        while IFS= read -r line; do
            [ -n "$line" ] || continue
            kind=${line%%$'\t'*} rest=${line#*$'\t'}
            if [ "$rest" = "$line" ]; then
                warn "a receipt line this script can't read (left alone): $line"
                continue
            fi
            case $kind in
                block | unit) tag=${rest%%$'\t'*} f=${rest#*$'\t'} ;;
                *) tag='' f=$rest ;;
            esac
            case $kind in
                prefix | meta) ;;
                backup) if [ -f "$f" ]; then info "kept your config's backup: $(tilde "$f")"; fi ;;
                block) if editable "$f"; then block_rm "$f" "$tag"; else skipped "$f"; fi ;;
                unit)
                    case $tag in synaps-daemon.service | capri-voice.service | capri-watch.service) ;; *) skipped "$tag"; continue ;; esac
                    if ! removable "$f" || [ "$(basename "$f")" != "$tag" ]; then
                        skipped "$f"
                        continue
                    fi
                    # His now (rewritten without our header), or gone: not disabled, not removed.
                    if ! ours "$f"; then
                        if [ -f "$f" ]; then info "$(tilde "$f") isn't the one this wrote any more: left as it is"; fi
                        continue
                    fi
                    if systemd_user; then
                        if [ "$tag" = synaps-daemon.service ]; then
                            # The daemon keeps running (other clients may use it); it just no longer starts at login.
                            run systemctl --user disable "$tag" || true
                        else
                            run systemctl --user disable --now "$tag" || true
                        fi
                    fi
                    run rm -f "$f"
                    reload=1
                    ;;
                file)
                    if ! removable "$f"; then
                        skipped "$f"
                    elif [ -e "$f" ] || [ -L "$f" ]; then
                        run rm -f "$f"
                    fi
                    ;;
                dir)
                    if ! removable "$f" || ! known_dir "$f"; then
                        skipped "$f"
                    elif [ -d "$f" ]; then
                        run rm -rf "$f"
                    fi
                    ;;
                rmdir) if removable "$f" rmdir; then rmdirs+=("$f"); else skipped "$f"; fi ;;
                *) warn "a receipt entry this script doesn't know (left alone): $line" ;;
            esac
        done < <(tac "$RECEIPT")
        if [ "$reload" = 1 ] && systemd_user; then run systemctl --user daemon-reload; fi
        run rm -f "$RECEIPT"
    else
        info "no receipt ($(tilde "$RECEIPT")): nothing installed by this script"
    fi
    for d in "$PACK".pre-install-*; do
        if [ -d "$d" ]; then info "kept: the pack that was there before she was installed, $(tilde "$d") (yours)"; fi
    done
    if [ "$PURGE" = 1 ]; then
        purge
    else
        info "kept: your settings ($(tilde "$CONF/synaps-avatar")) and her state ($(tilde "$STATE/synaps-avatar")); --purge removes them"
    fi
    # Her folders if they're empty now, then the ones this made (innermost first) if they are.
    for d in "$CONF/synaps-avatar" "$STATE/synaps-avatar" "$(dirname "$PACK")" "$DATA/synaps-avatar" "$CACHE_ROOT" "${rmdirs[@]}"; do
        if [ -d "$d" ] && [ -z "$(ls -A "$d")" ]; then run rmdir "$d"; fi
    done
    if [ "$PURGE" = 1 ] && [ "$DRY" = 0 ]; then
        for d in "$CONF/synaps-avatar" "$STATE/synaps-avatar" "$DATA/synaps-avatar" "$CACHE_ROOT"; do
            if [ -d "$d" ]; then info "left in $(tilde "$d"), not hers: $(cd "$d" && find . -mindepth 1 -maxdepth 1 | sed 's#^\./##' | head -n5 | tr '\n' ' ')"; fi
        done
    fi
    synaps_stays "$syn_mine" "$syn_rc" "$rust_mine"
    printf '\n %s▌removed%s%s\n' "$R$B" "$N" "$([ "$DRY" = 1 ] && printf '  %s(dry run: nothing was)%s' "$R" "$N")"
}

main() {
    STEP_N=0
    # `irm | iex` can't pass flags (install.ps1 reads them from here too); `curl | bash -s --` can.
    # Read once, said in the banner, and not passed on to anything this runs.
    FLAGS_ENV=''
    if [ -n "${CAPRI_INSTALL_FLAGS:-}" ]; then
        if [ $# -eq 0 ]; then
            local -a fl
            read -r -a fl <<<"$CAPRI_INSTALL_FLAGS"
            set -- "${fl[@]}"
            FLAGS_ENV="used:${fl[*]}"
        else
            FLAGS_ENV="ignored:$CAPRI_INSTALL_FLAGS"
        fi
    fi
    unset CAPRI_INSTALL_FLAGS
    args "$@"
    ui
    root_check
    TMP=$(mktemp -d "${TMPDIR:-/tmp}/capri-install.XXXXXX")
    trap 'rm -rf "$TMP"' EXIT
    locate
    paths
    if [ "$UNINSTALL" = 1 ]; then
        # Uninstalling needs no network (no manifest), and works where the installs went.
        local p
        PREFIXES=()
        while IFS= read -r p; do
            case $p in /?*) PREFIXES+=("$p") ;; *) warn "the receipt's prefix isn't an absolute path: $p (ignored)" ;; esac
        done < <(awk -F'\t' '$1 == "prefix" { sub(/^[^\t]*\t/, ""); print }' "$RECEIPT" 2>/dev/null || true)
        if [ ${#PREFIXES[@]} -gt 0 ]; then
            p=${PREFIXES[${#PREFIXES[@]} - 1]}
            if [ "$p" != "$PREFIX" ]; then
                [ "$PREFIX_GIVEN" = 0 ] || warn "--prefix $PREFIX: the receipt says she went to $p; she is removed from there"
                PREFIX=$p
                paths
            fi
        else
            PREFIXES=("$PREFIX")
        fi
        banner
        uninstall
        return 0
    fi
    manifest
    banner
    preflight
    receipt_init
    rec prefix "$PREFIX"
    step_synaps
    step_binary
    step_pack
    step_daemon
    step_summon
    step_autostart
    step_extras
    summary
}

main "$@"
