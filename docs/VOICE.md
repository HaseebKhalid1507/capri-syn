# Capri's voice

Push-to-talk in, spoken replies out. The avatar holds no audio. `tools/capri-voice` (the sidecar) owns the
mic, the speech models and the speaker, and talks to the avatar over a unix socket. The protocol is at the
top of that file. Everything runs on the CPU: whisper.cpp for speech-to-text, Kokoro (int8, onnxruntime)
for speech. Setup: `tools/capri-voice-setup`, then `tools/systemd/capri-voice.service`.

## Settings: `[voice]` in `~/.config/synaps-avatar/settings.toml`

| key | default | |
|---|---|---|
| `enabled` | `false` | voice on: the talk key, spoken replies, and the persona lines below |
| `stt_model` | `"small.en"` | `"tiny.en"`, `"base.en"`, `"small.en"` or `"medium.en"` |
| `voice` | her voice (below) | a Kokoro voice or a voice string |
| `speed` | `1.32` | 0.5 to 2; 1.32 belongs to the default voice |
| `flow` | `"emphasis"` | `"emphasis"`, `"pauses"`, `"full"` or `"off"` (below) |
| `duck` | `"pause"` | other audio while she listens or speaks: `"pause"` (MPRIS), `"duck"` (PipeWire) or `"off"` |
| `key` | `'c'` | the talk key's letter (`$mod+c` in i3) |

`avatarctl settings reload` re-sends changed voice settings to a connected sidecar.

## Her voice

```
af_heart:0.5,af_bella:0.3,af_jessica:0.2|af_heart:0.84,af_jessica:0.56,af_sarah:-0.4@+4.50+clean   speed 1.32, flow "emphasis"
```

Haseeb picked it by ear (LOCKED 13 and 14 in the workspace):
- **W3:** a bright, energetic blend.
- **F2:** stressed words, no added pauses.
- **Pitch and pace:** two semitones higher, about 23 % faster.

These are stock Kokoro voices plus DSP. Nothing is cloned, trained or converted.

Voice string: `TIMBRE[|PROSODY][@SEMITONES][+clean][+glitch]`.
- **TIMBRE / PROSODY:** a voice, or a blend of voices with weights (`af_heart:0.5,af_bella:0.3`). Kokoro's style
  vector has two halves. The first drives the decoder (timbre). The second drives the duration, pitch and energy
  predictors (prosody). `T|P` takes each half from its own blend. A negative weight pushes away from a voice.
- **@SEMITONES:** a pitch shift that keeps the formants. It goes through the `rubberband` CLI when that is
  installed (`pacman -S rubberband`); otherwise it is done inside the model.
- **+clean:** high-pass filter, two EQ bells, an air shelf, a de-esser and a gentle compressor.
- **+glitch:** the machine trace on the attack.

The shift is +4.50, not W3's +2.93 + 2. At speed 1.32 Kokoro plans its pitch about 0.4 st higher, and +4.50
lands on the reference's ~272 Hz median.

## Flow: how she stresses a word

Kokoro has no SSML, and it barely reacts to stress marks. Its graph predicts how long each phoneme lasts,
then a pitch curve and an energy curve, and only then decodes. The sidecar patches the model graph **in
memory**: the weights and voices are untouched and nothing is written to disk. The patch exposes those
predictions:
- `pred`: the graph cut down to the text encoder plus the duration predictor (~20 ms). Its output is each
  phoneme's duration before rounding.
- `f0`: pitch for a given set of durations.
- `synth`: the full graph, taking durations as an input plus a pitch multiplier and an energy offset.

Each sentence takes one decoder pass, so there are no seams. The patch loads only for the model file it was
built against, checked by SHA-256. On any other file, or if a pass fails, she speaks plain Kokoro. `--selftest`
checks that the patched graph with neutral inputs produces the same audio as the stock model.

Presets:

| `flow` | what it does |
|---|---|
| `emphasis` (default) | stresses `*word*`. Sentences inside one reply are 0.44 s / speed apart, as heard |
| `pauses` | adds pauses at punctuation (`,` `;` `—` `…` `.` `?`), with the forced frames muted. Not his pick; off |
| `full` | pauses + emphasis + a pitch register by position + speed jitter. Not his pick; off |
| `off` | plain Kokoro |

Emphasis on a `*word*` (one per sentence; a span of up to three words):
- **Length:** the stressed vowel lasts ×1.25, and the word's first consonant gets one more frame.
- **Energy:** about +2.5 dB on the word.
- **Pitch, closed loop and local:** the sidecar reads the model's own pitch plan first. The word's prominence
  is its peak over the median of the speech in the 450 ms before it, and over the 450 ms after it; the
  smaller of the two counts. The windows shrink with speed. They never reach across a pause or into the
  sentence's first 400 ms.
  - The bump brings the prominence to +1.5 st, clamped to 4..6 st. The bump itself is 0.5 to 4 st.
  - The word never goes over 400 Hz as heard.
  - A word that opens its sentence gets no pitch bump: it already rides the onset.
- **After the word:** pitch 1 st lower and slightly quieter, from the end of the word to the next pause.
- **Yes/no questions:** no lowering after the word, and the word's bump is capped at 2 st. The last 200 ms of
  voice ramp up (up to +3 st) until the question ends at most 1 st under the word's peak. Questions that open
  with what/why/how/… are left alone.

Speed is applied before Kokoro's rounding. Stock Kokoro divides each phoneme's duration by the speed, then
rounds it to whole 25 ms frames, so speed moves in steps: 1.30 → 1.38 cut a test set by 18 %, not 6 %. Here
the unrounded durations are summed and rounded once, so speed scales time as asked, and every phoneme
keeps at least one frame.

## Speech markup and the persona

While voice is on, a few lines join her persona (`voice::PERSONA`). They only matter on messages that start
with the `[voice: …]` hint. They ask for replies written for the ear, and allow **one** word in asterisks when
it carries the point: "at most one in a reply, and most replies need none".

- **Speech** (`voice.rs`): `*word*` reaches the sidecar. `**bold**` and stray asterisks don't. A span of more
  than three words is read plain.
- **Display:** the asterisks never show.
  - The bubble drops them (`session::clean` → `voice::unmark`).
  - The Line renders the word in italics (`reader::Mark::Em`).
  - The spoken caption drops them.
  - `a * b`, `2*3*4` and anything in backticks are left as written.

## Measured (six test lines, bella quiet)

Compared with the reference Haseeb picked (`rubberband -3 -F -p 2 -t 0.77` on the F2 render):

| | reference | engine (default) | alt: speed 1 + rubberband `-t 0.77` |
|---|---|---|---|
| F0 median / p10 / p90 | 272 / 221 / 360 Hz | 272 / 204 / 349 Hz | 273 / 223 / 355 Hz |
| syllables per second (articulation) | 6.22 | 6.20 | 6.15 |
| end glide | −5.0 st | −6.8 st | −4.6 st |
| "eleven" (word over the 450 ms before it) | +7.9 st, 393 Hz | +4.6 st, 320 Hz | +5.4 st, 342 Hz |
| first sentence of a reply, CPU | – | mean 201 ms, max 279 ms | mean 249 ms, max 328 ms |

Speeding Kokoro up inside the model changes its pitch plan: the floor drops about 1.4 st and sentence ends
fall deeper. The rubberband stretch keeps the F2 contour. ASR (whisper base.en) loses no word in either.

## Checks

- `tools/capri-voice --selftest`: voice strings, markup, gaps. With the models present it also checks: the
  neutral patch equals stock, stress spans, linear speed, the same text giving the same audio, the local
  stress caps, and the question rise.
- `tools/capri-voice --render "One. || Two *words*." --out /tmp/x.wav [--voice V --speed S --flow F]`
  writes a sample. It never opens the mic or plays anything.
- `tools/abuild test` covers the markup, display and persona tests (`voice::tests`, `reader::tests`,
  `session::tests`, `settings::tests`).
