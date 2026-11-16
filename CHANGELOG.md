# Changelog

## Unreleased

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
