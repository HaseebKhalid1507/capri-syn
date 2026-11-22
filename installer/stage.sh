#!/usr/bin/env bash
# Lays out her Linux install tree: the release tarball's contents (.github/workflows/release.yml), and
# what install.sh copies after a source build. One layout, so both paths install the same files:
#
#   bin/synaps-avatar  bin/avatarctl
#   lib/synaps-avatar/ VERSION, installer/manifest, packs/capri/{moods.json,persona.md},
#                      tools/ (make-pack and its pins, capri-voice, capri-watch, the user units, …),
#                      LICENSE and licenses/ (installer/licenses/list: the notices that go with her
#                      binary), tools/wake/NOTICE beside the wake-word model
#   install.sh         run it from the unpacked tarball to install that tarball
#   README.md CHANGELOG.md LICENSE
#
#   installer/stage.sh SRC BINARY OUT_DIR [VERSION [THIRD_PARTY]]
# VERSION: default `git describe`, else "source". THIRD_PARTY: the crates' licences for this build
# (installer/licenses/third-party.ps1; release.yml passes it). A source build has none: nothing of it
# is redistributed.
set -euo pipefail
[ $# -ge 3 ] || { sed -n '2,16p' "$0" >&2; exit 2; }
src=$(cd "$1" && pwd)
bin=$2
out=$3
ver=${4:-$(git -C "$src" describe --tags --always --dirty 2>/dev/null || echo source)}
third=${5:-}
[ -z "$third" ] || [ -s "$third" ] || { echo "stage.sh: no third-party licences at $third" >&2; exit 2; }

rm -rf "$out"
mkdir -p "$out/bin" "$out/lib/synaps-avatar/tools/systemd" "$out/lib/synaps-avatar/tools/wake" \
    "$out/lib/synaps-avatar/tools/capri-watch" "$out/lib/synaps-avatar/packs/capri" "$out/lib/synaps-avatar/installer"
install -m 755 "$bin" "$out/bin/synaps-avatar"
install -m 755 "$src/tools/avatarctl" "$out/bin/avatarctl"

l=$out/lib/synaps-avatar
printf '%s\n' "$ver" >"$l/VERSION"
install -m 644 "$src/installer/manifest" "$l/installer/manifest"
install -m 644 "$src/packs/capri/moods.json" "$src/packs/capri/persona.md" "$l/packs/capri/"
# make-pack and everything it reads (its pins, the desktop art's geometry, the hair recolour).
install -m 755 "$src/tools/make-pack" "$l/tools/"
install -m 644 "$src"/tools/*.sha256 "$src/tools/desktop-art.json" "$src/tools/capricore-outfit.json" "$src/tools/mochi_hair.py" "$l/tools/"
# Voice, watch and the remote helper (installed into bin/ by --voice / --watch).
install -m 755 "$src/tools/capri-voice" "$src/tools/capri-voice-setup" "$src/tools/synaps-avatar-remote" "$l/tools/"
install -m 644 "$src/tools/wake/hey-capri-v1.npz" "$src/tools/wake/NOTICE" "$l/tools/wake/"
install -m 755 "$src/tools/capri-watch/capri-watch" "$l/tools/capri-watch/"
install -m 644 "$src/tools/capri-watch/watch.toml" "$l/tools/capri-watch/"
install -m 644 "$src"/tools/systemd/*.service "$l/tools/systemd/"
install -m 755 "$src/install.sh" "$out/install.sh"
install -m 644 "$src/README.md" "$src/CHANGELOG.md" "$src/LICENSE" "$out/"
# Her licence and the notices that go with her binary (installer/licenses/list), installed with her.
while IFS=$'\t' read -r from to; do
    case $from in '' | '#'*) continue ;; esac
    if [ "$from" = @third-party@ ]; then
        [ -n "$third" ] || continue
        from=$third
    else
        from=$src/${from//@os@/linux}
    fi
    mkdir -p "$(dirname "$l/$to")"
    install -m 644 "$from" "$l/$to"
done <"$src/installer/licenses/list"
