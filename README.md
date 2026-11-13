# synaps-avatar

A desktop companion for [Synaps](https://github.com/HaseebKhalid1507/SynapsCLI). It's a transparent,
click-through, always-on-top overlay (X11 + [GPUI](https://github.com/longbridge/gpui-component))
that animates a character pack and is driven by one Synaps daemon session:

- **moods** come from the session's turn events: thinking, talking, `WORKING · <tool>`, done,
  annoyed (abort or error), needs-you (a pending prompt), sleepy, compacting, and away (no daemon);
- the **reply** streams into a speech bubble;
- **tap her body** to open a text field. Enter sends (or steers while a turn runs), Esc closes, and
  Esc Esc aborts a running turn;
- **prompts** show in the bubble: ALLOW/DENY for a confirm, a masked field for a secret. Esc puts a
  prompt off behind the status tag;
- drag her anywhere or perch her on a window edge; right-click gives a menu (reset position, hide).

The first character is **Capri**, built from [CapriCore](https://github.com/CapriChooms/CapriCore)'s art.
The art is third-party, so it is **not in this repo**: `tools/make-pack` assembles the pack on your machine.

## The session rules

- **Observe by default; Mirror only to act.** She watches her session in `Observe`. A Mirror attach
  keeps the session from parking and makes it eligible for keep-warm pings, which cost tokens.
  She re-attaches in `Mirror` when the text field opens or when she has to submit, answer a prompt or
  abort. That input waits for the Mirror `Attached`. She drops back to Observe 3 s after the turn is
  done, once nothing is pending. Each switch is a new attach with its own `attach_seq`. Events from
  the old one are dropped, and the full-replace `Attached` keeps the body and the bubble as they were.
- **Her own session.** With no `--session`, she uses a session named after the pack's character,
  lowercased (`capri`). She attaches to it if it's live. Otherwise she resumes the id she remembered last
  time (`~/.local/state/synaps-avatar/sessions.json`, one entry per daemon), else creates it by name.
  She only rides another session when you pass `--session`.
- **Never `subconscious`.** She refuses to attach to the session named `subconscious`, by name or by id.
  The refusal happens at argument parsing, when resolving the session list, and on `Attached`
  (she detaches and exits with status 2).
- **Hidden means detached.** HIDE (menu) quits, which detaches. The overlay can't be minimized.
- She shows up in `/clients` as `capri` (the pack's character name). If the daemon is gone or the
  tunnel is down, she shows Away and comes back by herself. The client is re-created if it stops, or
  after 90 s offline.

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

Environment knobs: `SA_ANY_GPU=1` lets GPUI pick the GPU (by default it uses the boot VGA device),
`SA_GL=1` brings back GPUI's GL probe, `SA_TIMING=1` logs Enter → first reply text → first word on screen,
`SA_DEBUG=1` logs attach mode switches, and `SA_RESPAWN_AFTER=<s>` sets the offline time before the
client is re-created (tests).

## Capri on avante's daemon, from bella

`tools/synaps-avatar-remote [host] [avatar args...]` (default host `avante`), installed as
`~/.local/bin/synaps-avatar-remote` with the launcher `tools/synaps-avatar-avante.desktop`:

- it forwards `<host>:~/.synaps-cli/run/daemon.sock` over `ssh -L` to
  `$XDG_RUNTIME_DIR/synaps-<host>-capri.sock` (0600, a different name from the desktop app's tunnel);
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
| `poses/poses.json` + PNGs | docking poses (sit-edge for perching on windows, ...) |
| `music/*.png` | headphones frames and note sprites for the music idle |
| `fonts/*.ttf` | the HUD font (Exo 2 static instances; OFL) |
| `moods.json` | **the avatar's behaviour sidecar**, our file: HUD tones, idle life (bob, sway, sparkles, activity timing), the mood table (frames, entry clips, labels, glyphs), idle activities, extra frames, click reactions (head pat / body / annoyed streak), perch rules, music. The code ships no character ids: everything character-specific is in here |
| `NOTICE.md` | the art's notice |

`packs/capri/moods.json` is Capri's sidecar. Build the full pack from a CapriCore checkout with:

```
git clone https://github.com/CapriChooms/CapriCore /tmp/CapriCore
tools/make-pack /tmp/CapriCore            # -> ~/.local/share/synaps-avatar/packs/capri
synaps-avatar --check                     # verifies every frame
```

(`make-pack` needs `fonttools` for the Exo 2 instances.)

## Building

The build machine is bella. Never run cargo there by hand; use:

```
tools/abuild build --release      # also: test, clippy --all-targets, check
```

It mirrors the checkout to `bella:~/Projects/synaps-avatar` and runs cargo through bella's shared build
lane (`~/.cache/bbuild-lane`: lane locks, nice, memory cap, time limit). There is one target dir,
`bella:~/.cache/cargo-target-synaps-avatar`. Toolchain 1.100.0 (`rust-toolchain.toml`). GPUI is pinned
exactly: `gpui-kit =0.9.1`, `gpui-pre =0.3.12`.

`synaps-client` is a git dependency on the private synaps-desktop repo, pinned to an exact rev.
`.cargo/config.toml` sets `net.git-fetch-with-cli`, so the machine's GitHub credentials are used.

## Dev control and test tools

The avatar listens on `$XDG_RUNTIME_DIR/synaps-avatar.sock`, one command per line:
`mood <m>`, `clip <id>`, `say <text>`, `pos <x> <y> [now]`, `perch [sit|stand] [win]`, `perch off`,
`music on|off`, `glitch`, `state` (JSON), `hide`, `quit`; with a session also `submit <text>`, `abort`, and
`prompt confirm|secret [text]` (a fake prompt). `tools/avatarctl` sends one.

- `tools/shoot.py` runs one short scenario on the live desktop: launch, steps, screenshots, recordings,
  then always kills the app. Its guards matter because someone else may be using that desktop:
  - it only connects to the `desktopproto` sandbox profile or a fake daemon;
  - it clicks only inside the avatar's own input shape, and only after the user has been idle;
  - typed text goes to the avatar's window only (XSendEvent) and only when it has the keyboard;
  - it never moves the user's pointer except for a click, and puts it straight back afterwards.

  Run it from the bella mirror (`~/Projects/synaps-avatar/tools`), never from a stale copy.
- `tools/fakedaemon.py`: a fake Synaps daemon (the synaps-client fixtures, Secret/Confirm prompts and
  compaction on demand) for what the sandbox can't raise safely. Run the avatar with `--socket /tmp/sa-fake/d.sock`.
- `tools/measure.py`: start-up, memory and CPU of the body alone (`--no-session`).
- `tools/sheet.py`: contact sheets from recordings. `tools/stacktest.py`: stacking against an i3 dialog.

## Testing rules

- **Never a live daemon in tests.** Use the `desktopproto` sandbox profile on bella or the fake daemon.
  A remote tunnel is only checked read-only (`synaps-probe --socket <tunnel> --list`).
- Every run on a live desktop is short (≤ 30 s) and leaves nothing behind.
- No real secrets, ever: secret prompts are tested against the fake daemon only.

CI (`.github/workflows/ci.yml`) runs clippy and the unit tests headless. Nothing in them needs a display
or a GPU. It needs a `SYNAPS_DESKTOP_TOKEN` secret (read access to synaps-desktop) to fetch `synaps-client`.
