# Changelog

## Unreleased

### Windows: her icon in the tray
- Her icon sits in the notification area while she runs. Its tooltip is `capri-syn · <her status>`
  (ONLINE, SLEEPING, NO BRAIN YET, HIDDEN, ...). It changes when her status does; her mic's clock
  and level meter stay off it, and hands-free's last seconds read ENDING.
- A click opens the Line, as Win+Alt+C does. A right click opens her menu: the same rows as her
  right-click menu, from one model (`src/menu.rs`), with the same greying of what can't act now.
  Hidden, the tray is the way back (SHOW). An ATTENTION pick that didn't save says so under its
  choices (CAN'T SAVE / NOT SAVED YET), as her menu does under its row.
- Explorer restarting puts it back. QUIT takes it out, with no ghost left. An icon the shell won't
  take at sign-in is tried again every 2 s for a minute.
- Linux: no tray yet. A StatusNotifierItem would plug in at `platform::linux::tray`, for X11 and
  Hyprland both.

### capri-syn's app icon
- `assets/icon/`: `capri-syn.ico` (256 to 16 px), 512 and 256 px PNGs, and its `NOTICE`. Her Windows
  exe carries the .ico as its icon (`build.rs`), and her tray loads its size from there.
- Her icon, in order: your own beside settings.toml (`icon.ico` on Windows, `icon.png` on Linux),
  then her pack's `icon` (Linux only), then capri-syn's. Read at start and on `settings reload`
  (`avatarctl`, `capri-ctl` on Windows).

### NO BRAIN YET goes when he sets up a remote broker (#76)
- A read that found no login put NO BRAIN YET on her tag; if he then set up a remote broker
  (`auth.remote_endpoint`, `SYNAPS_AUTH_ENDPOINT`) instead of logging in, the next read couldn't tell
  and stopped, and the tag stayed until his next message answered. A read that can't tell any more now
  takes back what a read said. The daemon's own no-brain error goes too once the files read otherwise
  than when it spoke (he said hi first, then set up the broker; Windows alike).
- Any sign of a working brain clears it at once, whatever the files say: a word, a thought, a tool call
  or the provider's token count (it was the first word only). After that no read puts it back, a new
  attach neither; only the daemon's own error, or a model change, does.

### Her voice: higher (his r26 pick, "R-A")
- The default voice is the same W3 blend, now at `@+7.00`: about 306 Hz median, up from `@+4.93` (~272 Hz).
  The speed (1/0.77), pace ("stretch") and flow ("emphasis") are unchanged, so only the pitch moved. He picked
  it blind from round r26 ("anime girlie, more bubbly"): six recipes plus the old default. Stock Kokoro voices
  and our own DSP only, as before.
- A `voice` set in `settings.toml` is kept; only the default changed. The previous voice, back in one line:
  `[voice] voice = "af_heart:0.5,af_bella:0.3,af_jessica:0.2|af_heart:0.84,af_jessica:0.56,af_sarah:-0.4@+4.93+clean"`.
- Stress on a `*word*` rides closer to its 400 Hz cap: any planned peak over ~267 Hz in the model leaves no
  room, so most pitch bumps are cut short and the stress is mostly length and energy. Expected: raising
  `emph_st` doesn't change it.
- Windows (no rubberband: the shift is in-model, the pace is Kokoro's): the median pitch matches Linux within
  0.1 st. The floor is 2.3 st lower, the sound brighter, and the name/number test set slips more (7.5 % vs 1.7 %
  WER). Numbers in docs/VOICE.md. Bundling rubberband there is a follow-up.
- Fixed: with no flow graph (another model file, or the patch didn't load) and no rubberband (always so on
  Windows), a voice with an `@` shift failed every sentence (`TypeError`: the flow pass was called on None).
  She now speaks plain Kokoro at its own pitch, as docs/VOICE.md says, and the log says once that the shift
  is off. `--selftest` covers it.

### A new personality, written from scratch

Her persona's first paragraph is rewritten clean-room from the show's canon: a construct stitched
together from many human minds, gleeful and fun-seeking, cocky, teasing, with a quiet side that
watches the sky. She's firmly on her user's side, never possessive, and still keeps to one or two
plain-text sentences. The old paragraph closely paraphrased CapriCore's GPL-3.0 `PERSONALITY.md`,
so it's gone. The Synaps and capri-watch paragraphs are unchanged.

### "Hey Capri" is never sent as his words
- "Hey Capri" alone could reach her as a turn he never said: Whisper decoding the phrase again in the first
  hands-free utterance wrote "Take a breath" (five times live since Nov 16), "Aye, capri", "And capri", "Ee capri" (his bare
  takes). The phrase is now cut as Whisper spells it, doubled or misheard as the first word or two; words with
  no trace of it are the phrase misheard, and the audio after the check's window decides (no speech there:
  nothing is sent, hands-free goes on; speech: that audio is decoded on its own).
- Junk, in every mode: subtitle credits (at the line's start) and more stock fillers dropped; `*cough*` tags cut;
  `thank you`/`bye`/`take a breath` count as filler under 300 ms of speech (was 400: a real "bye" was dropped),
  sign-offs (`see you next time`, `thanks for listening`) under 600 ms. Heard/dropped lines carry Whisper's confidence.

### A reply she says whole is read
- A spoken reply heard to its last word counts as read: no unread strip, no `N NEW` (the Line and
  `state`'s `unread` agree: they are the one strip). His spoken turns' replies and events' alike, hands-free
  or not; it was hands-free only, and from her first word.
- Cut off (a tap on her, Esc, music, a new turn, his voice over hers, the sidecar's gate, the sidecar
  going or restarting mid-say) or said in part (past the cap or 60 words, after `---`, code, a table, a
  link) it stays unread. `$mod+c` cuts her too, but opens the Line, so the reply is read there as seen.
  `docs/VOICE.md`, "Heard whole is read".
- The strip counts each missed reply and shows the newest; one heard whole after its bubble went drops
  only itself (it cleared the count's newest, whichever reply that was, and kept its line).
- `capri-voice`: a `say-end` carries `"cut": true` when its player ended early (nonzero exit: killed from
  outside, its sink gone) or the gate trimmed it to the 10 s cap (an event's long first sentence); the
  avatar counts it short. An older sidecar's plain `say-end` is whole. Windows: the player's `wait`
  reports a stream that didn't play all it was given (it always said 0).

### The HUD moves as one system
- One curve (ease-out, 2 % past the target at most, no wobble) and one set of durations, named in
  `src/hud.rs`: a size change 200 ms, the bubble opening 240 ms (180 ms out of the tag), into the tag
  150 ms, a close in place 120 ms, a label swap 120 ms (cut at its middle), a glitch 2 frames.
- **Never an empty shell, never faint or doubled words.** A plate or bubble stays opaque while it
  reshapes; its outline morphs into the new one (shared edges stay straight). Words cut, never fade
  through each other: on one frame the old go and the new are there, readable (60 % or more, whole a
  frame later). The glitch is a 2-frame RGB split (whole, then half) along the border's steep sides on
  that cut frame, never on text.
- **The tag** (and the pill, and the unread chip): the plate's width eases to the new text from
  wherever it is (it leads a longer label, waits for a shorter one); the label and WORKING's red block
  cut together, every change with its split. A label shows at least 350 ms; changes meanwhile wait, and
  only the latest shows (no strobe). Its place is pinned on whole pixels while she stays put.
- **The bubble** grows out of whatever tag is showing (its label at the centre until the shell covers
  the reply, which cuts in then), or from her (90 % at her tail) when none is; grows and shrinks to its
  reply, a new line showing only once there is room for it. A new reply cuts in whole at once
  (streaming text never glitches). It goes back into the tag that will show (closing or collapsing to
  the pill): its words on its first frame, the tag's label from the next as its box eases in.
- `[hud] glitch` and `[hud] motion` (both default on). Alpha mode only: under the X shape (no
  compositor) the HUD changes at once, as before. The Line and the menu don't glitch.
- `SA_HUD_PROF=1` logs frame intervals and main-thread frame times while the HUD moves.

### One installer: `install.sh` (Linux), `install.ps1` (Windows)
- `curl -fsSL …/install.sh | bash`, `irm …/install.ps1 | iex`, or either from a checkout. Seven steps, the
  same on both, each one checks first, so a rerun is an upgrade:
  1. Synaps, when it's missing or older than 0.11.0, through SynapsCLI's own release installer
     (sha256-checked);
  2. her binary (a release asset, else a source build: rustup, the pinned toolchain, the distro's
     packages printed and asked for, never a silent sudo);
  3. her art pack (CapriCore v1.0.4 + lachy through `tools/make-pack`, in a venv of its own;
     lachy's Required Notice shown);
  4. the Synaps daemon at login and her session;
  5. the summon key (i3/sway/Hyprland, with consent, backed up, between markers; Windows:
     Win+Alt+C);
  6. autostart (default on);
  7. `--voice` / `--watch`.
- Flags: `--yes`, `--dry-run`, `--uninstall [--purge]`, `--voice`, `--watch`, `--no-autostart`,
  `--from-source`, `--prefix`, `--with-local-art` (mochi, local personal use only), `--artifact`.
  Pins in `installer/manifest`.
- `--uninstall` walks a receipt of everything it added, and leaves Synaps, settings and state.
- `.github/workflows/release.yml`: a `v*` tag builds the Linux tarball and the two Windows
  installers, each with a `.sha256`, smoke-tests them through the installers, and attaches them to
  a draft release. Nothing is released until a tag is pushed.
- `docs/INSTALL.md`; CI: shellcheck, PSScriptAnalyzer, the two scripts kept in step, dry runs, and
  `install.ps1` end to end on windows-latest.
- `tools/make-pack` copies `persona.md` into the pack. `capri-voice-setup` takes PyPI's
  pywhispercpp wheel where the distro has none.

### Synaps, when they ask or need it (funnel 1)
- **Her persona is everyone's.** `packs/capri/persona.md` no longer names anyone or any machine ("your
  user", they/them), and it says what runs her only when asked: one plain sentence, no pitch, no links;
  for a job too big for a bubble she mentions once that `synaps --attach capri` opens the same
  conversation in a terminal. A personal persona goes in `[session] persona_file`.
- **No brain yet, in her words.** A turn that fails for want of model credentials shows "No brain yet,
  choom. Synaps has no Claude login. Log in, or set another model ([session] model in my settings):" and `$ synaps login` as a
  copyable chip (tag `NO BRAIN YET`, calm) instead of the daemon's text cut at 90 characters. A lapsed
  login, a rate limit, an overloaded provider and an unreachable model get their own lines; the
  Line's footer adds `↑ resends your message`. Anything else is shown raw, as before. A turn that
  fails at once never shows THINKING or ABORT first (a turn shows as started after 400 ms or its first word).
- **Her first words**, once in her life: "Hey. I'm Capri. Tap $MOD+C when you want me. Headpats also
  accepted." Nothing about what runs her. Not for anyone who ran her before (`state.json` `met`).
- **Her maker's mark**: one dim row under QUIT, `SYNAPS 0.12.0 · CLAUDE-OPUS-5`. Not a button.
- The right-click menu stays in her work area and keeps off her face, placed once before it shows; if
  her pose then puts her face under it, it moves once, across her face (never off-screen). The Line's
  footer wraps.
- **Fixed: the Line put her answer above his question** after a failed turn (a refold kept the error
  note ahead of the turn's prompt; `synaps-client`'s `Transcript` anchors it to its turn now).

### The talk key's fine print (Linux and Windows)
- A `talk tap` no longer waits out a press that lost its `talk release`. A tap is ignored only while a hold
  is his: the press is under 1 s old (its mic may still be opening), or its mic is asked for or open (up
  to 120 s, the sidecar's hold cap). Any other unreleased press (voice off, no sidecar, a binding that
  never sent `talk release`) is cleared by the tap, which then opens or closes the Line as usual. Before, the
  tap was ignored for 120 s.
- Unchanged: a press or a tap within 60 ms of a release is taken for an X auto-repeat (one key-down);
  a `talk press` while one waits for its release still waits out 120 s.
- `avatarctl state` → `line.waiting` (`false` unless the Line opened without the keyboard and is
  waiting for his key or click; Windows only today), beside `line.parked`.
- The summon key on a parked Line counts as `waiting` only when the desktop refused the keyboard. Any
  other failure (she is gone) is logged as itself (`line: summoned, but she is gone`), not as a wait.

### Windows: she runs her voice, a voice installer, the models, hold-to-talk (not shipped)
- **She starts the sidecar on Windows** while `[voice] enabled` is on: the bundle beside her, in a job
  object that kills it when she ends however she ends, restarted after 1, 2, 4 … 60 s, its log in
  `%LOCALAPPDATA%\capri-voice`. Linux: unchanged (systemd runs it; she starts nothing).
- **Connecting to it on Windows runs on a thread:** a stale endpoint on a silent port no longer
  freezes her for up to ~2 s. Linux connects inline, as before.
- **A voice installer of its own** (`synaps-avatar-voice-setup-….exe`). Her own installer stays ~9 MB.
  Its "models" task fetches Whisper, Kokoro, Silero and the wake word's models (~320 MB, SHA-256
  pinned, resumable) with `voice-models.ps1`.
- **`[voice] hold_to_talk`** (Windows, opt-in): the summon key talks while held. The release is read
  every 25 ms from her key thread, with no keyboard hook. A tap opens the Line as before.
- `tools/test_capri_voice.py` no longer reads his `CAPRI_VOICE_SOCK` / `CAPRI_VOICE_ENDPOINT`, and
  removes its temp folder.

### Known issues
- With `XDG_RUNTIME_DIR` unset, she and the sidecar look for the Linux socket in different places:
  she in the temp dir (`$TMPDIR`, else `/tmp`), the sidecar in `/run/user/<uid>`. Set it, or set
  `CAPRI_VOICE_SOCK` for both. It isn't aligned yet because that would change how she behaves.

### Windows: the voice sidecar's link and its Windows layer (not shipped)
- Her link to `capri-voice` goes through the platform layer. Linux: unchanged (the same Unix socket, the
  same log lines). Windows: loopback TCP after a two-token handshake (`endpoint.json` in
  `%LOCALAPPDATA%\capri-voice`), since stock CPython has no AF_UNIX there. `CAPRI_VOICE_ENDPOINT`
  overrides the file on both ends.
- `capri-voice` runs on Windows through `tools/capri_voice_win.py` (Linux never loads it):
  - PortAudio (WASAPI) capture and playback;
  - SMTC pause, and a session-volume duck with Linux's rules and crash repair;
  - the lock, calls (the mic privacy store) and do-not-disturb.
  On Linux only seams changed in `capri-voice`: every path runs the same commands in the same order.
- `packaging/windows/voice-bundle.ps1` builds a 143 MB folder (CPython 3.13 embeddable + wheels, no
  models) that runs the sidecar with no Python installed. It is not in the installer yet, and she
  doesn't start it yet.
- CI: a `voice` job on Linux and Windows runs the unit tests and the selftest. On Windows it also installs
  every wheel binary-only, builds the bundle, and runs the tests again with the bundled interpreter.

### Windows: an installer, her files in the right places (not shipped)
- A per-user installer (Inno Setup, no admin), built by CI as an artifact only: her windowed exe (no
  console window), her AUMID with an icon and a Start-menu shortcut carrying it, opt-in autostart, a
  clean uninstall. Her pack is never in it: the wizard (or `/PACK=`) copies one from his disk to where
  she looks by default. `docs/WINDOWS.md` → The installer.
- Settings go to `%APPDATA%\synaps-avatar`, packs and state to `%LOCALAPPDATA%\synaps-avatar` (they were
  in `%TEMP%`, and the default pack path was relative). `$XDG_*_HOME` still wins. Linux: unchanged.
- Without a console she logs to `%LOCALAPPDATA%\synaps-avatar\synaps-avatar.log`.
- How voice could come to Windows, options sized and nothing decided: `docs/design/windows-voice.md`.

### Windows: the Line's keyboard, music, toasts, desktops, effects, DPI (not shipped)
- The Line's keyboard follows Windows' rule: her key (Win+Alt+C) or a click on her takes it at once; any
  other opening (a summon, `capri-ctl`, voice) shows the Line without it, and her tag says
  `WIN+ALT+C TO TYPE` until he presses her key or clicks the Line. She never takes the keyboard from his
  window, never flashes, never injects input (GPUI's Alt trick patched out: PW-4).
- Music from the media flyout (SMTC): playing or quiet, and whether he's watching a video.
- Notifications are toasts (her AppUserModelID registered under HKCU; `docs/WINDOWS.md` lists what a
  package must provide).
- Perch targets on another virtual desktop are left out; a desktop switch is noticed at once.
- Effect layers reaching outside her window get windows of their own (click-through, never activated).
- Per-monitor DPI aware (v2); her windows keep one scale on every monitor (PW-5). Monitor, DPI and
  work-area changes are noticed at once.
- Linux: unchanged, except that a `talk tap` while the talk key is held is ignored (it ended the hold
  early), the MPRIS error line says `mpris:` again, and the Line's hint names the summon letter you set.

### Windows: the groundwork (draft, not shipped)
- She builds and runs on Windows 11 (x86_64-pc-windows-msvc): `src/platform/windows`, a Windows CI job
  (build, unit tests, the fake contract suite, clippy). GPUI's Windows crate is vendored with three
  small patches (`vendor/gpui-pre-windows/PATCHES.md`).
- On Windows she registers her own keys: Win+Alt+C is a tap (`talk tap`: the Line, never a hold) and
  Win+Alt+Ctrl+C is toggle-to-talk. No music detection or notifications there yet.
- `capri-ctl talk tap`: a press and its release at once, for a key that only reports its press.

### Her tag shows her state only
- The tag over her head drops her name: `ONLINE`, `SLEEPING`, `THINKING`, `NEEDS YOU`… instead of
  `CAPRI // ONLINE`. The unread pill drops it too (`1 NEW  <first words>  READ ▸`). WORKING keeps its red
  block, with the label on it. Notifications, the Line and the reader keep her name.

### Her tag says ONLINE while the wake word listens
- The idle wake stream no longer replaces her tag with `MIC ON · SAY "HEY CAPRI"`. Her usual tag stays
  (`ONLINE`, her mood, `LISTENING` for music), and while the wake word's mic is open its dot is a mic glyph,
  so the always-open mic is still visible. Live mic, hands-free, heard and speaking tags are unchanged; a
  side effect: `SPEAKING` and `HEARD` now show while the wake word listens (the wake tag used to hide them).

### The new art about half the time
- Window perch: CapriCore's sit and lachy's perch now come up 1 : 1 (was 4 : 1), and the pose shifts in
  place every 2–6 min (was 4–12). Floor sleep: CapriCore's lie and lachy's doze 1 : 1 (was 3 : 1). Window
  sleep stays CapriCore's (laydown is still at weight 0). Weights only, in `packs/capri/moods.json`.

### "Hey Capri" from across the desk
- The check decodes the 2.8 s it was measured and documented with. It had decoded 2.0 s: a byte count
  read as samples.
- The second look (his whole turn, decoded once he pauses) now gets every rejected candidate whose margin
  is over −8 (was −2), and it passes at its own thresholds: the words say "hey capri", `T > −10` and
  `margin > −3` (was `> 0`). With the request after the phrase, Whisper reads far and casual speech much
  better. On his own takes: 29 → 35 of 40 woken (far and casual 1 → 6 of 10), none of his 10 sound-alikes.
- The log gets the second look's two scores (never words), and says when its words held no "hey capri".

### "Hey Capri" starts a conversation
- "Hey Capri" now starts hands-free (`[voice] wake_handsfree`, default true, with `handsfree` on): the
  wake stream goes on as the hands-free one in the same capture (no gap), so "Hey Capri, what's spike
  doing?" is the conversation's first turn (the phrase cut) and "Hey Capri" alone just listens. Every
  hands-free rule holds: each pause sends, spoken replies, half-duplex/barge-in, the end phrases.
- Quiet for `handsfree_timeout` (60 s, counted down on her tag for the last 10), it ends and the wake
  word listens again at once (`◌ MIC ON · SAY "HEY CAPRI"`), as after every other way hands-free ends.
- `wake_handsfree = false` (or `handsfree = false`): one turn per "Hey Capri", as before.

### Asleep, she stays put
- While Sleepy (no turn for `--sleepy-after`) or in the away nap, nothing moves her on her own: no
  roam, explore, micro-hop, bid, focus hop, workspace follow, dodge, startle, look or lean, and her
  drives (attention, the roam clock) stop. A move already due between two polls is held too.
- He can still carry or throw her: she sleeps where she lands (lying if it fits, else sitting; on
  the floor Sleepy's entry plays again, the nap's pose comes back). A drop on his window is never
  "left" for covering it; a throw in her sleep is no offence (no sulk, no far move). A pat, a drag
  or the menu don't wake her; while she naps, input within 10 s of handling her isn't "he is back".
- She wakes for a new turn or a prompt (the mood leaves Sleepy), $mod+c, the Line, the field or
  typing to her, a summon, or him being back after the nap. Waking restarts the session's quiet
  clock, plays the stand-up (`sit_floor-neutral` → `floor_stand`; on a window top a 0.6 s sit-up)
  before any move ($mod+c's teleport to the Line waits for it), then 20 s of grace before her own
  moves resume.
- Still allowed asleep: her window closing (rescue or fall), her perch leaving with the workspace,
  riding a moved window, falling off a covered one.

### Voice: toggle-to-talk
- `capri-ctl talk toggle` (proposed i3: `$mod+Ctrl+c`): one tap opens the mic, his pause sends it
  (`[voice] silence_s`, 1.0 s; ×1.5 while he has said under 1 s). A second tap sends now (cancels before he
  spoke); Esc or `talk cancel` cancel; no speech in 8 s closes it; 60 s sends. Holding the chord is one
  toggle. Tag `● MIC 0:03 ▮▮▯ · AUTO`.
- The mic refuses on a secret (not an approval), the lock, and for a toggle her hidden; a secret appearing
  or the lock closes any open mic, hiding a toggled one. Hard caps in the sidecar: hold 120 s, toggle 65 s.
  A test sidecar (`CAPRI_VOICE_TEST`) refuses a real or unset source.
- `pw-record --latency 20ms` (was ~100 ms chunks).

### Voice: duck, don't pause; no more junk from near-silence
- `[voice] duck = "duck"` is the default: browser video and music go down −18 dB (raw ×0.5 per channel)
  while he talks and while she answers, then back to exactly their old per-channel levels. Never her own
  streams, module loopbacks, calls (role or `[music] ignore`), notification sounds; nothing at all while
  another app captures a mic. Streams that start or unpause mid-duck are ducked too; his own change wins;
  same stream only (`object.serial`); WirePlumber's per-app restore of a ducked level is undone (for a stream that comes back, and for a
  sibling stream of an app still ducked); a crash
  mid-duck is repaired at the next start (state written atomically).
- Silero VAD v5.1.2 gates Whisper: at least 6 speech frames (192 ms) peaking at p ≥ 0.70, else nothing is
  transcribed or sent (DIDN'T CATCH THAT). The clip is trimmed to the speech (+400 ms / +200 ms).
- Junk transcripts are dropped: no letters (`-2,` `,` `2.`), ≤ 2 letters unless a real short word (`nd,`
  `Q.`), one vowel-less word (`, chr,` `hmm`), stock hallucinations, the prompt echoed.
- Whisper's window is never under 10 s: on trimmed clips the smaller formula window looped ("b, b, b, b")
  and garbled most short sentences (56 % word errors on 30 test sentences; 3 % with the floor). Short words
  no longer take 2–7 s or come back as loops ("stomp, stomp, stomp"); no temperature retries; a token cap.
  A lone "stop" is heard in ~0.7 s.
- `tools/capri-voice-setup` fetches `silero_vad.onnx` (SHA-256 checked).

### Voice: her replies to what arrives on its own are spoken
- `[voice] speak_unprompted = true` (default): a reply she makes without him typing first is spoken: events
  (hooks, `synaps send`, comms, summons) and the session driver (`event_auto`, `driver_auto`, live turns
  only). Turns he typed stay silent, as before. Unknown triggers stay silent.
- First lines only: two sentences / 30 words / up to `---`, then "The rest is on the line." once.
- Silent over an approval or a secret, while music plays (skipped, not queued), with his mic open, when
  hidden for fullscreen; the sidecar adds calls, the screen lock, dunst do-not-disturb, 10 s per reply,
  20 s per 60 s and 15 s staleness (`say-skipped`). Any of these mid-reply stops her for that turn.
- Such a reply waits behind her last sentences instead of cutting them off; one waits at most (newest
  wins). No duck or pause for these. Events never open the mic.
- With voice on, the avatar connects to the sidecar at start and every 10 s after it went away.
- Sidecar protocol: `say … "unprompted": true`, `drop`, `say-skipped`, `ready.features`.
- Tools: `tools/fakevoice.py` (a fake sidecar, no audio); the fake daemon's event turns are `event_auto`,
  plus `trig T TEXT` and `resolve`.

### Voice: her picked voice, and stress on a word (LOCKED 13 + 14)
- The default voice is Haseeb's pick:
  - **W3**, the bright blend (`timbre|prosody` split, pushed away from af_sarah, `+clean`);
  - **two semitones higher** (`@+4.93`, ~272 Hz median; since raised to `@+7.00`, ~306 Hz: "Her voice:
    higher" above) **and 23 % faster**: speed 1/0.77, pace "stretch";
  - flow **"emphasis"**.
- Pace "stretch": Kokoro speaks at its own speed, and the time goes ×0.77 in the same rubberband pass as
  the pitch shift. This keeps Kokoro's speed-1 pitch shape. It was his blind pick ("ALT") over Kokoro's own
  speed 1.32, which dropped the pitch floor ~1.4 st and the sentence ends ~2 st deeper. `[voice] pace =
  "model"` keeps that option.
- Flow: Kokoro's graph is patched in memory (weights and voices untouched, guarded by SHA-256, plain
  Kokoro as the fallback) to read and steer its own duration, pitch and energy predictions.
  - A `*word*` in her reply is stressed: a longer vowel, more energy, and a closed-loop pitch accent.
    Its local target is 4–6 st over the speech around the word, capped at 400 Hz.
  - The stretch after the word is lowered.
  - A yes/no question keeps its rise.
  - The `pauses` and `full` presets exist, but are off.
- With pace "model", Kokoro's speed now scales time linearly. Unrounded durations are rounded once, which
  removes the 25 ms steps that made 1.30 → 1.38 cut 18 %.
- New settings: `[voice] flow` = `emphasis` (default), `pauses`, `full` or `off`; `[voice] pace` =
  `stretch` (default) or `model`. `avatarctl settings reload` re-sends changed voice settings to the
  sidecar.
- `*word*` reaches the sidecar (it used to be stripped); `**bold**` and stray asterisks don't. The
  bubble and the caption never show the asterisks; the Line shows the word in italics.
- With voice on, her persona gains a few lines on writing for the ear. They allow one stressed word in
  a reply, and say most replies need none.
- `capri-voice --render` (`|||` separates replies) and `--selftest` cover the new rules. Details are in
  `docs/VOICE.md`.
