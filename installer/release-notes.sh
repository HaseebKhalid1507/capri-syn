#!/usr/bin/env bash
# A release's notes: CHANGELOG.md's section for VERSION, without its heading. Its heading is
# `## VERSION`, optionally with a leading v and anything after a space (`## 1.0.0 — 2026-12-01`).
# release.yml publishes these (gh release create --notes-file), never --generate-notes: the merged
# PRs' titles are a developer's log, not notes for the people installing her.
#
#   installer/release-notes.sh VERSION [CHANGELOG]            # a tag: its section, or exit 1
#   installer/release-notes.sh --dry-run VERSION [CHANGELOG]  # its section, else `## Unreleased`'s
set -euo pipefail
dry=0
if [ "${1:-}" = --dry-run ]; then dry=1; shift; fi
[ $# -ge 1 ] || { sed -n '2,8p' "$0" >&2; exit 2; }
ver=${1#v}
log=${2:-CHANGELOG.md}

# The section under a heading whose first word (a leading v dropped) is $1; blank lines trimmed.
section() {
    awk -v want="$1" '
        /^## / {
            if (on) exit
            h = substr($0, 4); sub(/^v/, "", h); split(h, w, /[ \t]/)
            if (w[1] == want) on = 1
            next
        }
        on { buf[++n] = $0 }
        END {
            a = 1; while (a <= n && buf[a] ~ /^[ \t]*$/) a++
            b = n; while (b >= a && buf[b] ~ /^[ \t]*$/) b--
            for (i = a; i <= b; i++) print buf[i]
        }' "$log"
}

# Whether a heading's first word (a leading v dropped) is $1, empty or not.
has() {
    awk -v want="$1" '
        /^## / { h = substr($0, 4); sub(/^v/, "", h); split(h, w, /[ \t]/); if (w[1] == want) { f = 1; exit } }
        END { exit !f }' "$log"
}

# What $log has for heading $1, when its section gave nothing: none, or an empty one.
# shellcheck disable=SC2016 # the backticks are Markdown's
what() {
    if has "$1"; then printf 'an empty `## %s` section' "$1"; else printf 'no `## %s` section' "$1"; fi
}

notes=$(section "$ver")
if [ -n "$notes" ]; then
    printf '%s\n' "$notes"
    exit 0
fi
if [ "$dry" = 0 ]; then
    echo "release-notes: $log has $(what "$ver"): write it (what changed for the people installing her), then tag" >&2
    exit 1
fi
notes=$(section Unreleased)
[ -n "$notes" ] || { echo "release-notes: $log has $(what "$ver") and $(what Unreleased): nothing to show yet (write what changed under \`## Unreleased\`)" >&2; exit 1; }
# shellcheck disable=SC2016 # the backticks are Markdown's
printf '> **Dry run.** No notes under `## %s` yet: these are the `## Unreleased` notes.\n\n%s\n' "$ver" "$notes"
