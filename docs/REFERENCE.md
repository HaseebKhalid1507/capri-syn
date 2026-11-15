# Reference

Reference for `synaps-avatar`, the crate behind capri-syn. The [README](../README.md) is the tour.

## The session rules

- **Observe by default; Mirror only to act.** She watches her session in `Observe`. A Mirror attach
  keeps the session from parking and makes it eligible for keep-warm pings, which cost tokens.
  She re-attaches in `Mirror` when you write to her (the Line or the text field) or when she has to
  submit, answer a prompt or abort. That input waits for the Mirror `Attached`. She drops back to
  Observe 3 s after the turn is done, once nothing is pending. Each switch is a new attach with its own
  `attach_seq`. Events from the old one are dropped, and the full-replace `Attached` keeps the body and
  the bubble as they were.
- **Her own session.** With no `--session`, she uses a session named after the pack's character,
  lowercased (`capri`). She attaches to it if it's live. Otherwise she resumes the id she remembered last
  time (`~/.local/state/synaps-avatar/sessions.json`, one entry per daemon), else creates it by name.
  She only rides another session when you pass `--session`.
- **Never `subconscious`.** She refuses to attach to the session named `subconscious`, by name or by id.
  The refusal happens at argument parsing, when resolving the session list, and on `Attached`
  (she detaches and exits with status 2).
- **Hide and quit.** HIDE (right-click menu) parks her: she glitches out and a small chip with her name
  sits at the edge of her monitor; a click on it brings her back, as does `avatarctl show`. QUIT exits
  and asks for a second click first.
- She shows up in `/clients` as `capri` (the pack's character name). If the daemon is gone or the
  tunnel is down, she shows Away and comes back by herself. The client is re-created if it stops, or
  after 90 s offline.

## What she shows

- **Moods** come from the session's turn events: thinking, talking, `WORKING · <tool>`, done,
  annoyed (abort or error), needs-you (a pending prompt), sleepy, compacting, and away (no daemon).
- **The Line** (the summon key, tapped) is the way to write to her session: a thread with the
  conversation so far, a text field, prompts answered from the keyboard. It stays open after Enter;
  Esc closes it and keeps the draft. A reply that lands while it's closed leaves a one-line strip on her
  tag until you open it.
- **Her bubble** shows short replies as they stream. A tap on her body also opens a text field there.
  Long replies, and replies with code, open in the **reader** beside her (scrollable, copy buttons).
- **Prompts**: ALLOW/DENY for a confirm, a masked field for a secret. Anything that runs, writes or
  deletes needs <kbd>A</kbd> held (or the ALLOW button held) for 0.45 s; a tap does nothing.

## Every setting

`~/.config/synaps-avatar/settings.toml`; every key is optional, and an unknown key or an out-of-range
value rejects the whole file (the previous values stay). `avatarctl settings reload` re-reads it. The
[README](../README.md#configure) shows the common ones with their defaults.

| key | default | |
|---|---|---|
| `[brain] enabled` | `true` | the cat brain (roam, bids, summons). Off: she stays where she's put |
| `[brain] attention` | `"mix"` | `"mix"`, `"hang_only"` or `"off"` (no bids). Also in the right-click menu |
| `[brain] approaches` | hang 2, hang_upside_down 1, cling 3, sit_edge 3, peek 1, beside 1 | bid approach weights; 0 turns one off |
| `[brain] arrivals` | teleport 3, glitch 1, drop_in 2, hop 1 | how she gets there |
| `[brain] no_repeat` | `2` | her approach, arrival and emote each avoid her last N picks (while another is left) |
| `[brain] passive_every_min` | `[3, 10]` | minutes between her own moves |
| `[brain] bid_every_min` | `[20, 30]` | minutes between attention bids |
| `[brain] max_moves_per_hour` | `15` | her own moves per hour, at most |
| `[brain] cover_max_pct` | `12` | how much of a window she may cover when she picks a spot (0..50 %) |
| `[brain] fullscreen` | `"hide"` | `"hide"` while a window is fullscreen on her monitor, or `"stay"` (and never move) |
| `[brain] monitor` | `"follow"` | `"follow"` the monitor you work on, or `"any"` |
| `[ragdoll] strength` | `0.5` | 0 = rigid, 1 = the full-strength chain (bend, squash, shake, bid swing) |
| `[mirror] drag` | `true` | turn while dragged and flung (`[drag] mirror` is an alias) |
| `[mirror] drag_facing` | `"back"` | look `"back"` where she's pulled from, or `"forward"` |
| `[mirror] self` | `true` | her own turns: travel, what she cares about, an idle look-around |
| `[mirror] idle_every_min` | `[1, 4]` | minutes between idle look-arounds |
| `[mirror] turn_ms` | `150` | the turn's squeeze (0 = an instant flip, at most 400) |
| `[display] composited` | `"auto"` | `"auto"` (alpha while a compositor runs, the X shape otherwise), `"on"`, `"off"` |
| `[music] detect` | `true` | headphones on while an MPRIS player is playing |
| `[music] ignore` | `["discord", "vesktop"]` | player bus-name substrings that never count |
| `[session] model` | daemon default | her own session's model |
| `[session] thinking` | daemon default | `"low"`, `"medium"`, `"high"` or `"xhigh"` |
| `[session] persona` | `true` | send the pack's `persona.md` as her session's system prompt |
| `[session] persona_file` | none | a persona file of your own instead |
| `[voice] enabled` | `false` | push-to-talk and spoken replies, through `capri-voice` |
| `[voice] stt_model` | `"small.en"` | `"small.en"`, `"base.en"` or `"medium.en"` |
| `[voice] voice` | a stock Kokoro blend | a Kokoro voice or a blend (`"af_bella:0.6,af_nicole:0.4"`); `+glitch` adds the trace |
| `[voice] speed` | `1.05` | 0.5..2 |
| `[voice] duck` | `"pause"` | other audio while she listens or speaks: `"pause"` (MPRIS), `"duck"` (PipeWire) or `"off"` |
| `[voice] key` | `"c"` | the talk key's letter, so an X auto-repeat isn't taken for a release. Match your binding |

`[session]` applies only to her own session, never to one she rides with `--session`.

## Flags

```
synaps-avatar [--profile P | --socket PATH] [--session NAME|ID | --new [--cwd DIR] [--model M]] [--no-session]
              [--sleepy-after MIN] [--pack DIR] [--height PX] [--name NAME] [--threads N] [--no-verify]
              [--focus-probe] [--check]
```

| flag | |
|---|---|
| `--profile P` | daemon profile (default `$SYNAPS_PROFILE`, else the default daemon) |
| `--socket PATH` | this daemon socket instead (e.g. a forwarded remote daemon). Excludes `--profile` |
| `--session S` | ride this live session (name or id; never `subconscious`) |
| `--new` | create a new session (`--cwd`: absolute, on the daemon's machine; `--model`) and follow it |
| `--no-session` | the body alone (dev control only) |
| `--sleepy-after MIN` | sleepy after this many minutes without a turn (default 10) |
| `--pack DIR` | character pack (default `$XDG_DATA_HOME/synaps-avatar/packs/capri`) |
| `--height PX` | standing body height in logical px (default 340) |
| `--check` | load and decode the pack, print a summary, exit (no window) |

`synaps-avatar --help` lists the rest (`--name`, `--threads`, `--no-verify`, `--focus-probe`).

Environment knobs: `SA_COMPOSITED=0|1` overrides `[display] composited`. `SA_ANY_GPU=1` lets GPUI pick
the GPU (by default it uses the boot VGA device), `SA_GL=1` brings back GPUI's GL probe, `SA_TIMING=1`
logs Enter → first reply text → first word on screen, `SA_DEBUG=1` logs attach mode switches, and
`SA_RESPAWN_AFTER=<s>` sets the offline time before the client is re-created (tests).
`CAPRI_VOICE_SOCK` points her at a voice sidecar socket other than `$XDG_RUNTIME_DIR/capri-voice.sock`.

## A daemon on another machine

`tools/synaps-avatar-remote <host> [avatar args...]`:

- it forwards `<host>:~/.synaps-cli/run/daemon.sock` over `ssh -L` to
  `$XDG_RUNTIME_DIR/synaps-<host>-capri.sock` (0600);
- it runs `synaps-avatar --socket <that socket>` with any extra args (e.g. `--session NAME`);
- the tunnel survives daemon reloads and restarts. If ssh itself dies (network, host asleep), a loop
  starts it again 5 s later. She shows Away meanwhile and reattaches by herself;
- the tunnel and its socket go away when the avatar exits.

`Cwd::Path` for a session she creates must exist on **the daemon's machine**.

## Character packs

A pack is a directory:

| file | |
|---|---|
| `sprites.json` | the sprite catalog (CapriCore's `runtime-sprite-set.json`, schema 2): canvas, canonical anchor, sprites with pose/expression/anchor/visible bounds/sha256, clips (blink, speech), actions |
| `*.png` | the frames the catalog names (1254×1254, sha256-verified unless `--no-verify`) |
| `poses/poses.json` + PNGs | docking poses (sit-edge for perching on windows, hanging, clinging, crouching, asleep on her side, ...) |
| `intro/*.png` | the arrival frames (CapriCore's intro-v1, 02–06): launch, $mod+c, some teleports, landing on a window top |
| `icon.png` | her icon (`_NET_WM_ICON` on her window and the Line) |
| `music/*.png` | headphones frames and note sprites for the music idle |
| `fonts/*.ttf` | the HUD font (Exo 2 static instances; OFL) |
| `moods.json` | **the avatar's behaviour sidecar**, our file: HUD tones, idle life (bob, sway, sparkles, activity timing), the mood table (frames, entry clips, labels, glyphs), idle activities, extra frames (an extra frame may paint another file's face onto it: `patch`), click reactions (head pat / body / annoyed streak), perch rules (one frame, plus the headpat face), music, arrivals (`arrive`: launch / summon / teleport / perch sequences, `teleportChance`), the deep nap (`brain.napDeep`, `napPerch`, `napDeepAfterMs`, `napWake`), the startle and big-leap crouch, the look straight ahead (`brain.lookFront`), the rare air kiss (`kiss`) and the icon. The code ships no character ids: everything character-specific is in here |
| `persona.md` | optional: her session's system prompt (`[session] persona`) |
| `NOTICE.md` | the art's notice, plus a line naming the CapriCore versions it came from |

`packs/capri/moods.json` is Capri's sidecar. Build the full pack from a CapriCore checkout with:

```
git clone https://github.com/CapriChooms/CapriCore /tmp/CapriCore
git -C /tmp/CapriCore checkout v1.0.4
tools/make-pack /tmp/CapriCore            # -> ~/.local/share/synaps-avatar/packs/capri
synaps-avatar --check                     # verifies every frame
```

`make-pack` is pinned to **CapriCore v1.0.4** (`bc93892`): it refuses any other checkout, and checks every
file it copies against `tools/capricore-v1.0.4.sha256` (1.1.x retired and redrew most of the frames her
behaviour uses). One frame comes from v1.1.2 (`bb0c2d7`): `pose-sleep`, checked against its own pinned
sha256. It's read from the checkout's git objects when they have it (`git fetch --tags`), else downloaded
from GitHub at that commit, or given with `--sleep FILE`. It needs `fonttools` for the Exo 2 instances.
`python3 tools/test_make_pack.py` tests it on fake checkouts.

## Building

Toolchain 1.100.0 (`rust-toolchain.toml`). GPUI is pinned exactly: `gpui-kit =0.9.1`,
`gpui-pre =0.3.12`.

`synaps-client` is vendored in `crates/synaps-client` (a workspace member, path dependency):
capri-syn has no git dependencies. Its source and how to sync it: `crates/synaps-client/VENDORED.md`.

### Maintainer notes

Builds go through `tools/abuild` (`build --release`, `test`, `clippy --all-targets`, `check`), never a
bare cargo on the build host. It mirrors the checkout to the build host (`ABUILD_HOST`) and runs cargo
through that host's shared build lane (lane locks, nice, memory cap, time limit), with one target dir.

## Dev control and test tools

The avatar listens on `$XDG_RUNTIME_DIR/synaps-avatar.sock`, one command per line. `tools/avatarctl`
sends one:

```
mood <name> | clip <id> | say <text> | pos <x> <y> [now] | perch [sit|stand] [win] | perch off | drop | stack
page next|prev | scroll <n> | reader open [focus]|close|copy [n]|scroll <px> | collapse | expand
music on|off | glitch | face left|right|toggle | drag … | submit <text> | abort | copy
talk press|release | line close | prompt confirm|secret [text] | prompt off
brain … | settings reload | tap head|body | menu | shape | state | hide | show | quit
```

`brain` takes `on|off|state|dnd [min]|dnd off|scale <x>|seed <n>`, `force roam|seek|summon|sulk|nap`,
`event prompt [id]|done|error|abort|cleared` and `sense …` (focus, cursor, idle, fullscreen, near, arena,
windows, each with `real` to go back to the real senses).

- `tools/shoot.py` runs one short scenario on a desktop: launch, steps, screenshots, recordings,
  then always kills the app. Its guards matter because someone else may be using that desktop:
  - it only connects to the `desktopproto` sandbox profile or a fake daemon;
  - it clicks only inside the avatar's own input shape, and only after the user has been idle;
  - typed text goes to the avatar's window only (XSendEvent) and only when it has the keyboard;
  - it never moves the user's pointer except for a click, and puts it straight back afterwards.
- `tools/fakedaemon.py`: a fake Synaps daemon (the synaps-client fixtures, Secret/Confirm prompts and
  compaction on demand) for what the sandbox can't raise safely. Run the avatar with
  `--socket /tmp/sa-fake/d.sock`.
- `tools/fakempris.py`: a fake MPRIS player for the music idle.
- `tools/measure.py`: start-up, memory and CPU of the body alone (`--no-session`).
- `tools/sheet.py`: contact sheets from recordings. `tools/stacktest.py`: stacking against an i3 dialog.
- `tools/daylog.py`: audits a brain decision log (`SA_BRAIN_LOG`) against the hard rules.
  `tools/lagmeter.py`: frame pacing and input lag read off the screen. `tools/slabcheck.py`: black
  slabs in a recording. `tools/lockcheck.py`: two lockstep ragdoll recordings must show the same frames.

## Testing rules

- **Never a live daemon in tests.** Use the `desktopproto` sandbox profile or the fake daemon.
  A remote tunnel is only checked read-only (`synaps-probe --socket <tunnel> --list`).
- Every run on a live desktop is short (≤ 30 s) and leaves nothing behind.
- No real secrets, ever: secret prompts are tested against the fake daemon only.

CI (`.github/workflows/ci.yml`) runs clippy and the unit tests headless. Nothing in them needs a display
or a GPU. No secrets and no git dependencies: `synaps-client` is vendored. It covers the whole workspace,
so the client's own tests run too.
