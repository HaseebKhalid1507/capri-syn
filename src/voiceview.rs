//! Voice, from the avatar's side: push-to-talk on the talk key, toggle-to-talk (`talk toggle`:
//! one tap, the turn ends on his pause), the `capri-voice` sidecar link,
//! the truthful `● MIC` tag, spoken replies (his spoken turns, and her replies to what arrives
//! on its own) with captions and lip-sync, barge-in. A child module of `view`. The sidecar owns
//! the mic and the models; nothing here touches audio.

use super::*;
use crate::settings::VoiceSettings;
use crate::voice::{self, Said, Speech};
use std::io::{BufRead, Write};

/// Key-up waits this long before it counts: X auto-repeat sends release/press pairs.
const UP_DEBOUNCE: Duration = Duration::from_millis(60);
/// Other audio comes back this long after her last word.
const RESTORE_MS: u64 = 800;
/// With voice on, a missing sidecar is looked for again this often (connecting opens no mic).
const RECONNECT: Duration = Duration::from_secs(10);
/// Where connecting runs on a thread: after a failed one, the next waits this long (a stale
/// endpoint costs ~2 s of a thread each time; the wake word asks every tick).
const CONNECT_AGAIN: Duration = Duration::from_secs(2);
/// The toggle chord's key is polled this often until it is up: its auto-repeat is one toggle.
const KEY_POLL: Duration = Duration::from_millis(30);
/// No key state: this long without a toggle command counts as the key up.
const REPEAT_QUIET: Duration = Duration::from_millis(300);
/// While a toggled mic is open, the sidecar is asked this often whether it really is (it wins).
const MIC_CHECK: Duration = Duration::from_secs(2);
/// Not drawn this long for a reason other than hiding (a teleport): a hands-off mic closes.
const HIDDEN_GRACE: Duration = Duration::from_secs(2);

/// A second tap this soon after the first, before he spoke: hands-free instead.
const DOUBLE_TAP: Duration = Duration::from_millis(400);
/// The quiet countdown shows on her tag for its last seconds.
const HF_WARN: Duration = Duration::from_secs(10);

/// A hands-free conversation: one stream, many turns, until he ends it or it goes quiet.
#[derive(Clone, Copy, Debug)]
pub(super) struct HandsFree {
    pub since: Instant,
    /// The quiet clock: restarts at his accepted turn and at her last word; runs only while
    /// nothing is going on (she isn't thinking or speaking, nothing is being written down).
    quiet_since: Instant,
    /// The sidecar heard him start an utterance (a reply to an event waits).
    hearing: bool,
    /// The stream closed; the sidecar's reason (`handsfree-end`) may still come.
    closed: bool,
    /// "Hey Capri" started it (the wake stream went on as this one; it listens again after).
    wake: bool,
}

/// The wake word ("hey capri", on by default; `[voice] wake_word`): its standing stream, asked for while
/// the setting is on and nothing keeps it closed. Its tag is static (no timer, no meter).
#[derive(Clone, Debug, Default)]
pub(super) struct Wake {
    /// `listen-start wake` sent, not yet answered.
    asked: Option<Instant>,
    /// Its stream is open, since.
    open: Option<Instant>,
    /// "Hey Capri" was heard and checked: his turn is being taken (one turn: `wake_handsfree = false`).
    turn: bool,
    /// Why it is closed for now (her tag says so), and when to ask again.
    paused: Option<String>,
    retry_at: Option<Instant>,
    /// Its stream died: not reopened on whatever the default source is now (a settings reload
    /// or a new sidecar clears it).
    lost: bool,
    /// Lost only because a hands-free that "hey capri" began closed (MIC LOST, for now): its
    /// real reason (`handsfree-end`) may still come and undo it.
    lost_hf: bool,
}

/// Asked for, not answered: ask again after this long. Closed for a call or the lock: ask again this often.
const WAKE_RETRY: Duration = Duration::from_secs(3);

/// A toggled turn (`talk toggle`): open until his pause, a second tap, Esc or `talk cancel`.
#[derive(Clone, Copy, Debug)]
pub(super) struct Toggle {
    pub at: Instant,
    /// This tap opened (or summoned) the Line, so the keyboard goes back when the turn ends.
    opened_line: bool,
    /// The sidecar heard him start (`speech`): a second tap sends now; before it, cancels.
    hearing: bool,
}

pub(super) struct Lips {
    t0: Instant,
    step: Duration,
    shapes: Vec<u8>,
}

#[derive(Default)]
pub(super) struct VoiceState {
    pub cfg: VoiceSettings,
    link: Option<crate::platform::VoiceLink>,
    generation: u64,
    /// listen-start sent (the hold), not yet stopped.
    asked: Option<Instant>,
    /// The sidecar says the capture stream is open, since.
    pub mic: Option<Instant>,
    level: f32,
    transcribing: bool,
    /// The next new turn is the one he spoke.
    pending: bool,
    /// ...in hands-free.
    hf_pending: bool,
    /// The spoken turn, and what of it is said.
    turn: Option<u64>,
    /// It arrived on its own (not his): what keeps her quiet is checked all along.
    unprompted: bool,
    /// The turn before it, still playing (an unprompted turn waits behind it), and how many of
    /// its sentences are still out. At most one turn waits: a newer one replaces it.
    prev: Option<u64>,
    prev_left: u32,
    prev_unprompted: bool,
    /// ...and it is still being heard whole (`Said::heard`): said to its end, it is read.
    prev_whole: bool,
    /// His reply plays on under a newer, silent turn's bubble: its captions stay out of it.
    orphan: bool,
    /// The sidecar can gate unprompted speech (calls, lock, budget): its `ready` says so. Without
    /// it, unprompted replies stay silent (fail closed).
    can_unprompted: bool,
    /// The next look for a missing sidecar; its first miss is logged, not every retry.
    retry_at: Option<Instant>,
    missed: bool,
    /// Where connecting can block (`platform::VOICE_LINK_BLOCKS`: Windows), it runs on a thread:
    /// one at a time, and not again before `connect_after` once one failed.
    connecting: bool,
    connect_after: Option<Instant>,
    speech: Speech,
    /// The spoken turn's sentences: sent, said, done, cut (heard whole, it is read).
    said: Said,
    reply: String,
    lips: Option<Lips>,
    caption: Option<String>,
    /// Auto-repeat releases seen while the key stayed down (ignored).
    repeats: u32,
    held: bool,
    /// A key-up waiting out the auto-repeat window.
    up_at: Option<Instant>,
    /// A toggled turn, while its mic is asked for or open.
    pub toggle: Option<Toggle>,
    /// A hands-free conversation, while it lasts.
    pub hf: Option<HandsFree>,
    /// The wake word's stream (a draft, off by default).
    pub wake: Wake,
    /// A session turn is running (she is thinking or writing).
    busy: bool,
    /// A toggle was taken and its key is not yet seen up: more toggles are its auto-repeat.
    toggle_lock: bool,
    toggles_ignored: u32,
    toggle_last: Option<Instant>,
    /// She isn't drawn (any reason), since.
    hidden_since: Option<Instant>,
    mic_check_at: Option<Instant>,
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

impl Avatar {
    pub(super) fn voice_on(&self) -> bool {
        self.voice.cfg.enabled
    }

    /// The wake word's mic stream is open, idle (her tag's dot becomes a mic: the always-on mic stays visible).
    pub(super) fn voice_wake_idle(&self) -> bool {
        self.voice.wake.open.is_some()
    }

    pub(super) fn voice_speaking(&self) -> bool {
        self.voice.turn.is_some() && (self.voice.said.left() > 0 || self.voice.prev_left > 0)
    }

    pub(super) fn voice_listening(&self) -> bool {
        self.voice.asked.is_some() || self.voice.mic.is_some()
    }

    /// Connect to the sidecar if not yet (Linux: a systemd user unit runs it; Windows: she does,
    /// `voice_keepalive`). Where connecting can block (Windows: a connect and a hello, up to ~2 s on a
    /// stale endpoint), it goes to a thread and this answers false until it is through; Linux
    /// connects here, inline, as before.
    pub(super) fn voice_connect(&mut self, cx: &mut Context<Self>) -> bool {
        if self.voice.link.is_some() {
            return true;
        }
        if crate::platform::VOICE_LINK_BLOCKS {
            self.voice_connect_bg(cx);
            return false;
        }
        let (path, s) = crate::platform::voice_link();
        self.voice_linked(path, s, cx)
    }

    /// The blocking connect on a thread; its answer comes back here (`voice_linked`).
    fn voice_connect_bg(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        if self.voice.connecting || self.voice.connect_after.is_some_and(|t| now < t) {
            return;
        }
        let (tx, rx) = async_channel::bounded(1);
        if let Err(e) = crate::platform::voice_link_bg(move |path, s| {
            tx.send_blocking((path, s)).ok();
        }) {
            elog!("synaps-avatar: voice: no connect thread ({e})");
            self.voice.connect_after = Some(now + CONNECT_AGAIN);
            return;
        }
        self.voice.connecting = true;
        cx.spawn(async move |this, cx| {
            let Ok((path, s)) = rx.recv().await else { return };
            this.update(cx, |a, cx| {
                a.voice.connecting = false;
                if a.voice.link.is_none() {
                    if !a.voice_linked(path, s, cx) {
                        a.voice.connect_after = Some(Instant::now() + CONNECT_AGAIN);
                    }
                    a.voice_changed(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// A connect's answer: the link set up (true), or the miss logged once.
    fn voice_linked(&mut self, path: String, s: std::io::Result<crate::platform::VoiceLink>, cx: &mut Context<Self>) -> bool {
        let s = match s {
            Ok(s) => s,
            Err(e) => {
                if !std::mem::replace(&mut self.voice.missed, true) {
                    elog!("synaps-avatar: voice: no sidecar at {path} ({e}); looking again every {} s", RECONNECT.as_secs());
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
        elog!("synaps-avatar: voice: sidecar connected ({path})");
        self.voice.link.is_some()
    }

    /// With voice on, the sidecar is kept connected (at start, and after it went away), so a
    /// reply to an event can be heard before he ever pressed the talk key.
    pub(super) fn voice_keepalive(&mut self, now: Instant, cx: &mut Context<Self>) {
        // Windows: she runs the sidecar while voice is on (Linux: systemd does; nothing here).
        crate::platform::voice_sidecar(self.voice_on());
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
        // New settings take a fresh wake stream (a new sensitivity; the word back on after a lost one).
        let w = &mut self.voice.wake;
        w.lost = false;
        w.lost_hf = false;
        w.retry_at = None;
        if w.open.is_some() && !w.turn {
            w.open = None;
            self.voice_send(serde_json::json!({ "op": "listen-cancel" }));
        }
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
        self.voice.wake = Wake::default();
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
        if !self.voice_on() || self.voice.hf.is_some() {
            return hushed;          // hands-free already listens: the talk key only cuts her off
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
    /// False if the key is still down (an X auto-repeat's release): ignored. `physical` false
    /// (a tap's release, `talk tap`): the key isn't asked.
    pub(super) fn voice_up(&mut self, physical: bool, cx: &mut Context<Self>) -> bool {
        if physical && self.talk_down.is_some() && self.os.char_key_down(self.voice.cfg.key).unwrap_or(false) {
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
        if self.voice.hf.is_some() {
            return;                 // hands-free's stream isn't the talk key's
        }
        if self.voice.asked.take().is_none() {
            return;
        }
        self.voice.keyup = Some(at);
        self.voice.transcribing = true;
        self.voice_send(serde_json::json!({ "op": "listen-stop" }));
        elog!("synaps-avatar: voice: key up: transcribing");
        self.voice_changed(cx);
    }

    // ---- toggle-to-talk --------------------------------------------------------------------

    /// `talk toggle` (`$mod+Ctrl+c`, on press): open the mic, or end the turn (send now once he
    /// has started, else cancel). One toggle per key-down: its auto-repeat is ignored until
    /// the chord's key is seen up. Ignored while the talk key is held.
    pub(super) fn voice_toggle(&mut self, cx: &mut Context<Self>) -> String {
        self.voice.toggle_last = Some(Instant::now());
        if self.voice.toggle_lock {
            self.voice.toggles_ignored += 1;
            return "ok held (one toggle per press)".into();
        }
        if self.talk_down.is_some() {
            return "ok ignored (the talk key is held)".into();
        }
        self.voice_toggle_lock(cx);
        if self.voice.hf.is_some() {
            self.voice_hf_end("HANDS-FREE OFF", cx);
            return "ok hands-free off".into();
        }
        if let Some(t) = self.voice.toggle
            && !t.hearing
            && t.at.elapsed() <= DOUBLE_TAP
            && self.voice.cfg.handsfree
        {
            // The double-tap: the toggle's stream gives way to a hands-free one.
            self.voice_send(serde_json::json!({ "op": "listen-cancel" }));
            self.voice.asked = None;
            self.voice.mic = None;
            self.voice_toggle_end(cx);
            return self.voice_hf_start(cx);
        }
        if self.voice.toggle.is_some() {
            return self.voice_toggle_tap(cx);
        }
        self.voice_toggle_start(cx)
    }

    // ---- the wake word (a draft) ------------------------------------------------------------

    /// Another capture of his is asked for or open (the sidecar closes the wake stream for it).
    fn voice_other_mic(&self) -> bool {
        self.voice.toggle.is_some() || self.voice.hf.is_some() || self.talk_down.is_some() || self.voice.asked.is_some()
            || (self.voice.mic.is_some() && !self.voice.wake.turn)
    }

    /// Each tick: the wake stream is asked for while his setting is on and nothing keeps it
    /// closed; a secret on screen or her hidden closes it (and its turn) at once.
    fn voice_wake_tick(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let want = self.voice.cfg.wake_word && self.voice_on();
        let block = if want { self.voice_mic_refusal(true).map(|b| b.replace("NO MIC", "WAKE PAUSED")) } else { None };
        let w = &mut self.voice.wake;
        if w.asked.is_some_and(|a| now - a >= WAKE_RETRY) {
            w.asked = None;          // never answered (closed for another capture before it opened)
        }
        let stream = w.open.is_some() || w.asked.is_some();
        if stream && (!want || block.is_some()) {
            self.voice_send(serde_json::json!({ "op": "listen-cancel" }));
            self.voice_wake_closed(cx);
            elog!("synaps-avatar: voice: wake word closed ({})", block.as_deref().unwrap_or("off"));
        }
        let w = &mut self.voice.wake;
        if w.paused != block && (block.is_some() || w.paused.as_deref().is_some_and(|p| p.contains("SECRET") || p.contains("HIDDEN"))) {
            w.paused = block.clone();
            self.voice_changed(cx);
        }
        let w = &self.voice.wake;
        if !want || block.is_some() || w.lost || w.open.is_some() || w.asked.is_some() || w.retry_at.is_some_and(|t| now < t) || self.voice_other_mic() {
            return;
        }
        if !self.voice_connect(cx) {
            return;
        }
        let c = &self.voice.cfg;
        let sens = (f64::from(c.wake_sensitivity) * 100.0).round() / 100.0;
        // "Hey Capri" goes on as a hands-free conversation (its request the first turn), unless he said no.
        let hf = c.wake_handsfree && c.handsfree;
        let max_s = if c.handsfree_max == 0 { 86_400 } else { c.handsfree_max * 60 };
        let msg = serde_json::json!({ "op": "listen-start", "mode": "wake", "sensitivity": sens, "silence_s": c.silence_s, "echo": c.echo,
                                      "handsfree": hf, "max_s": max_s });
        self.voice_send(msg);
        self.voice.wake.asked = Some(now);
        self.voice.wake.retry_at = None;
    }

    /// The wake stream is gone (closed by us, or the sidecar said so): its turn too.
    fn voice_wake_closed(&mut self, cx: &mut Context<Self>) {
        let w = &mut self.voice.wake;
        w.open = None;
        w.asked = None;
        if std::mem::take(&mut w.turn) {
            self.voice.mic = None;
            self.voice.level = 0.0;
            // (the capture that closed it holds other audio itself)
            if self.voice.held && !self.voice_speaking() && !self.voice_other_mic() {
                self.voice.held = false;
                self.voice_send(serde_json::json!({ "op": "media-release", "after_ms": RESTORE_MS }));
            }
        }
        self.voice_changed(cx);
    }

    // ---- hands-free -----------------------------------------------------------------------

    /// `talk handsfree on`, the double-tap: the mic stays open across turns. Always off at launch.
    pub(super) fn voice_hf_start(&mut self, cx: &mut Context<Self>) -> String {
        let now = Instant::now();
        if self.voice.hf.is_some() {
            return "ok hands-free already".into();
        }
        if !self.voice.cfg.handsfree {
            self.hint = Some(("HANDS-FREE IS OFF IN SETTINGS".into(), now + Duration::from_secs(3)));
            self.voice_changed(cx);
            return "error: [voice] handsfree = false".into();
        }
        if self.voice.toggle.is_some() || self.voice.asked.is_some() {
            self.voice_cancel(cx);
        }
        if self.voice_speaking() {
            self.voice_hush("hands-free");
        }
        if let Some(why) = self.voice_mic_refusal(true) {
            self.hint = Some((why.into(), now + Duration::from_secs(3)));
            self.voice_changed(cx);
            return format!("error: {why}");
        }
        if !self.voice_connect(cx) {
            self.hint = Some(("VOICE OFF · START capri-voice".into(), now + Duration::from_secs(4)));
            self.voice_changed(cx);
            return "error: no sidecar".into();
        }
        let c = &self.voice.cfg;
        let max_s = if c.handsfree_max == 0 { 86_400 } else { c.handsfree_max * 60 };
        let (mode, silence_s, echo) = (c.duck.clone(), c.silence_s, c.echo.clone());
        self.voice_send(serde_json::json!({ "op": "media-hold", "mode": mode }));
        self.voice.held = true;
        self.voice_send(serde_json::json!({ "op": "listen-start", "mode": "handsfree", "silence_s": silence_s, "max_s": max_s, "echo": echo }));
        self.voice.asked = Some(now);
        self.voice.keyup = None;
        self.voice.hf = Some(HandsFree { since: now, quiet_since: now, hearing: false, closed: false, wake: false });
        self.voice.mic_check_at = Some(now + MIC_CHECK);
        elog!("synaps-avatar: voice: hands-free on (ends after {} s quiet)", self.voice.cfg.handsfree_timeout);
        self.voice_changed(cx);
        "ok hands-free on".into()
    }

    /// Hands-free ends: the stream closes (if the sidecar hasn't already), other audio comes back
    /// after her last word, and her tag says why.
    pub(super) fn voice_hf_end(&mut self, why: &str, cx: &mut Context<Self>) {
        let Some(h) = self.voice.hf.take() else { return };
        if !h.closed {
            self.voice_send(serde_json::json!({ "op": "listen-cancel" }));
        }
        self.voice.asked = None;
        self.voice.mic = None;
        self.voice.mic_check_at = None;
        if self.voice.held && !self.voice_speaking() {
            self.voice.held = false;
            self.voice_send(serde_json::json!({ "op": "media-release", "after_ms": RESTORE_MS }));
        }
        elog!("synaps-avatar: voice: {} after {} s{}", why.to_lowercase(), h.since.elapsed().as_secs(), if h.wake { " (it began with \"hey capri\")" } else { "" });
        self.hint = Some((why.to_string(), Instant::now() + Duration::from_secs(4)));
        // The wake word listens again now, however it ended (not on whatever source is default now if the
        // stream that began with "hey capri" died: that is the wake stream's own rule).
        if h.wake && why.contains("MIC LOST") && !self.voice.wake.lost {
            let w = &mut self.voice.wake;
            w.lost = true;
            w.lost_hf = true;
            w.paused = Some("WAKE OFF · MIC LOST".into());
        }
        self.voice_wake_tick(cx);
        self.voice_changed(cx);
    }

    /// The quiet clock and the cap. Each tick. The clock restarts after anything of hers (she is
    /// thinking, speaking, writing his words down); his voice alone doesn't restart it (a TV must
    /// not keep the mic open), but it never ends mid-utterance.
    fn voice_hf_clock(&mut self, cx: &mut Context<Self>) {
        let Some(h) = &mut self.voice.hf else { return };
        if self.voice.busy || self.voice.transcribing || self.voice.turn.is_some() {
            h.quiet_since = Instant::now();
            return;
        }
        if h.hearing {
            return;
        }
        let max = self.voice.cfg.handsfree_max;
        if max > 0 && h.since.elapsed() >= Duration::from_secs(max as u64 * 60) {
            self.voice_hf_end(&format!("HANDS-FREE OFF · {max} MIN"), cx);
        } else if h.quiet_since.elapsed() >= Duration::from_secs(self.voice.cfg.handsfree_timeout as u64) {
            let t = self.voice.cfg.handsfree_timeout;
            self.voice_hf_end(&format!("HANDS-FREE OFF · {t} S QUIET"), cx);
        }
    }

    /// Seconds of quiet left, when the countdown shows.
    fn voice_hf_left(&self) -> Option<u64> {
        let h = self.voice.hf?;
        let left = Duration::from_secs(self.voice.cfg.handsfree_timeout as u64).saturating_sub(h.quiet_since.elapsed());
        (left <= HF_WARN && !self.voice.busy && self.voice.turn.is_none()).then(|| left.as_secs() + u64::from(left.subsec_nanos() > 0))
    }

    /// Ignore toggles until the chord's key is up (polled; X has no key-up for i3's bindings).
    /// If the key state can't be read, up = 300 ms without a toggle (auto-repeat sends ~30/s).
    fn voice_toggle_lock(&mut self, cx: &mut Context<Self>) {
        self.voice.toggle_lock = true;
        let key = self.voice.cfg.toggle_key;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(KEY_POLL).await;
                let up = this
                    .update(cx, |a, _| match a.os.char_key_down(key) {
                        Ok(down) => !down,
                        Err(_) => a.voice.toggle_last.is_none_or(|t| t.elapsed() >= REPEAT_QUIET),
                    })
                    .unwrap_or(true);
                if up {
                    this.update(cx, |a, _| a.voice.toggle_lock = false).ok();
                    return;
                }
            }
        })
        .detach();
    }

    /// Why the mic may not open now, if it may not (the reason, for her tag).
    fn voice_mic_refusal(&self, hands_off: bool) -> Option<&'static str> {
        if !self.voice_on() {
            return Some("VOICE OFF · [voice] enabled = false");
        }
        // Approvals leave the mic usable (nothing spoken answers them); a secret never.
        if self.prompt_ui.as_ref().is_some_and(|p| p.view.secret) || self.input.as_ref().is_some_and(|i| i.secret.is_some()) {
            return Some("NO MIC · A SECRET IS ON SCREEN");
        }
        // Her tag must be on screen while a hands-off mic is open. A teleport (she moves; the
        // Line's opening moves her too) is not hiding unless it lasts.
        if hands_off {
            match self.hidden_reason() {
                Some("user" | "fullscreen") => return Some("NO MIC · SHE IS HIDDEN"),
                Some(_) if self.voice.hidden_since.is_some_and(|t| t.elapsed() >= HIDDEN_GRACE) => return Some("NO MIC · SHE IS HIDDEN"),
                _ => {}
            }
        }
        None
    }

    fn voice_toggle_start(&mut self, cx: &mut Context<Self>) -> String {
        let now = Instant::now();
        if self.voice_speaking() {
            self.voice_hush("talk toggle");
        }
        if let Some(why) = self.voice_mic_refusal(true) {
            self.hint = Some((why.into(), now + Duration::from_secs(3)));
            self.voice_changed(cx);
            return format!("error: {why}");
        }
        if !self.voice_connect(cx) {
            self.hint = Some(("VOICE OFF · START capri-voice".into(), now + Duration::from_secs(4)));
            self.voice_changed(cx);
            return "error: no sidecar".into();
        }
        // The Line opens (or comes back) with the keyboard, so Esc cancels; it goes back after.
        let opened_line = match &self.line {
            None => {
                self.open_line(cx);
                self.line.is_some()
            }
            Some(l) if l.parked => {
                let (view, handle) = (l.view.clone(), l.handle);
                cx.defer(move |cx| {
                    handle.update(cx, |_, window, cx| view.update(cx, |l, cx| l.summon(window, cx))).ok();
                });
                true
            }
            Some(_) => false,
        };
        let mode = self.voice.cfg.duck.clone();
        self.voice_send(serde_json::json!({ "op": "media-hold", "mode": mode }));
        self.voice.held = true;
        self.voice_send(serde_json::json!({ "op": "listen-start", "mode": "toggle", "silence_s": self.voice.cfg.silence_s }));
        self.voice.asked = Some(now);
        self.voice.keyup = None;
        self.voice.toggle = Some(Toggle { at: now, opened_line, hearing: false });
        self.voice.mic_check_at = Some(now + MIC_CHECK);
        self.brain_touch(crate::brain::Touch::Typing);
        elog!("synaps-avatar: voice: toggle: listening (ends on a {:.1} s pause)", self.voice.cfg.silence_s);
        self.voice_changed(cx);
        "ok toggle on".into()
    }

    /// The talk key pressed while a toggled mic is open: the same as a second tap.
    pub(super) fn voice_toggle_tap_key(&mut self, cx: &mut Context<Self>) -> String {
        self.voice_toggle_tap(cx)
    }

    /// The second tap: send now once he has started; before that, cancel.
    fn voice_toggle_tap(&mut self, cx: &mut Context<Self>) -> String {
        let Some(t) = self.voice.toggle else { return "ok".into() };
        if !t.hearing {
            self.voice_cancel(cx);
            return "ok toggle cancelled".into();
        }
        self.voice.asked = None;
        self.voice.keyup = Some(Instant::now());
        self.voice.transcribing = true;
        self.voice_send(serde_json::json!({ "op": "listen-stop" }));
        self.voice_toggle_end(cx);
        elog!("synaps-avatar: voice: toggle: second tap: sending");
        self.voice_changed(cx);
        "ok toggle sent".into()
    }

    /// The toggled turn is over: the keyboard goes back if its tap took it.
    fn voice_toggle_end(&mut self, cx: &mut Context<Self>) {
        let Some(t) = self.voice.toggle.take() else { return };
        self.voice.mic_check_at = None;
        if !t.opened_line {
            return;
        }
        let Some(l) = &mut self.line else { return };
        if l.parked {
            return;
        }
        l.parked = true;
        self.os.arm_clicks(false);
        if std::env::var_os("SA_READER_NO_RESTORE").is_none()
            && let Some(p) = l.prev
        {
            self.os.release_keyboard(l.xid, Some(p)).ok();
        }
        let (view, handle) = (l.view.clone(), l.handle);
        cx.defer(move |cx| {
            handle.update(cx, |_, _, cx| view.update(cx, |l, cx| l.park(cx))).ok();
        });
    }

    /// A secret on screen closes any open mic; hiding closes a toggled one. Each tick.
    fn voice_mic_guard(&mut self, cx: &mut Context<Self>) {
        match (self.hidden_reason(), self.voice.hidden_since) {
            (Some(_), None) => self.voice.hidden_since = Some(Instant::now()),
            (None, Some(_)) => self.voice.hidden_since = None,
            _ => {}
        }
        if !self.voice_listening() {
            return;
        }
        let hands_off = self.voice.toggle.is_some() || self.voice.hf.is_some();
        if let Some(why) = self.voice_mic_refusal(hands_off) {
            if self.voice.hf.is_some() {
                let why = why.replace("NO MIC · ", "HANDS-FREE OFF · ").replace("SHE IS HIDDEN", "HIDDEN");
                self.voice_hf_end(&why, cx);
                return;
            }
            let why = why.replace("NO MIC", "MIC CLOSED");
            elog!("synaps-avatar: voice: {why}");
            self.voice_cancel(cx);
            self.hint = Some((why, Instant::now() + Duration::from_secs(3)));
        }
        self.voice_hf_clock(cx);
        // The sidecar is asked now and then whether a toggled mic is open; it wins.
        if let Some(t) = self.voice.mic_check_at
            && Instant::now() >= t
        {
            self.voice.mic_check_at = Some(Instant::now() + MIC_CHECK);
            self.voice_send(serde_json::json!({ "op": "stats" }));
        }
    }

    /// Esc in hands-free: what is being heard now is dropped; the conversation stays.
    pub(super) fn voice_esc(&mut self, cx: &mut Context<Self>) {
        if let Some(h) = &mut self.voice.hf {
            h.hearing = false;
            self.voice_send(serde_json::json!({ "op": "listen-drop" }));
            self.hint = Some(("DROPPED · STILL LISTENING".into(), Instant::now() + Duration::from_secs(3)));
            self.voice_changed(cx);
            return;
        }
        self.voice_cancel(cx);
    }

    /// Esc while holding (or toggled), `talk cancel`: nothing is transcribed or sent.
    pub(super) fn voice_cancel(&mut self, cx: &mut Context<Self>) {
        if self.voice.hf.is_some() {
            self.voice_hf_end("HANDS-FREE OFF", cx);
            return;
        }
        if self.voice.asked.take().is_some() || self.voice.mic.is_some() {
            self.voice_send(serde_json::json!({ "op": "listen-cancel" }));
            self.voice_send(serde_json::json!({ "op": "media-release" }));
            self.voice.held = false;
            self.hint = Some(("CANCELLED · NOTHING SENT".into(), Instant::now() + Duration::from_secs(3)));
            self.voice_toggle_end(cx);
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

    /// She stops (hushed, his voice over hers, the sidecar gone): what she was saying is cut,
    /// so it is not heard whole and stays unread (as is the turn waiting behind her).
    fn voice_stop_speech(&mut self) {
        let was = self.voice.turn.is_some();
        if let Some(id) = self.voice.turn.filter(|_| self.voice.said.sent > 0) {
            elog!("synaps-avatar: voice: turn {id} cut ({} of {} said): stays unread", self.voice.said.ended, self.voice.said.sent);
        }
        self.voice.speech = Speech::default();
        self.voice.speech.feed("", true);
        self.voice.said = Said::default();
        self.voice.lips = None;
        self.voice.prev = None;
        self.voice.prev_left = 0;
        self.voice.prev_unprompted = false;
        self.voice.prev_whole = false;
        self.voice.unprompted = false;
        if was {
            self.voice.turn = None;
            self.voice_caption_done();
        }
        if self.voice.held && self.voice.hf.is_none() {
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
            "mic-open" if v.get("mode").and_then(|m| m.as_str()) == Some("wake") => {
                let w = &mut self.voice.wake;
                w.open = Some(now);
                w.asked = None;
                w.paused = None;
                elog!("synaps-avatar: voice: wake word listening (sensitivity {})", self.voice.cfg.wake_sensitivity);
                self.voice_changed(cx);
            }
            "mic-closed" if v.get("mode").and_then(|m| m.as_str()) == Some("wake") => {
                // "Hey Capri" went on as hands-free, then the wake stream closed after all (cancelled at that moment):
                // there is no stream; hands-free ends (and the wake word is asked for again).
                if let Some(h) = self.voice.hf.as_mut().filter(|h| h.wake && !h.closed) {
                    h.closed = true;
                    self.voice_hf_end("HANDS-FREE OFF", cx);
                    return;
                }
                // Refused before it opened: the sidecar's `heard` with the reason follows.
                if self.voice.wake.open.is_some() || self.voice.wake.turn {
                    self.voice_wake_closed(cx);
                }
            }
            // "Hey Capri", checked: the stream goes on as a hands-free conversation (`handsfree`: its request, if he
            // said one, is the first turn); else his one turn is taken as a toggle's (it ends on his pause).
            "wake" => {
                let w = &mut self.voice.wake;
                if w.open.is_none() {
                    return;
                }
                let hf = v.get("handsfree").and_then(|x| x.as_bool()) == Some(true);
                if hf {
                    w.open = None;
                    w.asked = None;
                    w.paused = None;
                } else {
                    w.turn = true;
                }
                self.voice.mic = Some(now);
                self.voice.keyup = None;
                if let Some(ms) = v.get("ms").and_then(|m| m.as_object()) {
                    for (k, x) in ms {
                        self.voice.stats.insert(format!("wake_{k}"), x.clone());
                    }
                }
                if self.voice_speaking() {
                    self.voice_stop_speech();
                }
                let mode = self.voice.cfg.duck.clone();
                self.voice_send(serde_json::json!({ "op": "media-hold", "mode": mode }));
                self.voice.held = true;
                if hf {
                    self.voice.hf = Some(HandsFree { since: now, quiet_since: now, hearing: false, closed: false, wake: true });
                    self.voice.mic_check_at = Some(now + MIC_CHECK);
                    elog!("synaps-avatar: voice: wake word: \"hey capri\": hands-free (ends after {} s quiet)", self.voice.cfg.handsfree_timeout);
                    // Turned off in the same moment (settings): it ends at once. A secret or her hiding: the mic guard.
                    if !(self.voice.cfg.handsfree && self.voice.cfg.wake_handsfree) {
                        self.voice_hf_end("HANDS-FREE OFF", cx);
                        return;
                    }
                } else {
                    elog!("synaps-avatar: voice: wake word: \"hey capri\"");
                }
                self.voice_changed(cx);
            }
            "wake-handsfree" => {
                self.voice.wake.turn = false;
                self.voice.mic = None;
                self.voice.transcribing = false;
                elog!("synaps-avatar: voice: wake word: let's talk");
                let r = self.voice_hf_start(cx);
                if !r.starts_with("ok") {
                    self.voice_send(serde_json::json!({ "op": "media-release" }));
                    self.voice.held = false;
                } else if let Some(h) = &mut self.voice.hf {
                    h.wake = true;
                }
            }
            "wake-end" => {
                let why = v.get("why").and_then(|x| x.as_str()).unwrap_or("");
                let w = &mut self.voice.wake;
                let (label, again) = match why {
                    "call" => ("WAKE PAUSED · IN A CALL", Some(WAKE_RETRY)),
                    "lock" => ("WAKE PAUSED · SCREEN LOCKED", Some(WAKE_RETRY)),
                    "false starts" => ("WAKE PAUSED · TOO MANY FALSE STARTS", Some(Duration::from_secs(600))),
                    _ => ("WAKE OFF · MIC LOST", None),
                };
                w.paused = Some(label.into());
                w.retry_at = again.map(|d| now + d);
                w.lost = again.is_none();
                elog!("synaps-avatar: voice: wake word ended ({why})");
                self.voice_wake_closed(cx);
            }
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
                let mode = v.get("mode").and_then(|m| m.as_str()).unwrap_or("handsfree");
                if let Some(h) = &mut self.voice.hf
                    && mode == "handsfree"
                {
                    // Its reason (`handsfree-end`) follows if the sidecar ended it; else it died.
                    h.closed = true;
                    self.voice_hf_end("HANDS-FREE OFF · MIC LOST", cx);
                    return;
                }
                if self.voice.hf.is_some() {
                    // The toggle's stream, closing as hands-free takes over.
                    self.voice_changed(cx);
                    return;
                }
                // A toggled stream that ended on its own (refused, the lock, the hard cap): the
                // sidecar's `heard` follows.
                if self.voice.toggle.is_some() && self.voice.asked.take().is_some() {
                    self.voice.transcribing = true;
                    self.voice_toggle_end(cx);
                }
                self.voice_changed(cx);
            }
            "speech" => {
                if let Some(t) = &mut self.voice.toggle {
                    t.hearing = true;
                }
                if let Some(h) = &mut self.voice.hf {
                    h.hearing = true;
                }
            }
            "handsfree-end" => {
                let why = match v.get("why").and_then(|x| x.as_str()).unwrap_or("") {
                    "call" => "HANDS-FREE OFF · IN A CALL".to_string(),
                    "lock" => "HANDS-FREE ENDED AT LOCK".to_string(),
                    "max" => format!("HANDS-FREE OFF · {} MIN", self.voice.cfg.handsfree_max),
                    _ => "HANDS-FREE OFF · MIC LOST".to_string(),
                };
                if self.voice.hf.is_some() {
                    self.voice.hf.as_mut().unwrap().closed = true;
                    self.voice_hf_end(&why, cx);
                } else {
                    // mic-closed already ended it as MIC LOST: the real reason replaces the hint (and the wake
                    // word, kept off for a lost mic, is asked for again: a lock or a call refuse it for now).
                    let lost = why.contains("MIC LOST");
                    self.hint = Some((why, now + Duration::from_secs(4)));
                    let w = &mut self.voice.wake;
                    if !lost && std::mem::take(&mut w.lost_hf) {
                        w.lost = false;
                        w.paused = None;
                        self.voice_wake_tick(cx);
                    }
                    self.voice_changed(cx);
                }
            }
            // His voice over hers (headphones): the sidecar already stopped her.
            "barge-in" => {
                elog!("synaps-avatar: voice: barge-in");
                self.voice_stop_speech();
                self.voice_changed(cx);
            }
            "echo" => {
                self.hint = Some(("HEARING MYSELF · TALK-OVER OFF".into(), now + Duration::from_secs(4)));
                self.voice_changed(cx);
            }
            // A toggled turn's end: his pause (or the cap) sends it; no speech in 8 s closes it.
            "endpoint" => {
                let why = v.get("why").and_then(|x| x.as_str()).unwrap_or("");
                if let Some(h) = &mut self.voice.hf {
                    h.hearing = false;
                    self.voice.keyup = Some(now);
                    self.voice.transcribing = true;
                    self.voice_changed(cx);
                    return;
                }
                if self.voice.wake.turn {
                    self.voice.mic = None;
                    self.voice.level = 0.0;
                    if why != "nothing" {
                        self.voice.keyup = Some(now);
                        self.voice.transcribing = true;
                    }
                    self.voice_changed(cx);
                    return;
                }
                if self.voice.toggle.is_some() {
                    self.voice.asked = None;
                    if why != "nothing" {
                        self.voice.keyup = Some(now);
                        self.voice.transcribing = true;
                    }
                    if why == "cap" {
                        self.hint = Some(("SENT AT 1:00 · MAX LENGTH".into(), now + Duration::from_secs(3)));
                    }
                    for k in ["speech_ms", "silence_ms"] {
                        if let Some(x) = v.get(k) {
                            self.voice.stats.insert(format!("endpoint_{k}"), x.clone());
                        }
                    }
                    elog!("synaps-avatar: voice: toggle: endpoint ({why})");
                    self.voice_toggle_end(cx);
                    self.voice_changed(cx);
                }
            }
            // The wake stream refused (a call, the lock, paused, no model): not now; asked again later.
            "heard"
                if self.voice.wake.asked.is_some()
                    && self.voice.wake.open.is_none()
                    && v.get("text").and_then(|t| t.as_str()).unwrap_or("").is_empty()
                    && v.get("why").and_then(|t| t.as_str()).is_some_and(voice::wake_refusal) =>
            {
                let why = v.get("why").and_then(|t| t.as_str()).unwrap_or("").replace("NO MIC", "WAKE PAUSED");
                let w = &mut self.voice.wake;
                w.asked = None;
                w.lost = why.contains("NO MODEL") || why.contains("TEST SIDECAR") || why.contains("SOURCE IS GONE");
                w.retry_at = (!w.lost).then(|| now + if why.contains("FALSE STARTS") { Duration::from_secs(60) } else { WAKE_RETRY });
                elog!("synaps-avatar: voice: wake word refused ({why})");
                w.paused = Some(why);
                self.voice_changed(cx);
            }
            "heard" => {
                self.voice.transcribing = false;
                self.voice.wake.turn = false;
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
                if self.voice.hf.is_some() {
                    let why = v.get("why").and_then(|t| t.as_str()).unwrap_or("").to_string();
                    if text.is_empty() && why.starts_with("NO MIC") {
                        self.voice.hf.as_mut().unwrap().closed = true;
                        self.voice_hf_end(&why.replace("NO MIC · ", "HANDS-FREE OFF · "), cx);
                        return;
                    }
                    if text.is_empty() {
                        // ECHO: her own voice; PHRASE ONLY: "hey capri" (or "let's talk") alone, already listening.
                        if why != "ECHO" && why != "PHRASE ONLY" {
                            self.hint = Some((if why.is_empty() { "DIDN'T CATCH THAT".into() } else { why }, now + Duration::from_secs(3)));
                        }
                    } else if voice::end_phrase(&text) {
                        self.voice_hf_end("HANDS-FREE OFF", cx);
                        return;
                    } else {
                        elog!("synaps-avatar: voice: hands-free: heard {} chars", text.chars().count());
                        self.voice.pending = true;
                        self.voice.hf_pending = true;
                        self.voice.submit_at = Some(now);
                        if let Some(h) = &mut self.voice.hf {
                            h.quiet_since = now;
                        }
                        if let Err(e) = self.send(voice::wrap(&text)) {
                            self.voice.pending = false;
                            self.voice.hf_pending = false;
                            self.unseen.refused(format!("NOT SENT ({e}) · \"{text}\""));
                        } else {
                            self.ack_unread();
                        }
                    }
                    self.voice_changed(cx);
                    return;
                }
                if text.is_empty() {
                    let why = v.get("why").and_then(|t| t.as_str()).filter(|w| !w.is_empty()).unwrap_or("DIDN'T CATCH THAT").to_string();
                    self.hint = Some((why, now + Duration::from_secs(3)));
                    self.voice_send(serde_json::json!({ "op": "media-release" }));
                    self.voice.held = false;
                } else {
                    elog!("synaps-avatar: voice: heard {} chars", text.chars().count());
                    self.voice.pending = true;
                    self.voice.hf_pending = false;
                    self.voice.submit_at = Some(now);
                    match self.send(voice::wrap(&text)) {
                        Ok(()) => self.ack_unread(),
                        Err(e) => {
                            self.voice.pending = false;
                            self.voice_send(serde_json::json!({ "op": "media-release" }));
                            self.voice.held = false;
                            self.unseen.refused(format!("NOT SENT ({e}) · \"{text}\""));
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
                    // The turn before it is over (the sidecar plays in order; not all of it
                    // came back said, so not heard whole).
                    self.voice.prev = None;
                    self.voice.prev_left = 0;
                    self.voice.prev_whole = false;
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
                let whole = voice::say_end_whole(&v);
                if !whole {
                    elog!("synaps-avatar: voice: turn {id}: a sentence ended short ({v})");
                }
                if self.voice.prev == Some(id) {
                    self.voice.prev_left = self.voice.prev_left.saturating_sub(1);
                    self.voice.prev_whole &= whole;
                    if self.voice.prev_left == 0 {
                        // The turn before it, said to its end: heard whole, it is read.
                        if std::mem::take(&mut self.voice.prev_whole) {
                            self.voice_heard(id);
                        }
                        self.voice.prev = None;
                    }
                } else {
                    self.voice.said.end(whole);
                }
                if self.voice.said.left() == 0 && self.voice.prev_left == 0 {
                    self.voice.lips = None;
                    if self.voice.said.done {
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
                    self.voice.prev_whole = false;
                } else if self.voice.turn == Some(id) {
                    self.voice.speech = Speech::default();
                    self.voice.speech.feed("", true);
                    if self.voice.prev.is_none() {
                        self.voice_stop_speech();
                    } else {
                        // Its turn never came: the one before it plays on; this one is text
                        // (unread).
                        self.voice.said = Said { done: true, cut: true, ..Said::default() };
                    }
                }
                self.voice_changed(cx);
            }
            "stats" => {
                // The sidecar wins: a toggled mic it says is closed is closed here too.
                if self.voice.hf.is_some() && self.voice.mic.is_some() && v.get("mic_open").and_then(|x| x.as_bool()) == Some(false) {
                    elog!("synaps-avatar: voice: the sidecar says the mic is closed: hands-free ends");
                    self.voice.hf.as_mut().unwrap().closed = true;
                    self.voice_hf_end("HANDS-FREE OFF · MIC LOST", cx);
                    return;
                }
                if self.voice.toggle.is_some()
                    && self.voice.mic.is_some()
                    && v.get("mic_open").and_then(|x| x.as_bool()) == Some(false)
                {
                    elog!("synaps-avatar: voice: the sidecar says the mic is closed: so it is");
                    self.voice.mic = None;
                    self.voice.asked = None;
                    self.voice_toggle_end(cx);
                    self.voice_changed(cx);
                }
            }
            "hushed" => {}
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
            // Hands-free listens all along: only an utterance under way (or being written down) waits.
            mic: match self.voice.hf {
                Some(h) => h.hearing || self.voice.transcribing,
                None => self.voice_listening() || self.voice.transcribing,
            },
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
        self.voice.busy = b.busy;
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
            let playing = self.voice.turn.filter(|_| self.voice.said.done && (self.voice.said.left() > 0 || self.voice.prev_left > 0));
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
            self.voice.hf_pending = false;
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
                    self.voice.prev_left = self.voice.said.left();
                    self.voice.prev_unprompted = self.voice.unprompted;
                    self.voice.prev_whole = !self.voice.said.cut && !self.voice.speech.partial();
                }
            } else {
                self.voice.prev = None;
                self.voice.prev_left = 0;
                self.voice.prev_unprompted = false;
                self.voice.prev_whole = false;
            }
            self.voice.said = Said::default();
            self.voice.turn = Some(b.turn);
            self.voice.unprompted = unprompted;
            self.voice.speech = if unprompted { Speech::unprompted() } else { Speech::default() };
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
        let first_text = !self.voice.unprompted && self.voice.said.sent == 0 && !b.raw.trim().is_empty() && !self.voice.stats.contains_key("submit_to_text_ms");
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
            self.voice.said.sent += 1;
        }
        if !b.busy {
            self.voice.said.done = true;
            if self.voice.said.left() == 0 && self.voice.prev_left == 0 {
                self.voice_finish();
            }
        }
        true
    }

    /// Her last word: other audio comes back, the bubble gets the whole reply. Heard whole
    /// (every sentence said to its end, nothing of it left unsaid), the reply is read.
    fn voice_finish(&mut self) {
        if let Some(id) = self.voice.turn {
            let said = self.voice.said;
            if said.heard(self.voice.speech.partial()) {
                self.voice_heard(id);
            } else if said.sent > 0 {
                elog!("synaps-avatar: voice: turn {id} said in part (cut {}, partial {}): stays unread", said.cut, self.voice.speech.partial());
            }
        }
        self.voice.turn = None;
        self.voice.prev = None;
        self.voice.prev_left = 0;
        self.voice.prev_unprompted = false;
        self.voice.prev_whole = false;
        self.voice.unprompted = false;
        self.voice.lips = None;
        if self.voice.held && self.voice.hf.is_none() {
            self.voice.held = false;
            self.voice_send(serde_json::json!({ "op": "media-release", "after_ms": RESTORE_MS }));
        }
        if let Some(h) = &mut self.voice.hf {
            h.quiet_since = Instant::now();
        }
        self.voice_caption_done();
    }

    /// A spoken reply he heard whole: read, as if he had seen it (no strip, no "N NEW"; one
    /// already left for it, as it went behind the chip mid-speech, goes).
    fn voice_heard(&mut self, id: u64) {
        elog!("synaps-avatar: voice: turn {id} heard whole: read");
        self.unseen.heard(id);
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
    pub(super) fn voice_tick(&mut self, now: Instant, cx: &mut Context<Self>) -> bool {
        self.voice_guard();
        self.voice_mic_guard(cx);
        self.voice_wake_tick(cx);
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
        // The MIC timer counts seconds; a toggled mic is checked with the sidecar.
        let timer = self.voice.mic.map(|m| m + Duration::from_secs(now.saturating_duration_since(m).as_secs() + 1));
        if let Some(t) = [timer, self.voice.mic_check_at].into_iter().flatten().min() {
            return Some(t);
        }
        let wake = self.voice.wake.retry_at.filter(|_| self.voice.cfg.wake_word && self.voice_on());
        [self.voice.retry_at.filter(|_| self.voice_on() && self.voice.link.is_none()), wake].into_iter().flatten().min()
    }

    /// The tag while the mic is open, he waits for the words, or she speaks.
    pub(super) fn voice_status(&self) -> Option<Status> {
        use crate::theme::Plate;
        // Live mic: the solid red swallowtail, its bone mic pulsing; the wake word: a dashed slant
        // with a mic; heard and speaking: arrows (doing); paused: a grey dashed square.
        let st = |label: String, key: &str, invert: bool, glyph: &str, plate: Plate| Status {
            label,
            tone: self.pack.tone(Some(key)),
            invert,
            glyph: glyph.into(),
            plate,
            key: key.into(),
        };
        let bars = |v: f32| -> String { (0..5).map(|i| if v * 5.0 > i as f32 + 0.3 { '▮' } else { '▯' }).collect() };
        if let (Some(h), Some(_)) = (self.voice.hf, self.voice.mic) {
            let label = match self.voice_hf_left() {
                Some(left) => format!("MIC LIVE · HANDS-FREE · ENDS IN 0:{left:02}"),
                None => {
                    let s = h.since.elapsed().as_secs();
                    format!("MIC LIVE · HANDS-FREE {}:{:02} {}", s / 60, s % 60, bars(self.voice.level))
                }
            };
            return Some(st(label, "mic", true, "hf", Plate::Flag));
        }
        if let Some(m) = self.voice.mic {
            let s = m.elapsed().as_secs();
            let auto = if self.voice.toggle.is_some() { " · AUTO" } else { "" };
            return Some(st(format!("MIC {}:{:02} {}{auto}", s / 60, s % 60, bars(self.voice.level)), "mic", true, "mic", Plate::Flag));
        }
        if self.voice.asked.is_some() {
            return Some(st("MIC …".into(), "mic", false, "ring", Plate::Slant));
        }
        // The idle wake stream (open all along) has no tag of its own: her usual tag (ONLINE, her
        // mood, music) stays, with a mic glyph for its dot (Haseeb, 2026-11-16: "just ONLINE").
        if self.voice.transcribing {
            return Some(st("HEARD · …".into(), "heard", false, "dots", Plate::Arrow));
        }
        if let Some(p) = self.voice.wake.paused.as_ref().filter(|_| self.voice.cfg.wake_word && !self.voice_speaking()) {
            return Some(st(p.clone(), "paused", false, "ring", Plate::Square));
        }
        if self.voice_speaking() {
            let s = self.voice_shape(Instant::now()).unwrap_or(0);
            return Some(st(format!("SPEAKING {}", bars(s as f32 / 3.0)), "speak", false, "play", Plate::Arrow));
        }
        None
    }

    /// The Line's mic row: the tag's label while held, then what follows key-up.
    pub(super) fn voice_mic_row(&self) -> Option<String> {
        if self.voice.hf.is_some() {
            return self.voice_status().map(|s| format!("● {}  ·  say \"that's all\" or press $mod+Ctrl+c to end · esc drops what you're saying", s.label));
        }
        if self.voice_listening() {
            let how = if self.voice.toggle.is_some() { "sends when you pause · tap again to send now · esc cancels" } else { "release to send · esc to cancel" };
            return self.voice_status().map(|s| format!("● {}  ·  listening · {how}", s.label));
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
            "unprompted": self.voice.unprompted, "sent": self.voice.said.sent, "ended": self.voice.said.ended,
            "said_done": self.voice.said.done, "cut": self.voice.said.cut, "partial": self.voice.speech.partial(),
            "speak_unprompted": self.voice.cfg.speak_unprompted, "sidecar_unprompted": self.voice.can_unprompted,
            "shape": self.voice_shape(Instant::now()), "held": self.voice.held, "repeats_ignored": self.voice.repeats,
            "mic": {
                "open": self.voice.mic.is_some() || self.voice.wake.open.is_some(),
                "mode": if self.voice.hf.is_some() { "handsfree" } else if self.voice.toggle.is_some() { "toggle" } else if self.voice.wake.turn { "wake-turn" } else if self.voice_listening() { "ptt" } else if self.voice.wake.open.is_some() { "wake" } else { "" },
                "since_ms": self.voice.mic.or(self.voice.wake.open).map(|m| m.elapsed().as_millis() as u64),
            },
            "toggle": self.voice.toggle.map(|t| serde_json::json!({ "hearing": t.hearing, "opened_line": t.opened_line, "ms": t.at.elapsed().as_millis() as u64 })),
            "hint": self.hint.as_ref().filter(|h| h.1 > Instant::now()).map(|h| h.0.clone()),
            "handsfree": self.voice.hf.map(|h| serde_json::json!({ "ms": h.since.elapsed().as_millis() as u64, "quiet_ms": h.quiet_since.elapsed().as_millis() as u64, "hearing": h.hearing, "left_s": self.voice_hf_left(), "wake": h.wake })),
            "busy": self.voice.busy,
            "wake": { "on": self.voice.cfg.wake_word, "sensitivity": self.voice.cfg.wake_sensitivity, "handsfree": self.voice.cfg.wake_handsfree, "open": self.voice.wake.open.is_some(),
                      "asked": self.voice.wake.asked.is_some(), "turn": self.voice.wake.turn, "paused": self.voice.wake.paused, "lost": self.voice.wake.lost },
            "toggle_lock": self.voice.toggle_lock, "toggles_ignored": self.voice.toggles_ignored, "silence_s": self.voice.cfg.silence_s,
            "stats": serde_json::Value::Object(self.voice.stats.clone()),
        })
    }
}
