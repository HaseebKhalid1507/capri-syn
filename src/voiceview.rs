//! Voice, from the avatar's side: push-to-talk on the talk key, the `capri-voice` sidecar link,
//! the truthful `● MIC` tag, spoken replies (his spoken turns, and her replies to what arrives
//! on its own) with captions and lip-sync, barge-in. A child module of `view`. The sidecar owns
//! the mic and the models; nothing here touches audio.

use super::*;
use crate::settings::VoiceSettings;
use crate::voice::{self, Speech};
use std::io::{BufRead, Write};
use std::os::unix::net::UnixStream;

/// Key-up waits this long before it counts: X auto-repeat sends release/press pairs.
const UP_DEBOUNCE: Duration = Duration::from_millis(60);
/// Other audio comes back this long after her last word.
const RESTORE_MS: u64 = 800;
/// With voice on, a missing sidecar is looked for again this often (connecting opens no mic).
const RECONNECT: Duration = Duration::from_secs(10);

pub(super) struct Lips {
    t0: Instant,
    step: Duration,
    shapes: Vec<u8>,
}

#[derive(Default)]
pub(super) struct VoiceState {
    pub cfg: VoiceSettings,
    link: Option<UnixStream>,
    generation: u64,
    /// listen-start sent (the hold), not yet stopped.
    asked: Option<Instant>,
    /// The sidecar says the capture stream is open, since.
    pub mic: Option<Instant>,
    level: f32,
    transcribing: bool,
    /// The next new turn is the one he spoke.
    pending: bool,
    /// The spoken turn, and what of it is said.
    turn: Option<u64>,
    /// It arrived on its own (not his): what keeps her quiet is checked all along.
    unprompted: bool,
    /// The turn before it, still playing (an unprompted turn waits behind it), and how many of
    /// its sentences are still out. At most one turn waits: a newer one replaces it.
    prev: Option<u64>,
    prev_left: u32,
    prev_unprompted: bool,
    /// His reply plays on under a newer, silent turn's bubble: its captions stay out of it.
    orphan: bool,
    /// The sidecar can gate unprompted speech (calls, lock, budget): its `ready` says so. Without
    /// it, unprompted replies stay silent (fail closed).
    can_unprompted: bool,
    /// The next look for a missing sidecar; its first miss is logged, not every retry.
    retry_at: Option<Instant>,
    missed: bool,
    speech: Speech,
    sent: u32,
    ended: u32,
    turn_done: bool,
    reply: String,
    lips: Option<Lips>,
    caption: Option<String>,
    /// Auto-repeat releases seen while the key stayed down (ignored).
    repeats: u32,
    held: bool,
    /// A key-up waiting out the auto-repeat window.
    up_at: Option<Instant>,
    /// Timings of the last turn (ms), for `voice stats`.
    keyup: Option<Instant>,
    heard_at: Option<Instant>,
    submit_at: Option<Instant>,
    pub stats: serde_json::Map<String, serde_json::Value>,
}

impl VoiceState {
    pub fn new(cfg: VoiceSettings) -> VoiceState {
        VoiceState { cfg, ..Default::default() }
    }
}

fn sock_path() -> std::path::PathBuf {
    if let Some(p) = std::env::var_os("CAPRI_VOICE_SOCK") {
        return p.into();
    }
    let rt = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    rt.join("capri-voice.sock")
}

impl Avatar {
    pub(super) fn voice_on(&self) -> bool {
        self.voice.cfg.enabled
    }

    pub(super) fn voice_speaking(&self) -> bool {
        self.voice.turn.is_some() && (self.voice.sent > self.voice.ended || self.voice.prev_left > 0)
    }

    pub(super) fn voice_listening(&self) -> bool {
        self.voice.asked.is_some() || self.voice.mic.is_some()
    }

    /// Connect to the sidecar if not yet (it is started on its own: a systemd user unit).
    pub(super) fn voice_connect(&mut self, cx: &mut Context<Self>) -> bool {
        if self.voice.link.is_some() {
            return true;
        }
        let path = sock_path();
        let s = match UnixStream::connect(&path) {
            Ok(s) => s,
            Err(e) => {
                if !std::mem::replace(&mut self.voice.missed, true) {
                    elog!("synaps-avatar: voice: no sidecar at {} ({e}); looking again every {} s", path.display(), RECONNECT.as_secs());
                }
                return false;
            }
        };
        self.voice.missed = false;
        let Ok(r) = s.try_clone() else { return false };
        self.voice.generation += 1;
        let generation = self.voice.generation;
        let (tx, rx) = async_channel::unbounded::<serde_json::Value>();
        std::thread::Builder::new()
            .name("voice-rx".into())
            .spawn(move || {
                for line in std::io::BufReader::new(r).lines() {
                    let Ok(l) = line else { break };
                    if let Ok(v) = serde_json::from_str(&l)
                        && tx.send_blocking(v).is_err()
                    {
                        break;
                    }
                }
            })
            .ok();
        cx.spawn(async move |this, cx| {
            while let Ok(v) = rx.recv().await {
                if this.update(cx, |a, cx| a.voice_event(generation, v, cx)).is_err() {
                    return;
                }
            }
            this.update(cx, |a, cx| a.voice_gone(generation, cx)).ok();
        })
        .detach();
        self.voice.link = Some(s);
        self.voice_config();
        elog!("synaps-avatar: voice: sidecar connected ({})", path.display());
        self.voice.link.is_some()
    }

    /// With voice on, the sidecar is kept connected (at start, and after it went away), so a
    /// reply to an event can be heard before he ever pressed the talk key.
    pub(super) fn voice_keepalive(&mut self, now: Instant, cx: &mut Context<Self>) {
        if !self.voice_on() || self.voice.link.is_some() || self.voice.retry_at.is_some_and(|t| now < t) {
            return;
        }
        self.voice.retry_at = Some(now + RECONNECT);
        if self.voice_connect(cx) {
            self.voice.retry_at = None;
        }
    }

    /// The sidecar gets the `[voice]` settings (on connect, and again when settings reload).
    pub(super) fn voice_config(&mut self) {
        // Calls are never ducked: the [music] ignore list (player names) goes along.
        let ignore = crate::settings::Settings::load().map(|s| s.music.ignore).unwrap_or_default();
        let c = &self.voice.cfg;
        let cfg = serde_json::json!({ "op": "config", "stt_model": c.stt_model, "voice": c.voice, "speed": c.speed, "pace": c.pace, "flow": c.flow, "duck_ignore": ignore });
        self.voice_send(cfg);
    }

    fn voice_send(&mut self, v: serde_json::Value) {
        let Some(s) = &mut self.voice.link else { return };
        let mut line = v.to_string();
        line.push('\n');
        if let Err(e) = s.write_all(line.as_bytes()) {
            elog!("synaps-avatar: voice: sidecar write failed: {e}");
            self.voice.link = None;
        }
    }

    fn voice_gone(&mut self, generation: u64, cx: &mut Context<Self>) {
        if generation != self.voice.generation {
            return;
        }
        elog!("synaps-avatar: voice: sidecar gone (its mic is closed with it)");
        self.voice.link = None;
        self.voice.mic = None;
        self.voice.asked = None;
        self.voice.transcribing = false;
        self.voice_stop_speech();
        self.hint = Some(("VOICE OFF · SIDECAR GONE".into(), Instant::now() + Duration::from_secs(4)));
        self.voice_changed(cx);
    }

    fn voice_changed(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.relayer(now);
        self.line_push(cx);
        cx.notify();
        self.schedule(cx);
    }

    // ---- the talk key ---------------------------------------------------------------------

    /// A fresh press (not an auto-repeat): she stops talking at once, and a hold past the tap
    /// time opens the mic. True if it hushed her (a tap then does nothing more).
    pub(super) fn voice_press(&mut self, t0: Instant, cx: &mut Context<Self>) -> bool {
        let hushed = self.voice_speaking();
        if hushed {
            self.voice_hush("talk key");
        }
        if !self.voice_on() {
            return hushed;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(lineview::TAP_MS as u64)).await;
            this.update(cx, |a, cx| a.voice_hold(t0, cx)).ok();
        })
        .detach();
        hushed
    }

    /// Still held at the tap time: listen (the stream opens only now, on his act).
    fn voice_hold(&mut self, t0: Instant, cx: &mut Context<Self>) {
        if self.talk_down != Some(t0) || self.voice.asked.is_some() {
            return;
        }
        // Secrets and approvals are never heard.
        if self.prompt_ui.as_ref().is_some_and(|p| p.view.secret) || self.input.as_ref().is_some_and(|i| i.secret.is_some()) {
            self.hint = Some(("NO MIC · A SECRET IS ON SCREEN".into(), Instant::now() + Duration::from_secs(3)));
            cx.notify();
            return;
        }
        if !self.voice_connect(cx) {
            self.hint = Some(("VOICE OFF · START capri-voice".into(), Instant::now() + Duration::from_secs(4)));
            cx.notify();
            self.schedule(cx);
            return;
        }
        let mode = self.voice.cfg.duck.clone();
        self.voice_send(serde_json::json!({ "op": "media-hold", "mode": mode }));
        self.voice.held = true;
        self.voice_send(serde_json::json!({ "op": "listen-start" }));
        self.voice.asked = Some(Instant::now());
        elog!("synaps-avatar: voice: hold: listening");
        self.voice_changed(cx);
    }

    /// Key-up, after the auto-repeat window: a hold stops listening (and sends what was heard).
    /// False if the key is still down (an X auto-repeat's release): ignored.
    pub(super) fn voice_up(&mut self, cx: &mut Context<Self>) -> bool {
        if self.talk_down.is_some() && self.x11.char_key_down(self.voice.cfg.key).unwrap_or(false) {
            self.voice.repeats += 1;
            return false;
        }
        let at = Instant::now();
        self.voice.up_at = Some(at);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(UP_DEBOUNCE).await;
            this.update(cx, |a, cx| {
                if a.voice.up_at == Some(at) {
                    a.voice.up_at = None;
                    a.voice_released(at, cx);
                }
            })
            .ok();
        })
        .detach();
        true
    }

    /// A press inside the key-up window: an auto-repeat pair, not a new press.
    pub(super) fn voice_repeat(&mut self) -> bool {
        self.voice.up_at.take().is_some()
    }

    fn voice_released(&mut self, at: Instant, cx: &mut Context<Self>) {
        self.talk_down = None;
        if self.voice.asked.take().is_none() {
            return;
        }
        self.voice.keyup = Some(at);
        self.voice.transcribing = true;
        self.voice_send(serde_json::json!({ "op": "listen-stop" }));
        elog!("synaps-avatar: voice: key up: transcribing");
        self.voice_changed(cx);
    }

    /// Esc while holding: nothing is transcribed or sent.
    pub(super) fn voice_cancel(&mut self, cx: &mut Context<Self>) {
        if self.voice.asked.take().is_some() || self.voice.mic.is_some() {
            self.voice_send(serde_json::json!({ "op": "listen-cancel" }));
            self.voice_send(serde_json::json!({ "op": "media-release" }));
            self.voice.held = false;
            self.hint = Some(("CANCELLED · NOTHING SENT".into(), Instant::now() + Duration::from_secs(3)));
            self.voice_changed(cx);
        }
    }

    /// Stop talking now (the key, Esc, a click). The reply keeps arriving in the thread.
    pub(super) fn voice_hush(&mut self, why: &str) {
        self.voice_send(serde_json::json!({ "op": "hush" }));
        elog!("synaps-avatar: voice: hushed ({why})");
        self.voice_stop_speech();
        self.hint = Some(("■ HUSHED · REST IS ON THE LINE".into(), Instant::now() + Duration::from_secs(4)));
    }

    fn voice_stop_speech(&mut self) {
        let was = self.voice.turn.is_some();
        self.voice.speech = Speech::default();
        self.voice.speech.feed("", true);
        self.voice.sent = 0;
        self.voice.ended = 0;
        self.voice.lips = None;
        self.voice.prev = None;
        self.voice.prev_left = 0;
        self.voice.prev_unprompted = false;
        self.voice.unprompted = false;
        if was {
            self.voice.turn = None;
            self.voice_caption_done();
        }
        if self.voice.held {
            self.voice.held = false;
            self.voice_send(serde_json::json!({ "op": "media-release", "after_ms": RESTORE_MS }));
        }
    }

    // ---- the sidecar's events -------------------------------------------------------------

    fn voice_event(&mut self, generation: u64, v: serde_json::Value, cx: &mut Context<Self>) {
        if generation != self.voice.generation {
            return;
        }
        let now = Instant::now();
        let ev = v.get("ev").and_then(|e| e.as_str()).unwrap_or("");
        match ev {
            "mic-open" => {
                self.voice.mic = Some(now);
                if let Some(a) = self.voice.asked {
                    self.voice.stats.insert("hold_to_mic_ms".into(), ((now - a).as_millis() as u64).into());
                }
                self.voice_changed(cx);
            }
            "level" => {
                self.voice.level = v.get("v").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
                self.line_push(cx);
                cx.notify();
            }
            "mic-closed" => {
                self.voice.mic = None;
                self.voice.level = 0.0;
                self.voice_changed(cx);
            }
            "heard" => {
                self.voice.transcribing = false;
                self.voice.heard_at = Some(now);
                let text = v.get("text").and_then(|t| t.as_str()).unwrap_or("").trim().to_string();
                if let Some(ms) = v.get("ms").and_then(|m| m.as_object()) {
                    for (k, x) in ms {
                        self.voice.stats.insert(format!("sidecar_{k}"), x.clone());
                    }
                }
                if let Some(k) = self.voice.keyup {
                    self.voice.stats.insert("keyup_to_heard_ms".into(), ((now - k).as_millis() as u64).into());
                }
                if text.is_empty() {
                    let why = v.get("why").and_then(|t| t.as_str()).filter(|w| !w.is_empty()).unwrap_or("DIDN'T CATCH THAT").to_string();
                    self.hint = Some((why, now + Duration::from_secs(3)));
                    self.voice_send(serde_json::json!({ "op": "media-release" }));
                    self.voice.held = false;
                } else {
                    elog!("synaps-avatar: voice: heard {} chars", text.chars().count());
                    self.voice.pending = true;
                    self.voice.submit_at = Some(now);
                    match self.send(voice::wrap(&text)) {
                        Ok(()) => self.ack_unread(),
                        Err(e) => {
                            self.voice.pending = false;
                            self.voice_send(serde_json::json!({ "op": "media-release" }));
                            self.voice.held = false;
                            self.unread = Some(lineview::Unread { n: 1, text: format!("NOT SENT ({e}) · \"{text}\""), refused: true });
                        }
                    }
                }
                self.voice_changed(cx);
            }
            "say-start" => {
                let id = v.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
                if !self.voice_ours(id) {
                    return;
                }
                if self.voice.turn == Some(id) && self.voice.prev.is_some() {
                    // The turn before it is over (the sidecar plays in order).
                    self.voice.prev = None;
                    self.voice.prev_left = 0;
                }
                if !self.voice.unprompted
                    && !self.voice.stats.contains_key("keyup_to_audio_ms")
                    && let Some(k) = self.voice.keyup
                {
                    self.voice.stats.insert("keyup_to_audio_ms".into(), ((now - k).as_millis() as u64).into());
                    if let Some(s) = self.voice.submit_at {
                        self.voice.stats.insert("submit_to_audio_ms".into(), ((now - s).as_millis() as u64).into());
                    }
                    if let Some(ms) = v.get("ms").and_then(|m| m.as_object()) {
                        for (k, x) in ms {
                            self.voice.stats.insert(format!("say1_{k}"), x.clone());
                        }
                    }
                    elog!("timing: voice: {}", serde_json::Value::Object(self.voice.stats.clone()));
                }
                let step = Duration::from_millis(v.get("step_ms").and_then(|x| x.as_u64()).unwrap_or(90));
                let shapes: Vec<u8> = v.get("shapes").and_then(|x| x.as_array()).map(|a| a.iter().map(|s| s.as_u64().unwrap_or(0) as u8).collect()).unwrap_or_default();
                self.voice.lips = Some(Lips { t0: now, step, shapes });
                let text = voice::unmark(v.get("text").and_then(|t| t.as_str()).unwrap_or(""));
                self.voice.caption = Some(text.clone());
                self.voice_caption(&text, now);
                self.voice_changed(cx);
            }
            "say-end" => {
                let id = v.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
                if !self.voice_ours(id) {
                    return;
                }
                if self.voice.prev == Some(id) {
                    self.voice.prev_left = self.voice.prev_left.saturating_sub(1);
                    if self.voice.prev_left == 0 {
                        self.voice.prev = None;
                    }
                } else {
                    self.voice.ended += 1;
                }
                if self.voice.ended >= self.voice.sent && self.voice.prev_left == 0 {
                    self.voice.lips = None;
                    if self.voice.turn_done {
                        self.voice_finish();
                    }
                }
                self.voice_changed(cx);
            }
            "ready" => {
                self.voice.can_unprompted = v.get("features").and_then(|f| f.as_array()).is_some_and(|f| f.iter().any(|x| x == "unprompted"));
            }
            // The sidecar kept an unprompted reply silent (a call, the lock, his do-not-disturb,
            // the speech budget, too late): the rest of it stays text.
            "say-skipped" => {
                let id = v.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
                let why = v.get("why").and_then(|x| x.as_str()).unwrap_or("").to_string();
                elog!("synaps-avatar: voice: the sidecar kept turn {id} silent ({why})");
                if self.voice_ours(id) {
                    let short = why.split(" (").next().unwrap_or("").to_uppercase();
                    self.hint = Some((format!("QUIET · {short} · IT'S ON THE LINE"), now + Duration::from_secs(3)));
                }
                if self.voice.prev == Some(id) {
                    self.voice.prev = None;
                    self.voice.prev_left = 0;
                } else if self.voice.turn == Some(id) {
                    self.voice.speech = Speech::default();
                    self.voice.speech.feed("", true);
                    self.voice.sent = 0;
                    self.voice.ended = 0;
                    if self.voice.prev.is_none() {
                        self.voice_stop_speech();
                    } else {
                        // Its turn never came: the one before it plays on; this one is text.
                        self.voice.turn_done = true;
                    }
                }
                self.voice_changed(cx);
            }
            "hushed" | "stats" => {}
            "error" => elog!("synaps-avatar: voice: sidecar: {v}"),
            _ => {}
        }
    }

    /// A sentence of the spoken turn, or of the one still finishing before it.
    fn voice_ours(&self, id: u64) -> bool {
        self.voice.turn == Some(id) || self.voice.prev == Some(id)
    }

    /// Why a reply that arrived on its own stays silent now, if it does.
    fn voice_unprompted_quiet(&self) -> Option<&'static str> {
        voice::Quiet {
            voice_off: !self.voice_on(),
            setting_off: !self.voice.cfg.speak_unprompted,
            no_sidecar: self.voice.link.is_none() || !self.voice.can_unprompted,
            prompt: self.voice_prompt_up(),
            music: self.player.playing,
            mic: self.voice_listening() || self.voice.transcribing,
            hidden: self.hidden_reason() == Some("fullscreen"),
        }
        .why()
    }

    /// An approval or a secret is on screen (never by voice: no mic, no speech).
    fn voice_prompt_up(&self) -> bool {
        self.prompt_ui.is_some() || self.input.as_ref().is_some_and(|i| i.secret.is_some())
    }

    /// While an unprompted reply is out: if something now keeps her quiet (music started, a
    /// prompt, his mic, fullscreen), she stops, and that turn stays silent after it clears.
    pub(super) fn voice_guard(&mut self) {
        if self.voice.turn.is_none() || !(self.voice.unprompted || self.voice.prev_unprompted) {
            return;
        }
        if let Some(why) = self.voice_unprompted_quiet() {
            self.voice_hush(why);
        }
    }

    /// The reply of a turn, as it streams: a spoken turn's sentences go to the sidecar. Spoken:
    /// his turns that he spoke here, and (`speak_unprompted`) her replies to what arrived on its
    /// own, unless something keeps her quiet (decided once, at the start: silent at the start
    /// is silent for the whole turn). True when the turn is spoken (the bubble shows captions,
    /// not the stream).
    pub(super) fn voice_reply(&mut self, b: &crate::session::Body, new_turn: bool) -> bool {
        if new_turn {
            let his = self.voice.pending && b.echo.as_deref().is_some_and(|e| e.starts_with("[voice"));
            let origin = voice::origin(&b.trigger);
            let unprompted = !his && origin == voice::Origin::Unprompted;
            let quiet = if unprompted { self.voice_unprompted_quiet() } else { None };
            if let Some(why) = quiet {
                elog!("synaps-avatar: voice: a reply to {} stays silent ({why})", b.trigger);
            } else if origin == voice::Origin::Quiet && !b.trigger.is_empty() && b.trigger != "compaction" {
                elog!("synaps-avatar: voice: trigger {:?} is unknown here: its reply stays silent", b.trigger);
            }
            let spoken = (his && self.voice.link.is_some()) || (unprompted && quiet.is_none());
            // His new turn stops her. An unprompted one that will be heard waits behind her last
            // sentences instead (events queued while she talked start the moment her text ends);
            // at most one waits: a newer one replaces it (that one stays text).
            let playing = self.voice.turn.filter(|_| self.voice.turn_done && (self.voice.sent > self.voice.ended || self.voice.prev_left > 0));
            let wait = unprompted && spoken && playing.is_some();
            // A silent turn that isn't his (an event's, compaction, an unknown trigger) doesn't cut
            // his spoken reply short either: it plays on, its captions kept out of the new bubble.
            let orphan = origin != voice::Origin::His && !spoken && playing.is_some() && !self.voice.unprompted && !self.voice.prev_unprompted;
            if orphan {
                self.voice.orphan = true;
            } else if self.voice.turn.is_some() && !wait {
                self.voice_hush("new turn");
            }
            self.voice.pending = false;
            if !spoken {
                return false;
            }
            self.voice.orphan = false;
            if wait {
                if self.voice.prev.is_some() {
                    // The one playing goes on; the one waiting is replaced (newest wins).
                    let id = self.voice.turn;
                    self.voice_send(serde_json::json!({ "op": "drop", "id": id }));
                    elog!("synaps-avatar: voice: a newer reply replaces the waiting one (turn {id:?} stays text)");
                } else {
                    self.voice.prev = self.voice.turn;
                    self.voice.prev_left = self.voice.sent - self.voice.ended;
                    self.voice.prev_unprompted = self.voice.unprompted;
                }
            } else {
                self.voice.prev = None;
                self.voice.prev_left = 0;
                self.voice.prev_unprompted = false;
            }
            self.voice.sent = 0;
            self.voice.ended = 0;
            self.voice.turn = Some(b.turn);
            self.voice.unprompted = unprompted;
            self.voice.speech = if unprompted { Speech::unprompted() } else { Speech::default() };
            self.voice.turn_done = false;
            if unprompted {
                // Never a pause or a duck for these: only calls and games are left to push down.
                elog!("synaps-avatar: voice: speaking a reply to {}", b.trigger);
            } else {
                self.voice.stats.remove("keyup_to_audio_ms");
                if let Some(s) = self.voice.submit_at {
                    self.voice.stats.insert("submit_to_turn_ms".into(), (s.elapsed().as_millis() as u64).into());
                }
            }
        }
        if self.voice.turn != Some(b.turn) {
            return false;
        }
        self.voice_guard();
        if self.voice.turn != Some(b.turn) {
            return false;
        }
        self.voice.reply = b.reply.clone();
        let first_text = !self.voice.unprompted && self.voice.sent == 0 && !b.raw.trim().is_empty() && !self.voice.stats.contains_key("submit_to_text_ms");
        if first_text && let Some(s) = self.voice.submit_at {
            self.voice.stats.insert("submit_to_text_ms".into(), (s.elapsed().as_millis() as u64).into());
        }
        for s in self.voice.speech.feed(&b.raw, !b.busy) {
            let id = b.turn;
            let mut say = serde_json::json!({ "op": "say", "id": id, "text": s });
            if self.voice.unprompted {
                say["unprompted"] = true.into();
            }
            self.voice_send(say);
            self.voice.sent += 1;
        }
        if !b.busy {
            self.voice.turn_done = true;
            if self.voice.ended >= self.voice.sent && self.voice.prev_left == 0 {
                self.voice_finish();
            }
        }
        true
    }

    /// Her last word: other audio comes back, the bubble gets the whole reply.
    fn voice_finish(&mut self) {
        self.voice.turn = None;
        self.voice.prev = None;
        self.voice.prev_left = 0;
        self.voice.prev_unprompted = false;
        self.voice.unprompted = false;
        self.voice.lips = None;
        if self.voice.held {
            self.voice.held = false;
            self.voice_send(serde_json::json!({ "op": "media-release", "after_ms": RESTORE_MS }));
        }
        self.voice_caption_done();
    }

    fn voice_caption(&mut self, text: &str, now: Instant) {
        let _ = now;
        if self.voice.orphan {
            return;
        }
        let Some(bb) = self.bubble.as_mut().filter(|x| x.stream) else { return };
        bb.text = text.chars().collect();
        bb.shown = bb.text.len();
        bb.wrap = None;
        bb.page = None;
        bb.more = true;
        bb.collapsed = false;
        bb.hide_at = None;
        bb.next_char = None;
    }

    fn voice_caption_done(&mut self) {
        let reply = std::mem::take(&mut self.voice.reply);
        if std::mem::take(&mut self.voice.orphan) {
            return;
        }
        let now = Instant::now();
        let Some(bb) = self.bubble.as_mut().filter(|x| x.stream) else { return };
        if !reply.is_empty() {
            bb.text = reply.chars().collect();
            bb.shown = bb.text.len();
            bb.wrap = None;
            bb.page = None;
        }
        bb.more = false;
        bb.reveal_all_if_long();
        if !bb.collapsed {
            bb.hide_at = Some(now + hold_for(bb.text.len()));
        }
    }

    // ---- what shows -----------------------------------------------------------------------

    /// The mouth shape now (standing only, v1), or closed while a spoken turn waits for audio.
    fn voice_shape(&self, now: Instant) -> Option<u8> {
        self.voice.turn?;
        let Some(l) = &self.voice.lips else { return Some(0) };
        let k = (now.saturating_duration_since(l.t0).as_millis() / l.step.as_millis().max(1)) as usize;
        Some(l.shapes.get(k).copied().unwrap_or(0))
    }

    /// The frame her layer shows for voice: leaning in while the mic is open, the mouth from
    /// the audio while she speaks. Standing only.
    pub(super) fn voice_layer(&self) -> Option<usize> {
        if !self.standing() || self.posed() {
            return None;
        }
        if self.voice.mic.is_some() || self.voice.asked.is_some() {
            return self.pack.index.get("lean_in-attentive").copied();
        }
        let s = self.voice_shape(Instant::now())?;
        self.pack.index.get(voice::mouth_frame(s)).copied()
    }

    /// Lips: the mouth follows the track. True if the frame changed.
    pub(super) fn voice_tick(&mut self, now: Instant) -> bool {
        self.voice_guard();
        if self.voice.lips.is_none() && self.voice.mic.is_none() {
            return false;
        }
        let before = self.anim.frame();
        if let Some(i) = self.voice_layer() {
            self.anim.set_layer(i, None, now);
        }
        before != self.anim.frame() || self.voice.mic.is_some()
    }

    pub(super) fn voice_deadline(&self, now: Instant) -> Option<Instant> {
        if let Some(l) = &self.voice.lips {
            let k = now.saturating_duration_since(l.t0).as_millis() / l.step.as_millis().max(1) + 1;
            if (k as usize) <= l.shapes.len() + 2 {
                return Some(l.t0 + l.step * k as u32);
            }
        }
        // The MIC timer counts seconds.
        if let Some(m) = self.voice.mic {
            return Some(m + Duration::from_secs(now.saturating_duration_since(m).as_secs() + 1));
        }
        self.voice.retry_at.filter(|_| self.voice_on() && self.voice.link.is_none())
    }

    /// The tag while the mic is open, he waits for the words, or she speaks.
    pub(super) fn voice_status(&self) -> Option<Status> {
        let bars = |v: f32| -> String { (0..5).map(|i| if v * 5.0 > i as f32 + 0.3 { '▮' } else { '▯' }).collect() };
        if let Some(m) = self.voice.mic {
            let s = m.elapsed().as_secs();
            return Some(Status { label: format!("MIC {}:{:02} {}", s / 60, s % 60, bars(self.voice.level)), tone: 0xff3b62, invert: true, glyph: "rec".into() });
        }
        if self.voice.asked.is_some() {
            return Some(Status { label: "MIC …".into(), tone: 0xff3b62, invert: false, glyph: "ring".into() });
        }
        if self.voice.transcribing {
            return Some(Status { label: "HEARD · …".into(), tone: self.pack.tone(Some("think")), invert: false, glyph: "dots".into() });
        }
        if self.voice_speaking() {
            let s = self.voice_shape(Instant::now()).unwrap_or(0);
            return Some(Status { label: format!("SPEAKING {}", bars(s as f32 / 3.0)), tone: self.pack.tone(Some("speak")), invert: false, glyph: "play".into() });
        }
        None
    }

    /// The Line's mic row: the tag's label while held, then what follows key-up.
    pub(super) fn voice_mic_row(&self) -> Option<String> {
        if self.voice_listening() {
            return self.voice_status().map(|s| format!("● {}  ·  listening · release to send · esc to cancel", s.label));
        }
        self.voice.transcribing.then(|| "◉ HEARD  ·  writing it down (nothing is kept)".to_string())
    }

    /// The sentence she is saying now.
    pub(super) fn voice_caption_now(&self) -> Option<String> {
        self.voice.lips.as_ref()?;
        self.voice.caption.clone()
    }

    pub(super) fn voice_state_json(&self) -> serde_json::Value {
        serde_json::json!({
            "enabled": self.voice.cfg.enabled, "connected": self.voice.link.is_some(),
            "mic_open": self.voice.mic.is_some(), "listening": self.voice_listening(), "transcribing": self.voice.transcribing,
            "speaking": self.voice_speaking(), "turn": self.voice.turn, "prev": self.voice.prev, "prev_left": self.voice.prev_left,
            "unprompted": self.voice.unprompted, "sent": self.voice.sent, "ended": self.voice.ended,
            "speak_unprompted": self.voice.cfg.speak_unprompted, "sidecar_unprompted": self.voice.can_unprompted,
            "shape": self.voice_shape(Instant::now()), "held": self.voice.held, "repeats_ignored": self.voice.repeats,
            "stats": serde_json::Value::Object(self.voice.stats.clone()),
        })
    }
}
