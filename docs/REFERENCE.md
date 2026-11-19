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
  and asks for a second click first. Under QUIT, one dim row that isn't a button: the daemon's version
  and her session's model (`SYNAPS 0.12.0 · CLAUDE-OPUS-5`, the provider left off), once the daemon has
  said them.
- **Her first words.** On her first launch ever (`state.json` has no `met` and nothing else of hers:
  no `ground`, `seenUnix`, `brain` or `perch`), once she has landed and nothing else is on, she says
  "Hey. I'm Capri. Click me when you want me. Headpats also accepted.", then `met` is set. Where she
  binds her key herself (Windows) it names it instead: "Tap WIN+ALT+C when you want me." (a WM's key
  may not exist: GNOME and KDE have none until he binds one). Not a word about what runs her, unless
  she has no brain (below; she waits up to 8 s for that read): "Hey. I'm Capri. Give me a brain and
  I'll talk back." with a red `SET ME UP ▸` that opens the Line on Settings ▸ Brain. If the Line opens first she skips it.
  Anyone who ran her before is marked met without it. `tools/shoot.py` writes `met` (`--first-run`:
  not).
- She shows up in `/clients` as `capri` (the pack's character name). If the daemon is gone or the
  tunnel is down, she shows Away and comes back by herself. The client is re-created if it stops, or
  after 90 s offline.

## What she shows

- **Moods** come from the session's turn events: thinking, talking, `WORKING · <tool>`, done,
  annoyed (abort or error), needs-you (a pending prompt), sleepy, compacting, and away (no daemon).
- **A failed turn she can name** is said in her words instead of the daemon's (`session::Trouble`,
  matched on the daemon's own wording): the bubble and her row in the Line get her line, her tag the
  name for 10 s, the Line's footer a quiet `↑ resends your message` (in place of the key legend). A fix that
  is a command ends on it in a monospace chip (`$ synaps login --provider claude`); a click copies it.
  The raw text stays in the log.

  | trouble | she says | tag | mood |
  |---|---|---|---|
  | no credentials (`synaps login` + no credential; 0.12.0: `No credentials for anthropic at …`) | No brain yet, choom. Synaps has no Claude login. Log in, or set another model ([session] model in my settings): `$ synaps login --provider claude` (the provider's own `--provider`; none named: `$ synaps login`) | `NO BRAIN YET` | idle |
  | a lapsed or rejected login | My login ran out. In a terminal: `$ synaps login` | `LOGGED OUT` | idle |
  | rate or usage limit, billing | Hit my model's limit for now. Give it a bit, then ask me again. | `AT THE LIMIT` | annoyed |
  | overloaded, a 5xx | My model's swamped right now. Try me again in a minute. | `MODEL BUSY` | annoyed |
  | unreachable, timed out, a dropped stream | Can't reach my model. Check the connection, then ask me again. | `NO CONNECTION` | annoyed |
  | `local/…`: `can't reach local endpoint` (Ollama's port 11434) | Ollama's not answering, so I've got no brain. Start Ollama, then ask me again. | `OLLAMA IS OFF` | idle |
  | the same, another port (LM Studio, llama.cpp) | My local model's server isn't answering. Start it, then ask me again. | `LOCAL MODEL OFF` | idle |
  | `local/<model>`: a 404 (Ollama hasn't pulled it) | qwen3:8b isn't on my local server. Pull it, then ask me again: `$ ollama pull qwen3:8b` | `MODEL MISSING` | idle |
  | the same, the docs' own Modelfile model (`capri-…`, no `:tag`, as [UNDER-THE-HOOD.md](UNDER-THE-HOOD.md#her-brain) makes it); any other name is pulled | capri-qwen3 isn't on my local server. Create it from its Modelfile, then ask me again: `$ ollama create capri-qwen3 -f Modelfile` | `MODEL MISSING` | idle |
  | `local/<model>`: a 400 (no tool calling; a context too small, where the server says so) | My local model turned that down. I need one with tool calling and a 16k+ context: set another ([session] model in my settings). In the Line, the server's own words under it. | `WRONG MODEL` | idle |

  Ollama cuts a prompt longer than its context without an error (4k below 24 GB of VRAM; hers is
  ~7k tokens): nothing to name there, so [UNDER-THE-HOOD.md](UNDER-THE-HOOD.md#her-brain) has a
  16k copy of the model made first.
- **No brain, before a word.** At each attach (and model change) she reads whether her model has
  credentials (`creds.rs`), by its provider as Synaps routes it (`provider.rs`: bare `claude-*` or
  `anthropic/…` is Claude, `<id>/…` that provider): an `<id>` or `<id>@<label>` entry in Synaps'
  `auth.json` (a profile's folder first; Claude's is `anthropic`; an API-key service's is `<id>`
  alone, `type: api_key` with a key in it), an API-key service's `provider.<id>` in Synaps' `config`
  (its last line for it; only if the daemon started after it was written, by 2 s or more so no
  file time's rounding flips it: Synaps reads its config once, at start, `daemon.json`'s
  `started_at`, so one added since, or too close to tell, is unknown until a restart), or
  its key variable (`ANTHROPIC_API_KEY`, `GROQ_API_KEY`, …) in her environment or the daemon's
  (Linux: `/proc/<pid>/environ`, whether it's set, never its value). Keys and types only, never a
  secret. None: her tag reads `NO BRAIN YET` in place of `ONLINE` (and asleep), and she reads again
  every 5 s, so a `synaps login` in a terminal clears it without a message. With no brain her summon
  key, a click on her or on her tag open the Line on Settings ▸ Brain (Esc: chat), and her menu's
  SETTINGS reads `SET UP MY BRAIN`. A remote broker (`auth.remote_endpoint`, `SYNAPS_AUTH_ENDPOINT`),
  a local model (`local/…`: no login; the Brain screen looks at its server), a provider Synaps
  doesn't know or an unreadable file is unknown: the daemon's first error says it. A read that can't tell any more takes back the tag a
  read put up (a remote broker set up instead of a login, #76). The daemon's own no-brain error
  holds it while the files read as they did when it spoke, until a turn answers or a read finds the
  login. A file caught mid-write changes neither: she reads again. Any sign of a working brain (a
  word, a thought, a tool call, her model's token count; not a subagent's on another model) clears
  it whatever the files say, and no read puts it back until her model or the daemon process
  changes: only the daemon's own error does. A daemon reached through `--socket` is never read here.

  The provider is read off the error (`No Anthropic credentials`: Claude; `no credential configured
  for 'kimi-code'`: Kimi); `synaps login` picks a provider, not a model. The other way out is
  `[session] model` (applied on `avatarctl settings reload`, or MODEL on Settings ▸ Brain); not
  `/model`, which the daemon doesn't take (the Line would send it to the model as words). Anything
  else: `‹error: …›` (cut at 90 characters), annoyed, as before.
- **Settings ▸ Brain.** Settings' first row, BRAIN, shows her model and its login (`✓ CLAUDE ·
  SONNET 4.6`, `● NO BRAIN YET` with a red `SET UP ▸`); ⏎ (or its button) opens the Brain screen:
  SIGN IN WITH CLAUDE, RUN LOCAL (its server looked at: `provider.local.url`'s last line, else the
  daemon's `LOCAL_ENDPOINT` (Windows: hers), else Ollama's port; one deadline for the lookup, the
  connect and the reply), OTHER
  (opened in place: ChatGPT, Copilot, Gemini, Grok, Kimi, Muse, then API KEY; Gemini only with
  `SYNAPS_GOOGLE_GEMINI_CLIENT_ID` set, which Synaps 0.12 needs to sign in to it, or a Gemini login
  on file), and, once she has a brain, MODEL: her provider's few models, written to `settings.toml`
  as an explicit `[session] model` (only that key; the safe writer) and set on her session at once,
  even when the file has it already (his TUI's `/model` moves her session off it: the daemon's
  `setting_changed` to every client, which she follows). If another client owns her session's input
  it isn't set: she says so, and her next start sets it. A Set whose reply doesn't come within 10 s
  counts as lost: MODEL shows her session's model again. A provider with a login on file shows ✓
  and a click switches her to its first model; a sign-in copies its `synaps login --provider …` for
  a terminal until she runs it herself (the keys under the list say what ⏎ does on the row picked).
  API KEY says `IN USE · GROQ` when her brain is a key service with its key, else `KEY SET · …` for
  the services with a key here. Locked: `--model` (`set by --model`), `--session` (`their
  session`); nothing else writes MODEL then either. A daemon through `--socket` signs in on its own
  machine: the ways in (RUN LOCAL too) say ON ITS MACHINE and do nothing here. The open Brain
  screen looks again every 5 s and when a read finds her login, so a sign-in made in a terminal
  (to any provider) shows without reopening it; the row picked stays on its provider when that
  adds or drops a row. Her menu's last row and ABOUT name her model the same way (`SYNAPS 0.12.0 ·
  SONNET 4.6`).
- **A turn shows as started** (THINKING, the busy bubble with ABORT) only once it has run 400 ms with
  no word, or its first word comes: one that fails at once never promises a reply.
- **Sleep follows you, not the chat**: while you're at the desk she stays up,
  however long the session is quiet. She naps 10 min after your last input (5 min at night, 22:00 to
  06:00 local), and at once when the screen locks or the machine sleeps (logind's `LockedHint` and
  `PrepareForSleep`). Late at night her energy runs low (`src/life.rs`: a curve over the local hour,
  tiredness from hours awake, a pat lifts it) and she dozes off for 4 to 8 min now and then even while
  you're there; news from the session wakes her. Back after an unlock, a resume or 30 min away she
  greets you once (`brain.greet`, the first of a morning `brain.greetMorning`), at most 3 a day, never
  under DND or behind a fullscreen window. `[brain] enabled = false` stops her moves, not this. Only
  without a brain at all (`SA_NO_BRAIN`, tests) the old rule holds: sleepy after `--sleepy-after`
  minutes without a turn. On Hyprland there's no idle time yet (ext-idle-notify isn't
  wired): your presence is the pointer moving, so typing alone for 5 min reads as away.
- **Her energy shows**: its band (high from 0.65, low under 0.35) picks her idle activities (a
  gesture that changes her outline and a face beat take turns, each from its own shuffled round) and
  their gaps (`energy.high|mid|low` in moods.json: 1.2-3 / 1.5-3.5 / 2-4 s), her breath (`bobPx`,
  `bobPeriodMs`, `swayPx`: a quick 3-4 px bounce about once a second by day, a slow deep one at night),
  her own roams' pace (×1.2 / 1 / 0.5), her sparkles (`sparkleEveryMs`: fewer at night), and at low
  energy her idle face and stance (`energy.low.idle`: the sleepy face, slow heavy blinks; `droopPx`,
  `leanPx`: standing, her head hangs and leans a few px; `nodPx`, `nodPeriodMs`: a slow nod; all
  transforms of the drawing, no new art). By day her beats mostly change her outline: hand on hip, arms
  crossed, points, a shrug, a chin rest, and two transforms an activity can ask for, `@hop` (a small
  hop in place, a squash as she lands) and `@lean` (a weight shift for the beat's length). The
  CapriCore stand stays home. At night she sits on the floor most of the time, in chains of two long
  sits joined by a hunched crouch, standing only a few seconds between them; her ONLINE tag is dimmed.
  A pool variant with `energy` (the lively lachy idle drawings: mid and high) shifts out when the band
  leaves it. Falling asleep and waking are one light glitch on the swap frame (the tag whole), straight
  into the drawn sleep (the drawing before holds while it decodes); the drawn wake keeps only its clean
  last cell, and the sleep loop skips the cells with green scan lines drawn in (`glitchCells`). Up
  after a nap of 20 min or more on the floor she gets up through CapriCore's floor sit and crouch (the
  curl holds a frame or two while it goes underneath), then stretches (`brain.stretch`,
  `stretchAfterMs`, at most 4 an hour). With a greeting due she gets up quicker (`brain.napWakeGreet`)
  and waves instead of stretching.
- **The cursor game** (her pointer watcher's samples, no other reads): the pointer still beside her
  for 2.5 s (within 150 px, not on her) makes her curious: a look (`brain.curious`, held 3 s) and
  her upper body bent toward it, feet planted: the torso curves from the waist (nothing at the hips)
  and her head and hair go along in one piece, 24 px at her neck, eased in over 0.6 s (the hair above
  her brow trails 0.14 s behind and catches up), held while it stays, eased back over 0.9 s when it
  moves on (every 20 s at most, 30 an hour). Her eyes lead: the pointer coming near already gets a
  look. No idle beat while she watches it (4 s after a look, a
  linger's whole bend and 1.5 s after), and a look cuts an idle beat short. A quick left-right
  wiggle near her (2 turns of 30 px within 0.6 s) gets a point at it (`brain.bat`, every 20 s at
  most); while it goes on she re-aims (turning if it crosses her, pointing again, her bend stepping
  7/13 px with the turns), and 0.45 s after it
  stops she smirks and settles. When the pointer goes still right after a wiggle (0.6 s, or now and
  then after 2.5 s) she pounces: a 0.3 s crouch, then a 0.3-0.5 s leap with an 80 px arc toward it
  (30-450 px) that lands with her body 40-80 px beside its tip, level with it, never on it (a window
  top only that close, not his; else the floor), a squash, then she turns to it and points
  (`brain.missed`); once in 2 min at most, never while you type, a hop in the hourly cap. None of it under DND, fullscreen, with
  a bubble, the Line, a prompt, the menu or a drag up, asleep, with a summon waiting, in a visit or a
  sulk.
- **A sulk** turns her back to your window for the sulk and 2 min after it (the cold shoulder:
  `brain.cold`), her tag reading `brain.sulkStatus` (SULKING) instead of ONLINE; a pat in that time
  makes up (`brain.makeUp`: embarrassed, happy, heart hands; she gets up first if she sat down to
  sulk, starting under the pat's hearts), and she stays put.
- **Where she goes**: a floor spot clear of windows' content (empty desktop, below a title bar) beats
  one over it, and over a window its edges beat the middle of the text; perches, hops and teleports
  rank their spots the same way, by her whole outline (her hair's reach included). Sitting on a window
  top, her legs over that window are the pose, not a cost. On a tiled desk that's a preference, not a
  rule.
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
| `[brain] arrivals` | hop 4, teleport 2, drop_in 2, glitch 1 | how she gets there |
| `[brain] no_repeat` | `2` | her approach, arrival and emote each avoid her last N picks (while another is left) |
| `[brain] passive_every_min` | `[0.75, 3]` | minutes between her own roams (small hops in between) |
| `[brain] bid_every_min` | `[8, 15]` | minutes between attention bids |
| `[brain] max_moves_per_hour` | `45` | her own moves per hour, at most (a small hop counts a quarter) |
| `[brain] cover_max_pct` | `12` | how much of a window she may cover when she picks a spot (0..50 %) |
| `[brain] fullscreen` | `"hide"` | `"hide"` while a window is fullscreen on her monitor, or `"stay"` (and never move) |
| `[brain] monitor` | `"follow"` | `"follow"` the monitor you work on, or `"any"` |
| `[ragdoll] strength` | `0.5` | 0 = rigid, 1 = the full-strength chain (bend, squash, shake, bid swing) |
| `[mirror] drag` | `true` | turn while dragged and flung (`[drag] mirror` is an alias) |
| `[mirror] drag_facing` | `"back"` | look `"back"` where she's pulled from, or `"forward"` |
| `[mirror] self` | `true` | her own turns: travel, what she cares about, an idle look-around |
| `[mirror] idle_every_min` | `[1, 4]` | minutes between idle look-arounds |
| `[mirror] turn_ms` | `150` | the turn's squeeze (0 = an instant flip, at most 400) |
| `[intro] cinematic` | `true` | the launch: her close-up over the desktop first (only with a pack built with `--mochi`), and the bolt back onto the window she sat on last time. `false`: the launch as before |
| `[intro] cinematic_min_gap_min` | `10` | no close-up if she was running less than this many minutes ago (a crash or `systemctl restart` isn't a greeting) |
| `[intro] cinematic_when` | `"every"` | `"every"` launch, or `"daily"`: the close-up on the first launch of the day only (`state.json` `introPlayedUnix`) |
| `[hud] glitch` | `true` | her bubble's and tag's frames glitch as they open, close and change (bands, an RGB split, a red tear; never their words); `false`: no glitch (reduced motion) |
| `[hud] motion` | `true` | sizes ease: the bubble growing or shrinking to its reply, opening from her and closing into the pill, the tag's width; `false`: they change at once |
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
| `[voice] speed` | `1.2987` (1 / 0.77, her pace) | 0.5..2 |
| `[voice] duck` | `"duck"` | other audio while she listens or speaks: `"duck"` (PipeWire, −18 dB), `"pause"` (MPRIS) or `"off"` |
| `[voice] vocab` | `[]` | your own words for speech-to-text (names, machines, jargon), after her generic prompt; up to 16 of ≤ 32 characters, 200 in all (past that, the first that fit are kept and the rest logged; a word already in her prompt still takes a slot); no empty entries, commas or control characters; they count for the prompt-echo filter too, so no everyday words |
| `[voice] key` | `"c"` | the talk key's letter, so an X auto-repeat isn't taken for a release. Match your binding |
| `[voice] wake_word` | `true` | "Hey Capri" starts a hands-free conversation. While on (with voice on) one mic stream stays open, shown on her tag. The rest of `[voice]` (toggle, hands-free, echo, pace): [VOICE.md](VOICE.md) |
| `[voice] hold_to_talk` | `false` | Windows: her summon key (Win+Alt+C) talks while held; a tap opens the Line either way. Linux: unused (the binding sends `talk press` / `talk release`) |
| `[voice] mic` | `""` | Windows: the microphone the sidecar listens on, by its **WASAPI name**, `"Microphone (USB Audio Device Pro)"`: not the two lines Sound settings shows, but the first with the second in brackets. Her installer's Voice page lists them and writes it; Device Manager ("Audio inputs and outputs") shows the same names, and [VOICE.md](VOICE.md#windows-which-mic) has a PowerShell line for them. Empty: Windows' default. The device with that exact name under WASAPI. The sidecar gets it when it starts, from the settings she runs with (a typo elsewhere in the file keeps the last good one), and a new one (`settings reload`) starts the sidecar again on it. A named one that's gone is said (NO MIC), never swapped for the default; one that connects later is found the next time she listens. No control characters, at most 256 characters. Linux: unused |

`[session]` applies only to her own session, never to one she rides with `--session`.

## Flags

```
synaps-avatar [--profile P | --socket PATH] [--session NAME|ID | --new [--cwd DIR] [--model M]] [--no-session]
              [--sleepy-after MIN] [--pack DIR] [--height PX] [--name NAME] [--threads N] [--no-verify]
              [--focus-probe] [--check [--dump DIR]]
```

| flag | |
|---|---|
| `--profile P` | daemon profile (default `$SYNAPS_PROFILE`, else the default daemon) |
| `--socket PATH` | this daemon socket instead (e.g. a forwarded remote daemon). Excludes `--profile` |
| `--session S` | ride this live session (name or id; never `subconscious`) |
| `--new` | create a new session (`--cwd`: absolute, on the daemon's machine; `--model`) and follow it |
| `--no-session` | the body alone (dev control only) |
| `--sleepy-after MIN` | under `SA_NO_BRAIN` only: sleepy after this many minutes without a turn (default 10). Otherwise sleep follows you (see above) |
| `--pack DIR` | character pack (default `$XDG_DATA_HOME/synaps-avatar/packs/capri`) |
| `--height PX` | standing body height in logical px (default 340) |
| `--check` | load and decode the pack, print a summary, exit (no window) |
| `--dump DIR` | with `--check`: every decoded frame as a PNG, the pixels she draws at `--height` (scale it by the display's sf to see the real render) |
| `-V`, `--version` | print `synaps-avatar <version>`, exit. Her log says it at start too: `--- synaps-avatar <version> (pid N)` |

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
| `icon.png` | her icon (`_NET_WM_ICON` on her window and the Line), unless yours is at `$XDG_CONFIG_HOME/synaps-avatar/icon.png`; with neither, capri-syn's (`assets/icon`). Windows doesn't use it: her icon there is the app's |
| `music/*.png` | headphones frames and note sprites for the music idle |
| `fonts/*.ttf` | the HUD font (Exo 2 static instances; OFL) |
| `moods.json` | **the avatar's behaviour sidecar**, our file: HUD tones (overrides of the theme's tokens by name: `accent`, `text`, `danger`, …; empty for Capri), each mood's nameplate shape (`plate`: slant / arrow / flag / square / split), idle life (bob, sway, sparkles, activity timing), the mood table (frames, entry clips, labels, glyphs), idle activities, extra frames (an extra frame may paint another file's face onto it: `patch`), click reactions (head pat / body / annoyed streak), perch rules (one frame, plus the headpat face), music, arrivals (`arrive`: launch / cinematic (the landing after the close-up) / summon / teleport / perch sequences, `teleportChance`), the deep nap (`brain.napDeep`, `napPerch`, `napDeepAfterMs`, `napWake`), the greeting when he's back (`brain.greet`, `greetMorning`), the startle and big-leap crouch, the look straight ahead (`brain.lookFront`), the rare air kiss (`kiss`) and the icon. The code ships no character ids: everything character-specific is in here |
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

Optional desktop art, into the pack's `anim/` (a pack without it loads as before):

- `--lachy DIR|auto`: [lachydotmcg/capri-desktop](https://github.com/lachydotmcg/capri-desktop) at `156ed18`
  (PolyForm Noncommercial 1.0.0): its sprite strips, stills and emotes, checked against
  `tools/lachy-desktop-156ed18.sha256`. `auto` downloads exactly those files at that commit (cached).
- `--mochi DIR|auto`: mochiiwuff/capri-desktop at `c011764`, checked against `tools/mochi-desktop-c011764.sha256`.
  Its images are show frames and fan art that its licence does not cover: local personal use only, never
  published. Only this flag puts them in a pack (into your local pack directory); nothing in this repo
  ships or fetches them otherwise. They give her the launch close-up (below).
- `--mochi-hair red|flat|original` (default `red`): those close-ups draw her hair black; make-pack repaints it
  as it copies them (`tools/mochi_hair.py`: a measured head region minus her face, times the black ink;
  arms, hands, body and the linework stay as drawn). `red` is her avatar's red with its shadow red and a
  starfield, `flat` one red, `original` leaves them untouched. Deterministic; the recoloured frames are
  lossless WebP and `anim.json` pins their own sha256 (with the source's as `sourceSha256`).
- lachy's stills, when the pack has them: her portrait beside her rows in the Line (`face`), `capri.png` over
  the right-click menu and as the image of the one notification she sends (NEEDS YOU while a fullscreen window
  hides her; org.freedesktop.Notifications), and its cursor while the pointer is over her head (X11).
- `anim/anim.json` is `tools/desktop-art.json` (capri-syn's own measured cell grid, anchors and loop flags)
  plus the sha256 of every copied file. `NOTICE.md` gains each source's `Required Notice:` lines, the licence
  URL and its credits.
- `--upscale` adds `anim/lachy/x2/`: every strip at 2x (`tools/upscale_art.py`, Real-ESRGAN's anime video
  model on the CPU; needs `torch`), checked pixel-for-pixel against `tools/lachy-x2.sha256`
  (`--record-upscale` writes that file once). Build her live pack with it: at the 340 px default on a
  1.167 display lachy's standing strips draw at 1.16x and their edges go soft (alpha ramp 1.8 px against
  CapriCore's 1.25-1.3); from the x2 strips they come out at 1.3 (perch and laydown at 0.95). The app uses
  `x2/` when present and her pixels are above 1:1, else the strips as shipped. About 24 min of CPU
  (recorded on an i5-13420H, 4 threads) and +220 MB on disk.

With `anim/`, `moods.json` `variants` mixes that art in; she never swaps one set for the other:

- `variants.pools` has a pool per state with more than one drawing: `perch` (sitting on a window top),
  `sleepFloor`, `sleepWindow`. Each variant has an `id` and a `weight`; without `strip` it is the pack's own
  art for that state, as before. With one it plays that strip's loop, anchored like the pack's frame
  (seat, feet, surface), mirrored with the strip's `mirrorArt` twin when she faces the other way, with its
  own `talk` and `music` loops and its `blinks` (weighted, every `blinkMs`). `lying` variants are picked
  only where the figure fits on the window's exposed top. `every: n` plays every n-th cell. `afterMs`: only
  once she has been on that window that long; `stayMs` [min, max]: picked, it lasts that long, then the
  pool shifts on. Capri's `laydown` is the "getting comfy" beat: on a window she has sat on 4 min or more,
  she lies down along its top for 1-3 min, then sits again; asleep there meanwhile, she sleeps in the same
  lying drawing (`sleepWindow`'s `laydown`, weight 0: it only follows the lounge). A shift in place cuts a
  frame late: the new drawing goes underneath first, so its first frame is whole. Its one light seam is on
  the cut frame itself (small shifts, no bars, nothing after it); the tag sits out the held frame and
  comes back whole at its new place. A strip that lands after its pick cuts the same way. Without a
  compositor every cut of her frames gets a render a frame ahead with the next drawing underneath, so
  a cut's new outline never shows pixels the window hasn't drawn (r43: stale bits of the SLEEPING tag
  and dashed edges at a wave's cuts).
- Entering the state picks one: weighted, never the last pick again, seeded (`SA_ART_SEED`). A long stay
  (`shiftMs`) shifts it in place. Every cut between CapriCore and a strip (or between two strip families)
  goes through a light seam (one light step on the cut frame; her tag stays whole). Every pose or
  family change that isn't a teleport gets that seam (`fx::Transition`: a mood change, music, a sleep
  or wake, a drop onto a window, a startle); the heavy glitch is for teleports and a summon (into or
  out of NEEDS YOU) only.
- Strips decode when first wanted, off the main thread; `cacheMb` caps what is kept (least recently shown
  first, never what is on screen), and one out of use for 45 s goes (a dance, a dangle, a walk are back
  to disk a minute after). Pixel buffers live in maps of their own and go back to the system when freed.
  A pack without `anim/` keeps only the pack's own variants.
- `stand` (standing idle on the floor, the Idle mood only: every other mood keeps the pack's poses) shifts
  in place every `shiftMs`; with `shiftStays` a shift may keep the pack's own pose, so each variant holds
  about its weight's share of the time (Capri: 6 : 1 : 1, CapriCore ~75 %). A drawn arrival or wake ends
  in its `bridge` variant's drawing, held `bridgeMs` before the next shift.
- `transitions` names drawn in-betweens by change (one-shots with the anchor the change needs):
  `perchLeave` (off a lachy perch into the bolt), `floorLeave` / `floorArrive` (a loud teleport from / onto
  the floor while the stand pick is lachy's; the arrival is the materialisation, no landing glitch over
  it, its first frames the plain red silhouette), `wake` (up from a lachy floor sleep) and `form` (the
  bloom a drawn floor arrival starts from, `formChance` of the time; 3/4 of her size, cut to her window).
  The travel bolt stays plain red. A quiet (passive) teleport never plays them.
- Landing on a window top she touches down with the pack's `arrive.perch` (Capri: her crouch on the
  edge), then her sit inside a dip at contact: the squash starts `arrive.perchDipMs` before the change,
  so the swap falls near its trough (no dissolve, no double image), and the sit's legs unfold down over
  the edge in 170 ms: 40 % of them already hang over on the cut frame, the rest come in 2 px row by row
  where they are drawn (nearest the seat first), never a straight cut across the shins, and nothing
  moves, so her shape only grows. All of the sit is painted underneath from two frames before the cut,
  so a row that comes in shows her legs at once (no dark copy of them, no streak under her feet). Into a
  drawn pose (lachy's perch) she forms as that pose: from the moment the bolt reaches the edge, its own
  red silhouette (the seated drawing's alpha, legs over the edge, painted a frame before it shows),
  settling on contact (a squash of that silhouette about the seat), then the drawing on the cut frame,
  so the outline never jumps and the edge is never bare. The landing glitch never shifts her legs below the edge, and a drawn pose
  lands as itself on the cut frame: no tint, tears or split after it.
- Sparkles: one at a time on hair only, every `life.sparkleEveryMs` [min, max] (Capri: 1.5-1.8 s, an even
  beat); never smaller than 40 % of full size (10-13 px), so none vanish into her hair's own glitter. Every
  point a grown one reaches over its life is on her hair (8-9.5 px arms), wherever on it (a new spot each
  time). It is a solid near-white 3 px core with four arms, 3 px across where they leave it and tapering to
  1 px points, pink on their outer half: a hot centre fading into her hair. One that couldn't show yet
  starts its life when it does (it swells from nothing). When one can't show (a glitch, a turn, a frame
  without hair) she tries again 300 ms later.
- `cutins` (local images, `--mochi` only; a beat without its image, or a pack without them, shows
  nothing): a show still beside her for a big moment, `ms` long (Capri 1.2 s): `needsYou` (NEEDS YOU,
  600 ms after it; a summon landing in that time shows `summon` instead), `done` (a turn of at least
  `doneAfterMs`), `laugh` (`laughChance` of her laughs at a click), `summon` (a summon for his prompt
  landed), `annoyed` (an error or abort). At most one per `gapMs`. It comes in torn in, a third of its
  rows on its first frame and all of them by 200 ms, each in place (the still itself from frame one, no
  flat plate, nothing past its edges), holds, and leaves row by row, its last rows together (no line left
  behind). It goes beside her, 14 px clear of her outline as drawn (her hair included),
  where it covers the least of his windows (either side, level with her head or a step up or down; else
  over her head), among equals her roomier side; `height` px tall, a plate cut and rimmed in her red;
  clicks pass through it. Never while the Line or the reader is open, a fullscreen window hides her, DND
  is on, or the intro plays; she moves off and it leaves at once. The window and its pixels go with it.
- `lineFaces` (mood → `anim/mochi` image, local only, with the square of it that is her face: `centre`,
  `size` in source px): her face on her latest row in the Line follows her mood (thinking, working, NEEDS
  YOU, done, annoyed, sleepy...), the square reduced by a whole number to the tile (sharp, no resampling
  blur) in a 1 px border of her red; older rows keep the portrait. A mood without one keeps the portrait.
  Capri's squares take her eyes and mouth with ~15 % around them, inside the image (never wrapped), one
  supplied frame per mood where the art has one (sleepy and away share the drowsy one).
- `motions` (each optional, each its strip from `anim/` with the anchor it needs; a pack without them
  does what it did before):
  - `walk` (feet, looping): a short calm move she makes on the floor or along the window top she sits on
    is walked `chance` of the time instead of hopped (`minPx` up to the hop's reach, `pxPerS`, at most one
    per `gapMs`, `edges` for window tops). Every step of the way keeps out of his window and clear of the
    pointer, in her work area, and on a window top only where nothing covers it. A summon, a panel, the
    menu, a fullscreen window or a hide stops it where she is. She sits back in the same drawing. On the
    floor she leans back 170 ms before setting off and her momentum settles forward 260 ms after she stops
    (her feet stay put; she stops facing the way she walked).
  - `dance` (feet, looping): music with energy (a tempo at or over `minBpm`; without one, a `chance`
    roll) gets a dance on the floor for `ms` [min, max], once a session and at most once per `gapMs`;
    else the headphones as before. With a tempo it takes two beats a loop. Only standing idle on the
    floor, no panel open, no DND. It plays the strip's clean cells only (`glitchCells` in the art's
    `anim.json`: cells with a chroma ghost drawn in), so nothing glitches inside the loop.
  - `dangle` (grip): carried by the head, `chance` of the time (at most once per `gapMs`) she hangs as
    that drawing from its grip after `afterMs`, instead of the ragdoll, on its clean cells only. A cut
    into or out of any motion's drawing is a light seam (one light step on the cut frame: a few tall
    bands shifted a few px inside her body, no colour split, bars or scanlines, the tag left whole),
    never the burst; and the
    drawing before a cut goes under the first frame after it, so the screen never shows the old
    drawing clipped to the new outline.
  - `click` (an effect strip): a click on her bursts under the pointer, at most one per `gapMs`, drawn
    as three server-painted colour layers (no alpha without a compositor): her red, the green, and a
    light inner edge on the ring so it reads over her red. Its specks (pieces under 40 cell px) are cut
    to her body; the ring shows whole.

The launch, with `[intro] cinematic` on:

- If the pack has the close-ups (`anim/mochi/intro-*`, `--mochi` only), her close-up rises from below the
  bottom of her monitor while a glitch builds it (62 % of its height, centred on the work area) and plays
  its 2.7 s beat: grab, a whip into a wave, "hi", a grin, then out: it sinks while its rows drop out, inside
  its own pixels; it holds its last rows until the bolt is on screen, then goes. A click on it
  or Esc anywhere skips to the out beat. Clicks anywhere else go through to the window under it, and Esc
  reaches your window too.
- It doesn't play while a fullscreen window is focused, within `cinematic_min_gap_min` of her last run
  (`state.json` `seenUnix`, written every minute and at a quit), or (`cinematic_when = "daily"`) if it
  already played today.
- Then the plain red bolt from the top of the monitor, onto the floor or onto the window she sat on last
  time (`state.json` `perch`: the same window, same size, same place, with room on its top; else the
  floor). On the floor she lands once (`arrive.cinematic`; the close-up was the greeting) and is idle
  ~3.1 s after the close-up's first frame. After any landing her idle life starts at once: a sparkle, a
  breath step and a blink within half a second. A launch without the close-up lands with `arrive.launch`.
- Without the close-ups, the launch is just that bolt. The close-ups are decoded for each launch and
  dropped after it (their memory back to the system); ones that decode after the launch went on are
  not kept.

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
music on [BPM]|off | walk <x> [scoot] | glitch | face left|right|toggle | drag … | submit <text> | abort | copy
art stand|perch|sleepFloor|sleepWindow <variant id|next> | intro [skip]
talk press|release|tap|toggle | line close | prompt confirm|secret [text] | prompt off
brain … | settings reload|open [brain]|close | tap head|body | menu | shape | state | hide | show | quit
```

`brain` takes `on|off|state|dnd [min]|dnd off|scale <x>|seed <n>`, `force roam|seek|summon|sulk|nap`,
`event prompt [id]|done|error|abort|cleared`, `session lock|unlock|sleep|resume` (what logind would say)
and `sense …` (focus, cursor, idle, fullscreen, near, arena, windows, `clock HH:MM` (her local time,
running on the brain's clock), each with `real` to go back to the real senses). Under `SA_BRAIN_SYNTH`
she has no time of day until `sense clock` sets one, and logind isn't read.

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
- `tools/fakevoice.py SOCK`: a fake `capri-voice` sidecar (no audio, no mic, no models) that prints every
  op it gets; run the avatar with `CAPRI_VOICE_SOCK=SOCK` to see what she would say.
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
