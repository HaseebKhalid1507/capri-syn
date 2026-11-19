#!/usr/bin/env bash
# Checks a Linux artifact (installer/stage.sh's layout, or the release tarball) carries her licence and
# every notice that goes with her binary (installer/licenses/list), each the same bytes as the repo's,
# and the wake-word model's NOTICE beside it. CI runs it: the `installer` job on a staged tree (no
# third-party file: no cargo there), release.yml's linux job on the tarball itself (--third-party).
#
#   installer/licenses/check.sh SRC DIR_OR_TARBALL [--third-party]
set -euo pipefail
[ $# -ge 2 ] || { sed -n '2,7p' "$0" >&2; exit 2; }
src=$(cd "$1" && pwd)
art=$2
want_third=${3:-}
if [ -f "$art" ]; then
    x=$(mktemp -d)
    trap 'rm -rf "$x"' EXIT
    tar -xzf "$art" -C "$x"
    art=$(find "$x" -mindepth 1 -maxdepth 1 -type d | head -n1)
fi
l=$art/lib/synaps-avatar
bad=0
ok() { echo "ok   $*"; }
no() { echo "FAIL $*"; bad=1; }
# A THIRD-PARTY-LICENSES.txt read by its sections (about.hbs's layout, as check.ps1 reads it): prints
# its number of crate entries, then `yes` if CRATE sits in an Apache-2.0 section that holds the
# licence's full text (not merely somewhere in the file), else `no`.
third_party() { # FILE CRATE
    awk -v want="$2" '
        BEGIN { r = sprintf("%80s", ""); d = r; gsub(/ /, "=", r); gsub(/ /, "-", d) }
        $0 == r { getline; head = $0; crates = 1; listed = 0; full = 0; next }
        crates && $0 == d { crates = 0; next }
        crates && /^  [^ ]+ [^ ]+/ { n++; if ($1 == want) listed = 1; next }
        !crates && index($0, "Version 2.0, January 2004") { full = 1 }
        head ~ /\(Apache-2\.0\), used by:$/ && listed && full { ok = 1 }
        END { print n + 0, (ok ? "yes" : "no") }' "$1"
}
same() { # FILE IN_ARTIFACT
    if [ ! -s "$2" ]; then no "${2#"$art"/}: missing"
    elif cmp -s "$1" "$2"; then ok "${2#"$art"/}"
    else no "${2#"$art"/}: not the repo's ${1#"$src"/}"; fi
}
same "$src/LICENSE" "$art/LICENSE"
same "$src/tools/wake/NOTICE" "$l/tools/wake/NOTICE"
[ -s "$l/tools/wake/hey-capri-v1.npz" ] || no "lib/synaps-avatar/tools/wake/hey-capri-v1.npz: missing (its NOTICE goes beside it)"
while IFS=$'\t' read -r from to; do
    case $from in '' | '#'*) continue ;; esac
    if [ "$from" = @third-party@ ]; then
        f=$l/$to
        if [ -z "$want_third" ]; then
            if [ -e "$f" ]; then no "${f#"$art"/}: in a build without --third-party"; fi
        elif [ ! -s "$f" ]; then
            no "${f#"$art"/}: missing"
        else
            read -r entries gpui < <(third_party "$f" gpui-pre-linux)
            if [ "$gpui" = yes ]; then ok "${f#"$art"/}: gpui-pre-linux under Apache-2.0's text ($entries crate entries)"
            else no "${f#"$art"/}: no gpui-pre-linux under Apache-2.0's text ($entries crate entries)"; fi
        fi
        continue
    fi
    same "$src/${from//@os@/linux}" "$l/$to"
done <"$src/installer/licenses/list"
if [ "$bad" = 0 ]; then echo "her licence and notices: all there"; else echo "her licence and notices: some missing (above)"; fi
exit "$bad"
