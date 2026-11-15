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
| `speed` | `1.2987` | 0.5 to 2; 1.2987 = 1/0.77 goes with the default voice |
| `pace` | `"stretch"` | how the speed is made: `"stretch"` (Kokoro at its own speed, the time scaled in the pitch pass) or `"model"` (Kokoro's speed) |
| `flow` | `"emphasis"` | `"emphasis"`, `"pauses"`, `"full"` or `"off"` (below) |
| `duck` | `"pause"` | other audio while she listens or speaks: `"pause"` (MPRIS), `"duck"` (PipeWire) or `"off"` |
| `speak_unprompted` | `true` | her replies to what arrives on its own are spoken too (below) |
| `key` | `'c'` | the talk key's letter (`$mod+c` in i3) |

`avatarctl settings reload` re-sends changed voice settings to a connected sidecar.

## Which replies are spoken

- **His turns**: spoken when he spoke them here (push-to-talk). A turn he typed stays silent.
- **Replies to what arrives on its own** (`speak_unprompted = true`, the default): any reply she makes
  without him typing first: events (hooks, `synaps send` messages, comms, a summons, notifications) and
  the session driver. The daemon's `TurnStarted.trigger` decides it, and only a turn that starts live on
  the current attach counts:
  - `event_auto`, `driver_auto`: hers, spoken;
  - `user`, `queued_auto`, `plugin_command`, or no trigger (an older daemon): his (spoken only if he spoke it);
  - `compaction`, and any trigger this build doesn't know: never spoken (fail closed);
  - the reply an attach shows, and a turn already running at attach: never spoken.
- Only its **first lines** are spoken: up to a `---` line, two sentences or 30 words (the first sentence
  always), then "The rest is on the line." once if anything is left. The persona asks her to open such a
  reply with one sentence that stands alone and names who it is from.
- She stays **silent** (the reply still lands in the bubble, the Line and the unread strip). The decision
  is made when the turn starts, and silent at the start means silent for the whole turn. If one of these
  becomes true while she speaks, she stops and does not resume:
  - an approval or a secret is on screen (a prompt's text is never spoken);
  - a media player is playing (the same MPRIS check as her LISTENING mood, `[music] ignore` honoured):
    **skipped, not queued**, so nothing is said minutes late;
  - his mic is open or his words are being written down;
  - she is hidden for a fullscreen window;
  - the sidecar's gate (`capri-voice`, checked before each sentence and every 0.35 s while one plays):
    a call (any other app capturing a real source), the screen locked (a locker process such as
    `i3lock`, or logind's LockedHint), his do-not-disturb (dunst paused), more than 10 s of audio for
    this reply, more than 20 s of unprompted speech in the last 60 s, or a reply that waited over 15 s
    for its turn;
  - voice is off, no sidecar is running, or the sidecar is too old to gate (fail closed).
- Never a monologue: a reply to an event never cuts her off. It waits behind her last sentences, and at
  most one waits: a newer one replaces it (the replaced one stays text). Hushing her (click, talk key, Esc)
  drops the waiting one too. A new turn of his stops her at once.
- These replies never pause or duck other audio, and there is no chime. **Events never open the mic.**
- With voice on, the avatar connects to the sidecar at start and looks for it again every 10 s.
  Connecting opens no mic.

## Her voice

```
af_heart:0.5,af_bella:0.3,af_jessica:0.2|af_heart:0.84,af_jessica:0.56,af_sarah:-0.4@+4.93+clean   speed 1/0.77, pace "stretch", flow "emphasis"
```

Haseeb picked it by ear (LOCKED 13 and 14 in the workspace):
- **W3:** a bright, energetic blend.
- **F2:** stressed words, no added pauses.
- **Pitch and pace:** two semitones higher, about 23 % faster (`rubberband -t 0.77`).
- **Pace by stretch:** in a blind listen of three files he chose "ALT" over "ENGINE" (Kokoro's own speed);
  see the table below.

These are stock Kokoro voices plus DSP. Nothing is cloned, trained or converted.

Voice string: `TIMBRE[|PROSODY][@SEMITONES][+clean][+glitch]`.
- **TIMBRE / PROSODY:** a voice, or a blend of voices with weights (`af_heart:0.5,af_bella:0.3`). Kokoro's style
  vector has two halves. The first drives the decoder (timbre). The second drives the duration, pitch and energy
  predictors (prosody). `T|P` takes each half from its own blend. A negative weight pushes away from a voice.
- **@SEMITONES:** a pitch shift that keeps the formants. It goes through the `rubberband` CLI when that is
  installed (`pacman -S rubberband`); otherwise it is done inside the model.
- **+clean:** high-pass filter, two EQ bells, an air shelf, a de-esser and a gentle compressor.
- **+glitch:** the machine trace on the attack.

**Pace.** With `pace = "stretch"` (the default), Kokoro speaks at its own speed 1. The time is then scaled by
1/`speed` in the same rubberband pass as the pitch shift, so the sentence keeps Kokoro's speed-1 pitch
shape. `pace = "model"` hands the speed to Kokoro instead. That replans the pitch:
- the floor sits about 1.4 st lower and sentence ends fall about 2 st deeper;
- its median sits ~0.4 st higher, so pair it with `@+4.50`.

Without rubberband installed, the pace is always "model".

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

With `pace = "model"`, speed is applied before Kokoro's rounding. Stock Kokoro divides each phoneme's
duration by the speed, then rounds it to whole 25 ms frames, so speed moves in steps: 1.30 → 1.38 cut a test
set by 18 %, not 6 %. Here the unrounded durations are summed and rounded once, so speed scales time as
asked, and every phoneme keeps at least one frame.

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

Compared with the reference Haseeb picked (`rubberband -3 -F -p 2 -t 0.77` on the F2 render), against the two
ways of building it in the engine. He heard all three and chose the stretch (ALT):

| | reference | **pace "stretch" (default, ALT)** | pace "model" (ENGINE: Kokoro speed 1.32, @+4.50) |
|---|---|---|---|
| F0 median / p10 / p90 | 272 / 221 / 360 Hz | **273 / 223 / 355 Hz** | 272 / 204 / 349 Hz |
| syllables per second (articulation) | 6.22 | **6.15** | 6.20 |
| end glide | −5.0 st | **−4.6 st** | −6.8 st |
| "Bold" (sentence onset) | 403 Hz | **402 Hz** | 361 Hz |
| "eleven" (word over the 450 ms before it) | +7.9 st, 393 Hz | **+5.4 st, 342 Hz** | +4.6 st, 320 Hz |
| "branch?" end vs the "whole" peak | −1.9 st | **−1.3 st** | −0.5 st |
| first sentence of a reply, CPU | – | **mean 240 ms, max 325 ms** | mean 201 ms, max 279 ms |

The stretch keeps the reference's pitch shape: its floor, top and sentence ends match within half a
semitone. Kokoro's own speed changes the shape. ASR (whisper base.en) loses no word in either; the stretch
transcribes exactly like W3 plain.

`voice-samples/r24/SHIPPED.wav` is the default path (`capri-voice --render` with no options). It is
sample-identical to ENGINE-alt.wav up to its loudness normalisation: the same length to the sample, and
75.8 dB SNR after one gain (at most 1.5 LSB apart).

## Checks

- `tools/capri-voice --selftest`: voice strings, markup, gaps. With the models present it also checks: the
  neutral patch equals stock, stress spans, linear speed, the same text giving the same audio, the local
  stress caps, the question rise, and both paces.
- `tools/capri-voice --render "One. || Two *words*. ||| A new reply." --out /tmp/x.wav [--voice V --speed S
  --flow F --pace P]` writes a sample. `||` separates sentences and `|||` separates replies. It never opens
  the mic or plays anything.
- `tools/abuild test` covers the markup, display and persona tests (`voice::tests`, `reader::tests`,
  `session::tests`, `settings::tests`).
