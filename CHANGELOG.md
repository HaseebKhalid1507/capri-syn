# Changelog

## Unreleased

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
  - **two semitones higher** (~272 Hz median) **and 23 % faster**: speed 1/0.77, pace "stretch";
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
