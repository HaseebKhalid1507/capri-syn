# Changelog

## Unreleased

### Voice: her picked voice, and stress on a word (LOCKED 13 + 14)
- The default voice is Haseeb's pick:
  - **W3**, the bright blend (`timbre|prosody` split, pushed away from af_sarah, `+clean`);
  - **two semitones higher** (~270 Hz median) **and 23 % faster** (Kokoro speed 1.32);
  - flow **"emphasis"**.
  All of it is built in the engine: the output is not time-stretched.
- Flow: Kokoro's graph is patched in memory (weights and voices untouched, guarded by SHA-256, plain
  Kokoro as the fallback) to read and steer its own duration, pitch and energy predictions.
  - A `*word*` in her reply is stressed: a longer vowel, more energy, and a closed-loop pitch accent.
    Its local target is 4–6 st over the speech around the word, capped at 400 Hz.
  - The stretch after the word is lowered.
  - A yes/no question keeps its rise.
  - The `pauses` and `full` presets exist, but are off.
- Kokoro's speed now scales time linearly. Unrounded durations are rounded once, which removes the
  25 ms steps that made 1.30 → 1.38 cut 18 %.
- `[voice] flow` setting: `emphasis` (default), `pauses`, `full` or `off`. `avatarctl settings reload`
  re-sends changed voice settings to the sidecar.
- `*word*` reaches the sidecar (it used to be stripped); `**bold**` and stray asterisks don't. The
  bubble and the caption never show the asterisks; the Line shows the word in italics.
- With voice on, her persona gains a few lines on writing for the ear. They allow one stressed word in
  a reply, and say most replies need none.
- `capri-voice --render` / `--selftest` cover the new rules. Details are in `docs/VOICE.md`.
