//! Voice, the pure parts: the spoken-turn hint, the persona lines for speech, the speech
//! markup (`*word*` stresses a word aloud, and is never shown), and what of a reply is spoken
//! (whole sentences, in order, with the guards: nothing after `---`, no code, tables or URLs
//! aloud, about 60 words at most), and whose turn is spoken (his spoken turns; her replies to
//! what arrives on its own). The audio itself lives in the `capri-voice` sidecar.

/// Sent ahead of a spoken message (per turn, so the system prompt and its cache stay put).
/// Short, so the echo (cut at 200 chars) still holds his words.
pub const HINT: &str = "[voice: spoken; read aloud. 1-2 short sentences, ~40 words; code, paths, URLs only after a line ---]";

/// Added to her persona while voice is on: how a reply that will be heard is written. Light on
/// purpose: a stressed word in every sentence sounds like a sales pitch.
pub const PERSONA: &str = "When a message starts with [voice: ...], your reply is read aloud. Write it for the ear: \
short sentences, contractions, numbers and names the way you'd say them. If one word carries the point, you may \
wrap it in *asterisks* and it will be stressed: at most one in a reply, and most replies need none. A dash — is a \
beat. No lists, headings, emoji or caps for emphasis.";

/// Added with `[voice] speak_unprompted`: her replies to what arrives on its own are heard too,
/// and only their first lines are (`Speech::unprompted`): the first sentence carries it.
pub const PERSONA_UNPROMPTED: &str = "Replies to messages that arrive on their own are read aloud: open with one \
short sentence that stands alone and names who it is from; details after a line ---";

/// The persona with the voice lines added (`voice`: `[voice] enabled`; `unprompted`: `[voice]
/// speak_unprompted`).
pub fn persona(system: Option<String>, voice: bool, unprompted: bool) -> Option<String> {
    match system {
        Some(s) if voice && unprompted => Some(format!("{}\n\n{PERSONA} {PERSONA_UNPROMPTED}", s.trim_end())),
        Some(s) if voice => Some(format!("{}\n\n{PERSONA}", s.trim_end())),
        s => s,
    }
}

/// Who started a turn, from the daemon's `TurnStarted.trigger` (only a live one: the reply an
/// attach shows, or a turn already running at attach, has none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// He did: typed, queued, a command he ran (`user`, `queued_auto`, `plugin_command`), or no
    /// trigger at all (an older daemon, an attach): spoken only if he spoke it here.
    His,
    /// It arrived on its own: an event (hooks, `synaps send`, comms, a summons: `event_auto`) or
    /// the session driver (`driver_auto`).
    Unprompted,
    /// Never spoken: compaction, and triggers this build doesn't know (a replay must not be read
    /// aloud: fail closed).
    Quiet,
}

pub fn origin(trigger: &str) -> Origin {
    match trigger {
        "user" | "queued_auto" | "plugin_command" | "" => Origin::His,
        "event_auto" | "driver_auto" => Origin::Unprompted,
        _ => Origin::Quiet,
    }
}

/// What she can't be heard through: why an unprompted reply stays silent (it is still on the
/// Line, the bubble and the unread strip), or None to speak it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Quiet {
    pub voice_off: bool,
    pub setting_off: bool,
    pub no_sidecar: bool,
    /// An approval or a secret on screen: those are never by voice.
    pub prompt: bool,
    /// A media player is playing (LOCK 12 (9)): skipped, not queued, so nothing is said late.
    pub music: bool,
    /// His mic is open or his words are being written down.
    pub mic: bool,
    /// She is hidden for a fullscreen window.
    pub hidden: bool,
}

impl Quiet {
    pub fn why(&self) -> Option<&'static str> {
        [
            (self.voice_off, "voice off"),
            (self.setting_off, "speak_unprompted = false"),
            (self.no_sidecar, "no sidecar"),
            (self.prompt, "a prompt is on screen"),
            (self.music, "music is playing"),
            (self.mic, "his mic is open"),
            (self.hidden, "hidden for fullscreen"),
        ]
        .into_iter()
        .find_map(|(on, why)| on.then_some(why))
    }
}

/// Speech markup, opening: a single `*` (not `**`) that is not inside a word. `prev`: the
/// character before it.
pub fn em_opens(prev: Option<char>, at: &str) -> bool {
    at.starts_with('*') && !at.starts_with("**") && !prev.is_some_and(|p| p.is_alphanumeric() || p == '*')
}

/// Speech markup, closing: in `r` (the text just after an opening `*`), the byte offset of the
/// `*` that closes a `*word*` span: the span starts and ends with a non-space, stays on one
/// line, holds no code, and the closing `*` is not followed by a letter or a digit. So `a * b`,
/// `2*3*4` and `**bold**` are not spans.
pub fn em_close(r: &str) -> Option<usize> {
    let first = r.chars().next()?;
    if first.is_whitespace() || first == '*' {
        return None;
    }
    let mut prev = first;
    for (i, c) in r.char_indices().skip(1) {
        match c {
            '`' | '\n' => return None,
            '*' => {
                let next = r[i + 1..].chars().next();
                return (!prev.is_whitespace() && next != Some('*') && !next.is_some_and(char::is_alphanumeric)).then_some(i);
            }
            _ => prev = c,
        }
    }
    None
}

/// `s` as shown: the asterisks of `*word*` spans dropped (outside `code`); other asterisks stay.
pub fn unmark(s: &str) -> String {
    marks(s, |inner, out| out.push_str(inner), |c, out| out.push(c))
}

/// `s` as said: a `*span*` of up to three words keeps its asterisks (the sidecar stresses it),
/// longer ones are read plain, `**` and stray asterisks go.
fn speech_marks(s: &str) -> String {
    let s = s.replace("**", "");
    marks(
        &s,
        |inner, out| {
            if inner.split_whitespace().count() <= 3 {
                out.push('*');
                out.push_str(inner);
                out.push('*');
            } else {
                out.push_str(inner);
            }
        },
        |c, out| out.push(if c == '*' { ' ' } else { c }),
    )
}

fn marks(s: &str, span: impl Fn(&str, &mut String), other: impl Fn(char, &mut String)) -> String {
    let mut out = String::with_capacity(s.len());
    let (mut rest, mut prev, mut code) = (s, None, false);
    while let Some(c) = rest.chars().next() {
        if c == '`' {
            code = !code;
        } else if !code
            && em_opens(prev, rest)
            && let Some(end) = em_close(&rest[1..])
        {
            let inner = &rest[1..1 + end];
            span(inner, &mut out);
            prev = Some('*');
            rest = &rest[end + 2..];
            continue;
        }
        if code { out.push(c) } else { other(c, &mut out) }
        prev = Some(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Words spoken at most; then she says the rest is written.
const WORDS_MAX: usize = 60;
const REST: &str = "The rest is on the line.";

/// A spoken message as sent: the hint, then his words.
pub fn wrap(text: &str) -> String {
    format!("{HINT} {text}")
}

/// A spoken turn as the sidecar said it: its sentences sent and said, whether its text is all
/// in, and whether anything cut it. Heard whole (`heard`), it counts as read: no unread strip,
/// no "N NEW". Anything less (cut, or said in part) leaves it unread, as a reply never spoken.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Said {
    pub sent: u32,
    pub ended: u32,
    /// The turn is over: its text is all in, every sentence of it given out.
    pub done: bool,
    /// Stopped before its end: a tap on her, the talk key, Esc, music, a new turn, his voice
    /// over hers, a call or the lock (the sidecar skips it), a sentence the sidecar couldn't
    /// say or whose player ended early, the sidecar going (or restarting) mid-say.
    pub cut: bool,
}

impl Said {
    /// A sentence's `say-end` (`say_end_whole`: said to its end or not).
    pub fn end(&mut self, whole: bool) {
        self.ended += 1;
        self.cut |= !whole;
    }

    /// Sentences still out (being said, or waiting their turn).
    pub fn left(&self) -> u32 {
        self.sent.saturating_sub(self.ended)
    }

    /// Heard whole: the turn over, every sentence said to its end, none cut, and nothing of
    /// the reply left unsaid (`partial`: `Speech::partial`).
    pub fn heard(&self, partial: bool) -> bool {
        self.done && !self.cut && !partial && self.sent > 0 && self.ended >= self.sent
    }
}

/// A `say-end` said its sentence to the end: not when the sidecar couldn't make the audio
/// (`error`) or its player ended early (`cut`). An older sidecar sends neither: whole.
pub fn say_end_whole(v: &serde_json::Value) -> bool {
    v.get("error").is_none() && v.get("cut").and_then(serde_json::Value::as_bool) != Some(true)
}

/// A hands-free utterance that ends the conversation ("that's all", "goodbye"): the whole of it,
/// after a leading ok/okay/alright/thanks and a trailing capri/thanks/for now; never over 6 words.
pub fn end_phrase(text: &str) -> bool {
    let norm: String = text.to_lowercase().chars().map(|c| if c.is_alphanumeric() || c == '\'' || c == ' ' { c } else { ' ' }).collect();
    let mut w: Vec<&str> = norm.split_whitespace().collect();
    if w.is_empty() || w.len() > 6 {
        return false;
    }
    while let Some(&first) = w.first() {
        if ["ok", "okay", "alright", "thanks"].contains(&first) && w.len() > 1 {
            w.remove(0);
        } else if first == "all" && w.get(1) == Some(&"right") && w.len() > 2 {
            w.drain(..2);
        } else {
            break;
        }
    }
    loop {
        let n = w.len();
        if n > 1 && ["capri", "thanks"].contains(&w[n - 1]) {
            w.pop();
        } else if n > 2 && matches!((w[n - 2], w[n - 1]), ("for", "now") | ("thank", "you")) {
            w.truncate(n - 2);
        } else {
            break;
        }
    }
    let p = w.join(" ");
    ["that's all", "that is all", "that'll be all", "we're done", "we are done", "stop listening", "goodbye", "bye", "bye bye"].contains(&p.as_str())
}

/// The sidecar's reason for refusing the wake stream (`heard`, no text, right after `listen-start wake`):
/// not an utterance's (DIDN'T CATCH THAT, ECHO, PHRASE ONLY from a hands-free that just ended).
pub fn wake_refusal(why: &str) -> bool {
    why.starts_with("WAKE ") || why.starts_with("NO MIC") || why.starts_with("TEST SIDECAR")
}

/// His words, if `user_text` is a spoken turn (the hint stripped).
pub fn spoken(user_text: &str) -> Option<&str> {
    let t = user_text.strip_prefix("[voice")?;
    Some(t.find("] ").map_or("", |end| t[end + 2..].trim_start()))
}

/// The mouth frame for a loudness shape from the sidecar (0 closed .. 3 wide).
pub fn mouth_frame(shape: u8) -> &'static str {
    match shape {
        0 => "neutral",
        1 => "idle-mouth_small",
        2 => "idle-mouth_round",
        _ => "idle-mouth_wide",
    }
}

/// A reply, sentence by sentence, as it streams.
#[derive(Debug, Default)]
pub struct Speech {
    said: usize,
    stopped: bool,
    /// An unprompted reply's cap: its first sentences only (`unprompted`).
    cap: Option<(usize, usize)>,
    /// Stopped at a cap after this many sentences: any after them go unsaid.
    capped: Option<usize>,
    /// Something of the reply goes unsaid: past a cap, after `---`, code, a table, a link.
    partial: bool,
}

/// An unprompted reply says this much at most: two sentences, 30 words.
pub const UNPROMPTED_CAP: (usize, usize) = (2, 30);

impl Speech {
    /// For a reply to what arrived on its own: up to `---`, two sentences or 30 words (the first
    /// sentence always), then the closing line once if anything is left.
    pub fn unprompted() -> Speech {
        Speech { cap: Some(UNPROMPTED_CAP), ..Default::default() }
    }

    /// The sentences of `raw` (the reply as written so far) not yet given out; `finished`:
    /// the turn is over, so the last one is complete too.
    pub fn feed(&mut self, raw: &str, finished: bool) -> Vec<String> {
        let (text, ended, left_out) = speakable(raw, finished);
        self.partial |= left_out;
        if self.stopped {
            if let Some(n) = self.capped {
                self.partial |= sentences(&text, finished || ended).len() > n;
            }
            return Vec::new();
        }
        let all = sentences(&text, finished || ended);
        let total = all.len();
        let mut out = Vec::new();
        let mut words = 0;
        for (i, s) in all.into_iter().enumerate() {
            words += s.split_whitespace().count();
            if let Some((n, w)) = self.cap
                && (i >= n || (i > 0 && words > w))
            {
                out.push(REST.into());
                self.stopped = true;
                self.capped = Some(i);
                self.partial = true;
                return out;
            }
            if i >= self.said {
                out.push(s);
            }
            self.said = self.said.max(i + 1);
            if words > WORDS_MAX {
                out.push(REST.into());
                self.stopped = true;
                self.capped = Some(i + 1);
                self.partial |= total > i + 1 || !finished;
                break;
            }
        }
        if ended {
            self.stopped = true;
            if self.cap.is_some() {
                out.push(REST.into());
            }
        }
        out
    }

    /// Something of the reply went unsaid (past a cap, after `---`, code, a table, a link):
    /// heard, it is still not heard whole.
    pub fn partial(&self) -> bool {
        self.partial
    }
}

/// What may be said of `raw`, whether it hit the `---` line, and whether anything of it is
/// left out (text after the rule, code, a table, a link or a path named, not read). Only
/// whole lines count for the markers; the last, unfinished line goes in as it is.
fn speakable(raw: &str, finished: bool) -> (String, bool, bool) {
    let mut out = String::new();
    let (mut code, mut said_code, mut said_table, mut left) = (false, false, false, false);
    let lines: Vec<&str> = raw.split('\n').collect();
    let n = lines.len();
    for (i, line) in lines.iter().enumerate() {
        let whole = i + 1 < n || finished;
        let t = line.trim();
        if whole && (t == "---" || t == "***" || t == "___") {
            left |= lines[i + 1..].iter().any(|l| !l.trim().is_empty());
            return (out, true, left);
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            if !code && !said_code {
                out.push_str(" Code's on the line. ");
                said_code = true;
            }
            left = true;
            code = !code;
            continue;
        }
        if code || t.is_empty() {
            continue;
        }
        if t.starts_with('|') {
            if !said_table {
                out.push_str(" There's a table on the line. ");
                said_table = true;
            }
            left = true;
            continue;
        }
        let (mut s, named) = plain(t);
        left |= named;
        if whole && !s.is_empty() && !s.ends_with(['.', '?', '!', ';', ':', '…']) {
            s.push('.');
        }
        out.push(' ');
        out.push_str(&s);
    }
    (out, false, left)
}

/// A markdown line as said: no markers, link text without its URL, bare URLs and long
/// inline code named, not read (`true`: something was).
fn plain(line: &str) -> (String, bool) {
    let mut t = line.trim_start_matches(['#', '>', ' ']).to_string();
    for m in ["- ", "* ", "+ "] {
        if let Some(r) = t.strip_prefix(m) {
            t = r.to_string();
        }
    }
    if let Some(p) = t.find(". ").filter(|&p| p <= 3 && t[..p].chars().all(|c| c.is_ascii_digit())) {
        t = t[p + 2..].to_string();
    }
    // [text](url) -> text
    let mut s = String::new();
    let mut rest = t.as_str();
    while let Some(a) = rest.find('[') {
        let Some(b) = rest[a..].find("](").map(|b| a + b) else { break };
        let Some(c) = rest[b..].find(')').map(|c| b + c) else { break };
        s.push_str(&rest[..a]);
        s.push_str(&rest[a + 1..b]);
        rest = &rest[c + 1..];
    }
    s.push_str(rest);
    // `code`: short words stay, paths and long bits are named.
    let mut o = String::new();
    let mut parts = s.split('`');
    if let Some(first) = parts.next() {
        o.push_str(first);
    }
    let mut named = false;
    for (i, p) in parts.enumerate() {
        if i % 2 == 0 {
            let short = p.len() <= 24 && !p.contains(['/', '\\', '(', '{', '=']);
            named |= !short;
            o.push_str(&if short { p.replace('*', " ") } else { "that (it's on the line)".into() });
        } else {
            o.push_str(p);
        }
    }
    let words: Vec<String> = speech_marks(&o)
        .split_whitespace()
        .flat_map(|w| {
            if w.contains("://") || w.starts_with("www.") {
                named = true;
                vec!["the link's on the line".to_string()]
            } else {
                w.replace('_', " ").split_whitespace().map(str::to_string).collect()
            }
        })
        .collect();
    (words.join(" "), named)
}

/// Whole sentences of `s`: they end at `. ? ! ; …` followed by a space (closing quotes,
/// brackets and a closing `*` go with them), not inside a decimal or after a common abbreviation. `finished`:
/// the remainder is one too.
fn sentences(s: &str, finished: bool) -> Vec<String> {
    const ABBR: [&str; 9] = ["e.g", "i.e", "etc", "vs", "mr", "mrs", "dr", "st", "approx"];
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '.' | '?' | '!' | ';' | '…') {
            let mut j = i + 1;
            while j < chars.len() && matches!(chars[j], '.' | '?' | '!' | '"' | '\'' | '”' | '’' | ')' | ']' | '*') {
                j += 1;
            }
            let spaced = j < chars.len() && chars[j].is_whitespace();
            let word: String = chars[start..i].iter().rev().take_while(|c| !c.is_whitespace()).collect::<Vec<_>>().into_iter().rev().collect();
            let abbr = c == '.' && ABBR.contains(&word.to_ascii_lowercase().as_str());
            if spaced && !abbr {
                let t: String = chars[start..j].iter().collect::<String>().trim().to_string();
                if t.chars().any(char::is_alphanumeric) {
                    out.push(t);
                }
                start = j;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    if finished {
        let t: String = chars[start..].iter().collect::<String>().trim().to_string();
        if t.chars().any(char::is_alphanumeric) {
            out.push(t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reply as the view says it: fed whole, every sentence sent; `ends` of them come back
    /// said with these `say-end` lines (fewer: she was stopped before the rest).
    fn say(raw: &str, unprompted: bool, ends: &[&str]) -> (Said, Speech) {
        let mut sp = if unprompted { Speech::unprompted() } else { Speech::default() };
        let mut said = Said::default();
        // As it streams: a half, then the rest (the turn over).
        let half = &raw[..raw.char_indices().nth(raw.chars().count() / 2).map_or(0, |c| c.0)];
        said.sent += sp.feed(half, false).len() as u32;
        said.sent += sp.feed(raw, true).len() as u32;
        said.done = true;
        for e in ends {
            said.end(say_end_whole(&serde_json::from_str(e).unwrap()));
        }
        (said, sp)
    }

    const END: &str = r#"{"ev":"say-end","id":7}"#;

    #[test]
    fn a_reply_said_whole_is_heard_whole() {
        let (said, sp) = say("Build's green, choom. Spike pushed the reader fix eleven minutes ago.", false, &[END, END]);
        assert_eq!((said.sent, said.ended, said.cut, sp.partial()), (2, 2, false, false));
        assert!(said.heard(sp.partial()));
        // An event's reply, under its cap: whole too.
        let (said, sp) = say("From spike: merged.", true, &[END]);
        assert!(said.heard(sp.partial()));
        // Not yet: a sentence still out, or the turn not over.
        let (said, sp) = say("One. Two. Three.", false, &[END, END]);
        assert!(!said.heard(sp.partial()) && said.left() == 1);
        assert!(!Said { sent: 1, ended: 1, ..Said::default() }.heard(false), "its text not all in");
        assert!(!Said { done: true, ..Said::default() }.heard(false), "nothing said is nothing heard");
    }

    #[test]
    fn a_cut_reply_is_not_heard_whole() {
        let reply = "Rebasing polish-e onto main. Two conflicts in view.rs. He's on it.";
        // A tap on her body, the talk key ($mod+c), Esc: hushed after the first sentence. Her
        // next `say-end`s never come (the sidecar's hush drops them), and the view drops the turn.
        let (said, sp) = say(reply, false, &[END]);
        assert_eq!((said.sent, said.left()), (3, 2));
        assert!(!said.heard(sp.partial()));
        // ...on the last sentence, mid-word: still one short.
        let (said, sp) = say(reply, false, &[END, END]);
        assert!(!said.heard(sp.partial()));
        // Music starts under an event's reply: she is hushed (as a tap), or the sidecar's gate
        // skips the turn (`say-skipped`: cut), or its player ends early (`cut`).
        let event = "From spike: merged. CI is green.";
        let (said, sp) = say(event, true, &[END, END]);
        assert!(said.heard(sp.partial()), "said whole, it would be");
        let (said, sp) = say(event, true, &[END]);
        assert!(!said.heard(sp.partial()));
        let (mut said, sp) = say(event, true, &[END]);
        said = Said { done: true, cut: true, ..said };
        assert!(!said.heard(sp.partial()));
        let (said, sp) = say(event, true, &[END, r#"{"ev":"say-end","id":7,"cut":true}"#]);
        assert!(said.cut && !said.heard(sp.partial()));
        let (said, sp) = say(reply, false, &[END, END, r#"{"ev":"say-end","id":7,"cut":true}"#]);
        assert!(said.cut && !said.heard(sp.partial()));
        // The sidecar restarts mid-say: the sentence it was saying never ends (the view drops
        // the turn as the link goes); one it couldn't make after it comes back with an error.
        let (said, sp) = say(reply, false, &[END, END, r#"{"ev":"say-end","id":7,"error":"onnxruntime: device lost"}"#]);
        assert!(said.cut && !said.heard(sp.partial()));
        // An older sidecar's plain `say-end` is a whole one.
        assert!(say_end_whole(&serde_json::json!({ "ev": "say-end", "id": 7 })));
        assert!(say_end_whole(&serde_json::json!({ "ev": "say-end", "id": 7, "cut": false })));
    }

    #[test]
    fn a_reply_said_in_part_is_not_heard_whole() {
        let all = |raw: &str, unprompted: bool| {
            let mut sp = if unprompted { Speech::unprompted() } else { Speech::default() };
            let n = sp.feed(raw, true).len();
            let (said, sp) = say(raw, unprompted, &vec![END; n]);
            (said.heard(sp.partial()), sp.partial())
        };
        // Partly spoken counts as cut off: past `---`, code, a table, a link or a path named.
        assert_eq!(all("Merged.\n---\nsha 9f8e7d6", false), (false, true));
        assert_eq!(all("Here's the command:\n```bash\ntools/abuild build\n```", false), (false, true));
        assert_eq!(all("Totals:\n| a | b |\n|---|---|", false), (false, true));
        assert_eq!(all("Docs at https://x.y/z now.", false), (false, true));
        assert_eq!(all("Run `tools/abuild build --release` now.", false), (false, true));
        // Past the 60 words, or an event's cap: the rest is on the line.
        assert_eq!(all(&"This sentence has exactly ten words in it, see here. ".repeat(8), false), (false, true));
        assert_eq!(all("Spike says CI is red. Rebase or merge? He waits on you.", true), (false, true));
        // Said whole: a rule with nothing after it, short code, a link's text, markers.
        assert_eq!(all("Merged.\n---\n", false), (true, false));
        assert_eq!(all("Run `ls` and see [the PR](https://x.y/1).", false), (true, false));
        assert_eq!(all("## Done\n- **Build** is green.", false), (true, false));
        // Exactly over the 60 words on the last sentence: all of it was said.
        let sixty_five = "This sentence has exactly ten words in it, see here. ".repeat(6) + "Five more words right here.";
        assert_eq!(all(&sixty_five, false), (true, false));
        // Text after the rule that streams in once she stopped at it still counts.
        let mut sp = Speech::default();
        assert_eq!(sp.feed("Merged.\n---\n", false), vec!["Merged."]);
        assert!(!sp.partial());
        assert!(sp.feed("Merged.\n---\nsha 9f8e7d6", true).is_empty());
        assert!(sp.partial());
    }

    #[test]
    fn wake_refusals_are_told_from_an_utterance() {
        for r in ["WAKE PAUSED · IN A CALL", "WAKE OFF · NO MODEL (capri-voice-setup)", "NO MIC · SCREEN LOCKED", "NO MIC · ITS SOURCE IS GONE", "TEST SIDECAR · NO REAL MIC"] {
            assert!(wake_refusal(r), "{r}");
        }
        for u in ["", "ECHO", "PHRASE ONLY", "DIDN'T CATCH THAT", "NOTHING HEARD · MIC CLOSED"] {
            assert!(!wake_refusal(u), "{u}");
        }
    }

    #[test]
    fn hands_free_ends_on_a_whole_end_phrase_only() {
        for t in ["That's all.", "Okay, that's all, Capri.", "Thanks, that's all for now.", "We're done", "Stop listening.", "Goodbye!", "bye", "Alright, we're done, thanks", "OK. That is all."] {
            assert!(end_phrase(t), "{t}");
        }
        for t in ["That's all I wanted to say about the build, really.", "Is that all?", "Tell lynx goodbye", "Stop", "Don't stop listening", "", "Bye the way, what's next", "okay"] {
            assert!(!end_phrase(t), "{t}");
        }
    }

    #[test]
    fn sentences_come_whole_and_in_order_as_the_reply_streams() {
        let mut sp = Speech::default();
        assert!(sp.feed("On it, cho", false).is_empty());
        assert!(sp.feed("On it, choom.", false).is_empty());
        assert_eq!(sp.feed("On it, choom. Spike pushed v1.2 at 3.5 p", false), vec!["On it, choom."]);
        assert_eq!(sp.feed("On it, choom. Spike pushed v1.2 at 3.5 pm, e.g. the fix. Done", true), vec!["Spike pushed v1.2 at 3.5 pm, e.g. the fix.", "Done."]);
        assert!(sp.feed("On it, choom. Spike pushed v1.2 at 3.5 pm, e.g. the fix. Done", true).is_empty());
    }

    #[test]
    fn nothing_after_the_rule_and_no_code_or_urls_aloud() {
        let mut sp = Speech::default();
        let r = "Here's the **build** command:\n```bash\ntools/abuild build --release\n```\nDocs at https://github.com/x/y and [the PR](https://x.y/1).\n---\nsha 9f8e7d6c";
        assert_eq!(sp.feed(r, true), vec!["Here's the build command: Code's on the line.", "Docs at the link's on the line and the PR."]);
        let mut sp = Speech::default();
        assert_eq!(sp.feed("Run `tools/abuild build --release` or `ls`.", true), vec!["Run that (it's on the line) or ls."]);
    }

    #[test]
    fn a_long_reply_stops_and_says_the_rest_is_written() {
        let mut sp = Speech::default();
        let long = "This sentence has exactly ten words in it, see here. ".repeat(8);
        let out = sp.feed(&long, true);
        assert_eq!(out.last().map(String::as_str), Some(REST));
        assert!(out.len() <= 8);
        assert!(sp.feed(&long, true).is_empty());
    }

    #[test]
    fn a_stressed_word_reaches_the_sidecar_and_nothing_else_does() {
        let mut sp = Speech::default();
        let r = "I'm *not* signing that. It's **really** fine, *slightly* odd *and much too long a span*. Then *why*? 2*3*4 a * b";
        assert_eq!(sp.feed(r, true), vec!["I'm *not* signing that.", "It's really fine, *slightly* odd and much too long a span.", "Then *why*?", "2 3 4 a b."]);
        let mut sp = Speech::default();
        assert_eq!(sp.feed("Run `*.rs` now, and it's *done.* Next", true), vec!["Run .rs now, and it's *done.*", "Next."]);
    }

    #[test]
    fn markup_is_never_shown() {
        assert_eq!(unmark("I'm *not* signing that, *choom*."), "I'm not signing that, choom.");
        assert_eq!(unmark("a * b, 2*3*4, `*.rs` and *two words*"), "a * b, 2*3*4, `*.rs` and two words");
        assert_eq!(unmark("*unclosed and a lone *"), "*unclosed and a lone *");
        assert!(em_opens(None, "*x*") && !em_opens(Some('a'), "*x*") && !em_opens(None, "**x**"));
        assert_eq!((em_close("not* x"), em_close(" not*"), em_close("not *"), em_close("no*t")), (Some(3), None, None, None));
    }

    #[test]
    fn the_voice_lines_join_the_persona_only_with_voice_on() {
        assert_eq!(persona(Some("You are Capri.\n".into()), false, true).as_deref(), Some("You are Capri.\n"));
        let on = persona(Some("You are Capri.\n".into()), true, false).unwrap();
        assert!(on.starts_with("You are Capri.\n\nWhen a message starts with [voice") && on.contains("*asterisks*"));
        assert!(!on.contains("on its own"));
        assert!(persona(Some("You are Capri.".into()), true, true).unwrap().ends_with(PERSONA_UNPROMPTED));
        assert_eq!(persona(None, true, true), None, "no persona (off): nothing is sent, voice or not");
        assert!(PERSONA.contains("at most one in a reply"), "light on stress: she must not mark every sentence");
    }

    #[test]
    fn whose_turn_and_what_keeps_her_quiet() {
        for t in ["user", "queued_auto", "plugin_command"] {
            assert_eq!(origin(t), Origin::His, "{t}");
        }
        assert_eq!(origin(""), Origin::His, "no trigger (older daemon, attach): his rule");
        assert_eq!((origin("event_auto"), origin("driver_auto")), (Origin::Unprompted, Origin::Unprompted));
        for t in ["compaction", "event", "replay", "something_new"] {
            assert_eq!(origin(t), Origin::Quiet, "{t}: unknown fails closed");
        }
        assert_eq!(Quiet::default().why(), None);
        assert_eq!(Quiet { music: true, ..Default::default() }.why(), Some("music is playing"));
        assert_eq!(Quiet { prompt: true, music: true, ..Default::default() }.why(), Some("a prompt is on screen"));
        assert_eq!(Quiet { mic: true, ..Default::default() }.why(), Some("his mic is open"));
        assert_eq!(Quiet { setting_off: true, ..Default::default() }.why(), Some("speak_unprompted = false"));
        assert_eq!(Quiet { hidden: true, ..Default::default() }.why(), Some("hidden for fullscreen"));
    }

    #[test]
    fn an_unprompted_reply_says_its_first_lines_only() {
        let mut sp = Speech::unprompted();
        assert_eq!(sp.feed("Spike says CI is red. Rebase or merge?", false), vec!["Spike says CI is red."]);
        assert_eq!(sp.feed("Spike says CI is red. Rebase or merge? He waits on you. More", false), vec!["Rebase or merge?", REST]);
        assert!(sp.feed("Spike says CI is red. Rebase or merge? He waits on you. More.", true).is_empty());
        // 30 words: the second sentence would cross it.
        let mut sp = Speech::unprompted();
        let long = format!("Lynx finished the rebase. {}.", "word ".repeat(30).trim());
        assert_eq!(sp.feed(&long, true), vec!["Lynx finished the rebase.", REST]);
        // The first sentence always, however long; and the rule ends it with the closing line.
        let mut sp = Speech::unprompted();
        let one = format!("{}.", "word ".repeat(40).trim());
        assert_eq!(sp.feed(&one, true), vec![one.clone()]);
        let mut sp = Speech::unprompted();
        assert_eq!(sp.feed("From spike: merged.\n---\nsha 9f8e7d6", true), vec!["From spike: merged.", REST]);
        let mut sp = Speech::unprompted();
        assert_eq!(sp.feed("From spike: merged.", true), vec!["From spike: merged."], "nothing left: no closing line");
    }

    #[test]
    fn the_hint_wraps_and_unwraps() {
        let w = wrap("what's spike doing?");
        assert!(w.starts_with("[voice"));
        assert_eq!(spoken(&w), Some("what's spike doing?"));
        assert_eq!(spoken("what's spike doing?"), None);
        assert!(HINT.len() < 120, "the echo is cut at 200 chars: the hint must leave room for his words");
        assert_eq!(mouth_frame(0), "neutral");
        assert_eq!(mouth_frame(9), "idle-mouth_wide");
    }
}
