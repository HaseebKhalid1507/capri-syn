# Changelog

## Unreleased

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
