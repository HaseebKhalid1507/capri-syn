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
        if self.stopped {
            return Vec::new();
        }
        let (text, ended) = speakable(raw, finished);
        let all = sentences(&text, finished || ended);
        let mut out = Vec::new();
        let mut words = 0;
        for (i, s) in all.into_iter().enumerate() {
            words += s.split_whitespace().count();
            if let Some((n, w)) = self.cap
                && (i >= n || (i > 0 && words > w))
            {
                out.push(REST.into());
                self.stopped = true;
                return out;
            }
            if i >= self.said {
                out.push(s);
            }
            self.said = self.said.max(i + 1);
            if words > WORDS_MAX {
                out.push(REST.into());
                self.stopped = true;
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
}

/// What may be said of `raw`, and whether it hit the `---` line. Only whole lines count
/// for the markers; the last, unfinished line goes in as it is.
fn speakable(raw: &str, finished: bool) -> (String, bool) {
    let mut out = String::new();
    let (mut code, mut said_code, mut said_table) = (false, false, false);
    let lines: Vec<&str> = raw.split('\n').collect();
    let n = lines.len();
    for (i, line) in lines.into_iter().enumerate() {
        let whole = i + 1 < n || finished;
        let t = line.trim();
        if whole && (t == "---" || t == "***" || t == "___") {
            return (out, true);
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            if !code && !said_code {
                out.push_str(" Code's on the line. ");
                said_code = true;
            }
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
            continue;
        }
        let mut s = plain(t);
        if whole && !s.is_empty() && !s.ends_with(['.', '?', '!', ';', ':', '…']) {
            s.push('.');
        }
        out.push(' ');
        out.push_str(&s);
    }
    (out, false)
}

/// A markdown line as said: no markers, link text without its URL, bare URLs and long
/// inline code named, not read.
fn plain(line: &str) -> String {
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
    for (i, p) in parts.enumerate() {
        if i % 2 == 0 {
            o.push_str(&if p.len() <= 24 && !p.contains(['/', '\\', '(', '{', '=']) { p.replace('*', " ") } else { "that (it's on the line)".into() });
        } else {
            o.push_str(p);
        }
    }
    let words: Vec<String> = speech_marks(&o)
        .split_whitespace()
        .flat_map(|w| if w.contains("://") || w.starts_with("www.") { vec!["the link's on the line".to_string()] } else { w.replace('_', " ").split_whitespace().map(str::to_string).collect() })
        .collect();
    words.join(" ")
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
