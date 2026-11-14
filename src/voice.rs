//! Voice, the pure parts: the spoken-turn hint, and what of a reply is spoken (whole
//! sentences, in order, with the guards: nothing after `---`, no code, tables or URLs aloud,
//! about 60 words at most). The audio itself lives in the `capri-voice` sidecar.

/// Sent ahead of a spoken message (per turn, so the system prompt and its cache stay put).
/// Short, so the echo (cut at 200 chars) still holds his words.
pub const HINT: &str = "[voice: spoken; read aloud. 1-2 short sentences, ~40 words; code, paths, URLs only after a line ---]";

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
}

impl Speech {
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
            o.push_str(if p.len() <= 24 && !p.contains(['/', '\\', '(', '{', '=']) { p } else { "that (it's on the line)" });
        } else {
            o.push_str(p);
        }
    }
    let words: Vec<String> = o
        .split_whitespace()
        .map(|w| if w.contains("://") || w.starts_with("www.") { "the link's on the line".to_string() } else { w.replace(['*', '_'], " ").trim().to_string() })
        .filter(|w| !w.is_empty())
        .collect();
    words.join(" ")
}

/// Whole sentences of `s`: they end at `. ? ! ; …` followed by a space (closing quotes and
/// brackets go with them), not inside a decimal or after a common abbreviation. `finished`:
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
            while j < chars.len() && matches!(chars[j], '.' | '?' | '!' | '"' | '\'' | '”' | '’' | ')' | ']') {
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
