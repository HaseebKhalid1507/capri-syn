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
| `duck` | `"duck"` | other audio while she listens or speaks: `"duck"` (−18 dB, put back exactly), `"pause"` (MPRIS) or `"off"` |
| `speak_unprompted` | `true` | her replies to what arrives on its own are spoken too (below) |
| `key` | `'c'` | the talk key's letter (`$mod+c` in i3) |
| `silence_s` | `1.0` | 0.5 to 5: the pause that ends a toggled turn |
| `toggle_key` | `'c'` | the toggle chord's letter (`$mod+Ctrl+c` in i3): its auto-repeat is one toggle |
| `handsfree` | `true` | the double-tap, `talk handsfree on` and the menu item can start a hands-free conversation. It is always off at launch |
| `handsfree_timeout` | `60` | 15 to 600 s: it ends after this long with no accepted turn of his |
| `handsfree_max` | `30` | minutes, 0 = no cap: it ends after this long in all |
| `wake_word` | `false` | **a draft, not for use yet**: "Hey Capri" opens a turn (below). On, a mic stream stays open, shown on her tag |
| `wake_sensitivity` | `0.5` | 0.1 to 0.9: higher wakes more easily (and falsely more often) |
| `echo` | `"auto"` | `"auto"` (the sink says headphones or speakers), `"headphones"` (talking over her stops her) or `"speakers"` (half-duplex) |

`avatarctl settings reload` re-sends changed voice settings to a connected sidecar.

## Talking to her: hold, or toggle

- **Hold** (`$mod+c` → `capri-ctl talk press|release`): the mic is open while the key is down; the
  release sends. A tap (under 250 ms) opens the Line for typing.
- **Toggle** (`$mod+Ctrl+c` → `capri-ctl talk toggle`, on press): one tap opens the mic, and his pause
  sends: `silence_s` (1.0 s) of silence after he has spoken, ×1.5 while he has said less than 1 s ("so…",
  "um, Capri…" often stop for a breath). The tag reads `● MIC 0:03 ▮▮▯ · AUTO`.
  - A second tap sends now once he has started; before that it cancels. The talk key does the same.
  - Esc in the Line, or `capri-ctl talk cancel`: nothing is sent.
  - No speech in 8 s: `NOTHING HEARD · MIC CLOSED`. At 60 s it sends what it has
    (`SENT AT 1:00 · MAX LENGTH`; a 42 s utterance decodes whole).
  - Holding the chord is one toggle: after a toggle, more are ignored until its key is seen up.
  - A toggle while the talk key is held is ignored.
  - The Line opens with the keyboard, so Esc works. If the tap opened it, the keyboard goes back to his
    window when the turn ends.
- **The mic refuses**, with the reason on her tag: a secret on screen (an approval doesn't: nothing
  spoken answers one), the screen locked, voice off, no speech detector (toggle), and for a toggle,
  her hidden (her tag must be on screen). A secret appearing or the screen locking closes any open
  mic; hiding closes a toggled one within 1 s.
- **Caps** in the sidecar, whatever the avatar does: hold 120 s, toggle 65 s.
- `avatarctl state` → `voice.mic = {open, mode: "ptt"|"toggle", since_ms}`. While a toggled mic is open
  the avatar asks the sidecar every 2 s; if it says the mic is closed, it is.
- **How the end is found:** Silero on 32 ms frames. Speech starts at p ≥ 0.50 and stays speech until
  p < 0.35, so a soft word ending isn't silence. Once out of speech, every frame counts toward the pause,
  frames in the 0.35–0.50 band too, so a far voice or a TV hovering there doesn't hold the mic open. A turn
  has started after 6 speech frames.
- **A draft decode** starts in his pause (`silence_s − 0.9 s`, at least 0.2 s), while the stream stays
  open. If no new speech came by the end, its words are sent with no second decode (speech end → sent:
  ~1.1 s instead of ~1.7 s). If they end on a dangling word ("and", "the", "because"…) or on `,` `…` `—`,
  she waits 1.5 s more.
- **The one exception to "her tag on screen":** when a toggle opens the Line, the Line moves her (a
  teleport), so for a moment she isn't drawn. The Line and its mic row are on screen the whole time, so a
  teleport closes a toggled mic only if she stays undrawn for more than 2 s. Hiding (user, fullscreen)
  closes it at once.

## Hands-free: a conversation

- **On:** double-tap the toggle chord (the second tap within 400 ms, before he has said anything), the
  menu's HANDS-FREE TALK, or `capri-ctl talk handsfree on`. Always off at launch, never restored.
  Refused, with the reason on her tag, when voice is off, `handsfree = false`, a secret is on screen,
  the screen is locked, he is in a call, she is hidden, or there is no sidecar.
- **One stream, many turns.** Each utterance ends on his pause exactly as a toggle does (Silero, the
  draft decode, the hold after a dangling word) and is sent; the mic stays open. Her reply is spoken. The
  tag reads `● MIC LIVE · HANDS-FREE 0:42 ▮▯▯▯▯`; the Line's mic row says how to end it.
- **Off**, the stream closed by the sidecar, the reason on her tag:

  | cause | tag |
  |---|---|
  | the chord (one tap), the menu, `talk handsfree off`, `talk cancel` | `HANDS-FREE OFF` |
  | an end phrase, the whole utterance: `that's all`, `that is all`, `that'll be all`, `we're done`, `stop listening`, `goodbye`, `bye` (a leading ok/okay/alright/thanks and a trailing capri/thanks/for now/thank you are allowed; never over 6 words). Not sent | `HANDS-FREE OFF` |
  | `handsfree_timeout` with no accepted turn of his (the last 10 s count down on the tag) | `HANDS-FREE OFF · 60 S QUIET` |
  | `handsfree_max` | `HANDS-FREE OFF · 30 MIN` |
  | the screen locks | `HANDS-FREE ENDED AT LOCK` |
  | a call starts (another app captures a real source, or `pactl` can't say) | `HANDS-FREE OFF · IN A CALL` |
  | a secret on screen | `HANDS-FREE OFF · A SECRET IS ON SCREEN` |
  | she is hidden | `HANDS-FREE OFF · HIDDEN` |
  | the stream dies, or PipeWire moves it to another source | `HANDS-FREE OFF · MIC LOST` |

  A lost stream is never reopened on whatever the new default source is.
- **The quiet clock** restarts on an accepted turn of his and while she thinks, speaks or writes his
  words down. His voice alone doesn't restart it: a TV that Whisper turns into junk (dropped) doesn't
  keep the mic open. It never ends mid-utterance.
- **Esc** drops what he is saying now (`DROPPED · STILL LISTENING`); the conversation stays. The talk key
  only hushes her (no second capture).
- **Other audio** is held (ducked or paused, `duck`) for the whole conversation and comes back 0.8 s after
  it ends.
- **Her voice and his.** `echo = "auto"` reads the default sink at the start: headphones (port, name or
  form factor says headphone/headset/earbud/hands-free) or speakers.
  - **Speakers: half-duplex.** The mic stays open but hears nothing from her first sentence of a reply to
    0.25 s after her last one (the gaps between her sentences included), so she can't answer herself.
    `$mod+c`, the chord or a click stop her.
  - **Headphones: talking over her stops her** (Silero p ≥ 0.50 on 7 of the last 9 frames, ~0.3 s of
    his voice).
  - **The echo check** (a jack labelled Headphones may feed desk speakers): her first sentence is
    observe-only. The mic's 20 ms level envelope is correlated with her playback's over 0–600 ms after
    pw-play starts; correlation ≥ 0.5 with the mic ≥ 10 dB over its idle floor → speakers for the rest of
    the conversation (`HEARING MYSELF · TALK-OVER OFF`), and what the mic held is dropped. The sink and its
    port are read again at each of her sentences: one that changed and isn't headphones is speakers at
    once; another headphone one is observed again.
  - **The echo guard**, both modes: an utterance whose words are ≥ 70 % what she said in the last 15 s
    is dropped (logged by length only).
- **Replies to what arrives on its own** are spoken while he is quiet, and stay text while he is
  mid-utterance (`speak_unprompted`, below).
- **The one exception to "her tag on screen"** holds here too: the Line's opening moves her, and a
  hands-free stream closes only if she stays undrawn for more than 2 s. Hidden for a fullscreen window
  ends it at once, so hands-free can't run under a fullscreen app.
- Cap in the sidecar, whatever the avatar does: its `max_s` (from `handsfree_max`).
- Measured on the virtual chain (bella, nothing played aloud): see the PR.

## "Hey Capri": the wake word (a draft, off by default)

**Not for use until it is measured on his voice.** Everything below was measured on stock synthetic voices
only (Kokoro packs held out of training, espeak-ng), never on a recording of anyone.

- **On** only with `wake_word = true` (and voice on). Nothing about it loads before that. Then one mic
  stream stays open, and her tag says so all the time: `◌ MIC ON · SAY "HEY CAPRI"` (dim red outline, no
  timer, no meter, no level events). It keeps the tag's first slot.
- **Stage 1, always on:** openWakeWord's front end (its v0.5.1 `melspectrogram.onnx` and
  `embedding_model.onnx`, SHA-pinned; its README puts its released models under CC BY-NC-SA 4.0, so
  personal use) and our head (`tools/wake/hey-capri-v1.npz`: 16 embeddings of 96 → 64 → 1, numpy). Every
  80 ms it scores the last ~2 s. The last 2.5 s of audio are kept in RAM, nothing more.
- **Stage 2, only on a candidate** (two hops in a row over the threshold): 0.3 s later, the loaded Whisper
  decodes the last 2 s with no recognition prompt. It must say a hey-like word, then capri-like, as Whisper
  spells it (Capri, Copry, Kapree, "cup pray", "Haikapri"); never Capricorn, Capra, caprese or Cappy. Fail: nothing visible happens, nothing is held or sent, and the
  log gets a count and a score, never words. **At most 12 checks in any 10 minutes**; the 13th candidate
  pauses it for 10 minutes (`WAKE PAUSED · TOO MANY FALSE STARTS`).
- **A pass** flips the tag to `● MIC`, holds other audio, and takes his turn as a toggle does (it ends on
  his pause). "Hey Capri, what's spike doing?" is one turn, and the phrase is cut from it. "Hey Capri" then
  a pause: the mic waits 5 s for speech, then `NOTHING HEARD`. "Hey Capri, let's talk" (`let's chat`,
  `hands free`) starts hands-free. A turn is sent at 30 s.
- **She can't wake herself:** on speakers her voice is dropped at the reader while she speaks (+0.25 s).
  On headphones the phrase cuts in and stops her.
- **Closed** (no stream, the reason on her tag) while: a secret is on screen (`WAKE PAUSED · A SECRET IS ON
  SCREEN`), she is hidden, the screen is locked, he is in a call (or `pactl` can't say). It comes back when
  the cause clears (the avatar asks again every 3 s; the sidecar refuses while locked or in a call). Push-to-
  talk, a toggle or hands-free close it for their turn, and it comes back after. A stream that dies is not
  reopened (`WAKE OFF · MIC LOST`) until the settings are reloaded.
- **Measured (synthetic voices only: 8 Kokoro packs held out of training, and espeak-ng):** stage 1 at
  sensitivity 0.5 finds 94 % of clean wakes and 92 % at 10 dB with reverb, and fires 0.4 times an hour on
  real read speech (2 in 5.4 h of LibriSpeech test-clean) and 0.5 an hour on the validation set. But
  Whisper accepts only about half of the true wakes on these voices (56 % clean, 33 % noisy), so **end to
  end it wakes on about half (clean) and a third (noisy)**. That is under the 90 % bar, which is why it is
  a draft. With the prompt "Hey Capri." the check passes 85 % / 74 %, but near phrases ("hey caprese",
  "hey Cappy") then pass 9 % of the time instead of 2.5 %. Details in the PR.
- `avatarctl state` → `voice.wake = {on, sensitivity, open, turn, paused, lost}`; `voice.mic.mode` is
  `"wake"` while it listens and `"wake-turn"` during his turn. The sidecar's `stats` count candidates,
  rejections, wakes and pauses.

## Other audio while he talks or she speaks (`duck`)

- `"duck"` (default): every eligible stream (browser video, music, games) goes down **−18 dB** (each
  channel's raw volume ×0.5; Pulse volume is cubic) while his mic is open and while she answers him, then
  comes back to **exactly** its old per-channel level, 800 ms after her last word.
  - Never ducked: her own streams, module/virtual streams (e.g. a combine-sink's loopback), calls
    (`media.role` phone/communication, the `[music] ignore` apps), notification sounds. While another app
    captures a real mic (a call with no role), nothing is ducked at all.
  - A stream that starts (or unpauses) mid-duck is ducked too. A stream whose volume he changed mid-duck
    keeps his change. A stream is only put back if it is the same stream (`object.serial`).
  - WirePlumber remembers a stream's last volume per app: a stream that ends while ducked would come
    back ducked next time. Its old level is kept for 10 minutes, and a stream of that app that shows up
    at exactly the ducked level gets it back.
  - A crash mid-duck is repaired at the sidecar's next start (the levels are saved to
    `~/.local/state/capri-voice/media.json`, written atomically).
- `"pause"`: MPRIS players are paused, then played again (only the ones she paused).
- Her replies to events never duck or pause anything.

## What counts as speech

- Before Whisper runs, Silero VAD (v5, 2.3 MB, `tools/capri-voice-setup` fetches it, SHA-256 checked)
  must find at least **6 speech frames (192 ms)** in the clip, peaking at p ≥ 0.70. Otherwise nothing is
  transcribed or sent, and her tag says DIDN'T CATCH THAT. The clip is trimmed to 400 ms before the first
  speech frame and 200 ms after the last. Without the file, the old energy gate stands in (`stats` says
  `vad: "energy"`).
- A transcript is dropped (same quiet hint) when it is junk Whisper writes from near-silence:
  - no letters (`-2,`, `,`, `2.`). Note: a bare number he says, written by Whisper as `2.`, is dropped too;
  - two letters or fewer, unless one of `no ok hi go yo ya ye up on me do it` (`nd,` and `Q.` drop);
  - a single word with no vowel (`, chr,`, `hmm`);
  - `you`, `thanks for watching`, `please subscribe`; `thank you` and `bye` from under 400 ms of speech;
  - three or more words that are all from the recognition prompt (Capri, spike, lynx, …).
- A drop is logged by length and milliseconds of speech, never the words or the audio.
- Whisper's window: the utterance + 1 s, but never under 10 s (a trimmed clip in a smaller window
  decodes into loops like "yes, yes, yes" or "b, b, b" and took 2–7 s); no temperature retries; a token
  cap; a run of 4+ of the same word is kept once (the log says how many were dropped, never the words).
- Limits: whispering is not heard (the speech detector scores it below the gate).

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
  is made when the turn starts (conservative: a condition that clears before her first sentence still
  keeps the turn silent), and silent at the start means silent for the whole turn. If one of these
  becomes true while she speaks, she stops and does not resume:
  - an approval or a secret is on screen (a prompt's text is never spoken);
  - a media player is playing (the same MPRIS check as her LISTENING mood, `[music] ignore` honoured):
    **skipped, not queued**, so nothing is said minutes late;
  - his mic is open or his words are being written down;
  - she is hidden for a fullscreen window;
  - the sidecar's gate (`capri-voice`, checked before each sentence and every 0.35 s while one plays):
    his do-not-disturb (dunst paused), more than 10 s of audio for this reply, more than 20 s of
    unprompted speech in the last 60 s, or a reply that waited over 15 s for its turn;
  - voice is off, no sidecar is running, or the sidecar is too old to gate (fail closed).
- Never a monologue: a reply to an event never cuts her off. It waits behind her last sentences, and at
  most one waits: a newer one replaces it (the replaced one stays text). Hushing her (click, talk key, Esc)
  drops the waiting one too. A new turn of his stops her at once.
- These replies never pause or duck other audio, and there is no chime. **Events never open the mic.**
- **Nothing she says is heard** (his spoken turns included) **with the screen locked or in a call**: the
  sidecar checks before every sentence and every 0.35 s while one plays, stops her, and her tag says why.
  Locked = a locker process (`i3lock`, `xsecurelock`, `slock`, `swaylock`, … by name) or logind's
  LockedHint. A call = any other app capturing a real source (not a monitor; level meters don't count).
  If `pactl` can't answer, that counts as a call ("audio state unknown").
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
