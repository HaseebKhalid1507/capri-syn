# Vendored: synaps-client

This crate is a copy of `crates/synaps-client` from
[synaps-desk](https://github.com/HaseebKhalid1507/synaps-desk) at commit
`4101e93483127d7b31e84b4aeaa21a678fbd2955` (branch `feat/client-set-system`; the
sanitized rewrite of synaps-desktop `cea3639`). Capri depends on it by path, so capri-syn
builds with no git dependencies and no tokens.

Changed from upstream:
- `Cargo.toml`: workspace-inherited fields and dependencies written out inline, and
  `license = "MIT AND Apache-2.0"` (MIT, except the Apache-2.0 portions `NOTICE` names).
- Added `LICENSE`, `LICENSE-APACHE`, `NOTICE` and this file.

`src/transcript.rs` and `tests/fixtures.rs` differ from upstream by one fix, to port back: a turn's
error or abort note (`turn_notice`) is anchored to the end of its turn (`Item::after_turns`,
`opens_turn`), so a refold (a re-attach, a resync) that brings the turn's prompt after the note no
longer puts the note above the prompt (`a_turns_error_stays_after_its_turn_on_a_refold`). Nothing
else in `src/` or `tests/` differs. To check:
`git -C <synaps-desk> diff --stat 4101e93 -- crates/synaps-client` against this directory.

Syncing: port fixes by hand in either direction, and update the commit above. The long-term
plan is to move the client into SynapsCLI, next to the daemon whose protocol it speaks, and
depend on it from there.
