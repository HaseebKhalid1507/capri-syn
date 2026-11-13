//! Dev control channel: a Unix socket with newline commands. It stands in for the
//! session stream phase 2 brings; both produce the same `Command`s, so the
//! command → state boundary stays in `view::Avatar::apply`.

use crate::anim::Mood;
use anyhow::{Context as _, Result, bail};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum Command {
    Mood(Mood),
    Clip(String),
    Say(String),
    /// Put the avatar's ground point (feet) at root pixel (x, y): a teleport, or an
    /// instant move with `now`.
    Pos(i32, i32, bool),
    /// Sit (or stand) on a window's top edge: the focused window, or `win`.
    Perch { sit: bool, win: Option<u32> },
    /// Leave the perch: teleport back to the ground.
    Unperch,
    /// Dev: a drag released right where she is (the release snap).
    Drop,
    /// Dev: the window stack as she sees it (bottom → top), and the work areas.
    Stack,
    /// Dev: the right-click menu, at her head (as a right press there would open it).
    Menu,
    /// One-line JSON snapshot of the avatar's state.
    State,
    /// Glitch out, then quit.
    Hide,
    Quit,
    /// Dev: a glitch burst.
    Glitch,
    /// Bubble paging: +1 / -1.
    Page(i32),
    /// Music mode (headphones, notes, head-bob to the pack's BPM).
    Music(bool),
    /// Collapse the bubble to the status tag (true) or expand it again.
    Collapse(bool),
    /// Internal: from the X watcher thread.
    Watch(crate::platform::x11::WatchEvent),
    /// Send text to the session, as if typed (submit, or steer while a turn runs).
    Submit(String),
    /// Abort the running turn.
    Abort,
    /// Dev: show a prompt panel (confirm or secret) that answers nowhere.
    FakePrompt { secret: bool, text: String },
    /// Dev: close that panel, as if answered.
    FakePromptOff,
    /// The brain: on/off, DND, state, time scale, test senses, forced decisions, synthetic
    /// session signals.
    Brain(BrainCmd),
    /// Re-read ~/.config/synaps-avatar/settings.toml.
    SettingsReload,
    /// A synthetic tap on her head (headpat) or body, as a click would.
    Tap { head: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub enum BrainCmd {
    On,
    Off,
    /// Minutes; None = DND off.
    Dnd(Option<f64>),
    State,
    Scale(f64),
    Seed(u64),
    Force(crate::brain::Force),
    Event(crate::brain::SessionSignal),
    Sense(Sense),
}

/// Test senses. `None` = back to the real one.
#[derive(Debug, Clone, PartialEq)]
pub enum Sense {
    /// A window id (client or frame), `Some(None)` = nothing focused.
    Focus(Option<Option<u32>>),
    Cursor(Option<(i32, i32)>),
    /// Seconds since the last input, growing from now (negative: "active", stays 0).
    Idle(Option<f64>),
    Fullscreen(Option<bool>),
    /// He is (not) near her, whatever the windows say.
    Near(Option<bool>),
    /// Only this root rect is hers (areas and windows outside are left alone).
    Arena(Option<(i32, i32, i32, i32)>),
    /// Only these windows may be sat on, hung over or focused.
    Windows(Option<Vec<u32>>),
    /// Every sense back to real.
    Real,
}

fn win_id(w: &str) -> Result<u32> {
    let v = w.trim_start_matches("0x");
    let radix = if v.len() < w.len() { 16 } else { 10 };
    u32::from_str_radix(v, radix).with_context(|| format!("bad window id `{w}`"))
}

fn parse_brain(rest: &str) -> Result<BrainCmd> {
    use crate::brain::{Force, SessionSignal};
    let w: Vec<&str> = rest.split_whitespace().collect();
    let num = |i: usize| -> Result<f64> { w.get(i).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()).context("a number") };
    Ok(match w.as_slice() {
        ["on"] => BrainCmd::On,
        ["off"] => BrainCmd::Off,
        ["state"] | [] => BrainCmd::State,
        ["dnd", "off"] => BrainCmd::Dnd(None),
        ["dnd"] => BrainCmd::Dnd(Some(60.0)),
        ["dnd", _] => BrainCmd::Dnd(Some(num(1)?.max(0.1))),
        ["scale", _] => BrainCmd::Scale(num(1)?.clamp(0.01, 1000.0)),
        ["seed", n] => BrainCmd::Seed(n.parse().context("seed: a number")?),
        ["force", f] => BrainCmd::Force(match *f {
            "roam" => Force::Roam,
            "seek" | "bid" => Force::Seek,
            "summon" => Force::Summon,
            "sulk" => Force::Sulk,
            "nap" => Force::Nap,
            _ => bail!("brain force roam|seek|summon|sulk|nap"),
        }),
        ["event", "prompt", id] => BrainCmd::Event(SessionSignal::Prompt(id.parse().context("prompt id")?)),
        ["event", "prompt"] => BrainCmd::Event(SessionSignal::Prompt(1)),
        ["event", "done"] => BrainCmd::Event(SessionSignal::Done),
        ["event", "error"] => BrainCmd::Event(SessionSignal::Error { own_abort: false }),
        ["event", "abort"] => BrainCmd::Event(SessionSignal::Error { own_abort: true }),
        ["event", "cleared" | "answered"] => BrainCmd::Event(SessionSignal::Cleared),
        ["sense", "real"] => BrainCmd::Sense(Sense::Real),
        ["sense", "focus", "real"] => BrainCmd::Sense(Sense::Focus(None)),
        ["sense", "focus", "none"] => BrainCmd::Sense(Sense::Focus(Some(None))),
        ["sense", "focus", id] => BrainCmd::Sense(Sense::Focus(Some(Some(win_id(id)?)))),
        ["sense", "cursor", "real"] => BrainCmd::Sense(Sense::Cursor(None)),
        ["sense", "cursor", _, _] => BrainCmd::Sense(Sense::Cursor(Some((num(2)? as i32, num(3)? as i32)))),
        ["sense", "idle", "real"] => BrainCmd::Sense(Sense::Idle(None)),
        ["sense", "idle", "active"] => BrainCmd::Sense(Sense::Idle(Some(-1.0))),
        ["sense", "idle", _] => BrainCmd::Sense(Sense::Idle(Some(num(2)?.max(0.0)))),
        ["sense", "fullscreen", "real"] => BrainCmd::Sense(Sense::Fullscreen(None)),
        ["sense", "fullscreen", "on"] => BrainCmd::Sense(Sense::Fullscreen(Some(true))),
        ["sense", "fullscreen", "off"] => BrainCmd::Sense(Sense::Fullscreen(Some(false))),
        ["sense", "near", "real"] => BrainCmd::Sense(Sense::Near(None)),
        ["sense", "near", "on"] => BrainCmd::Sense(Sense::Near(Some(true))),
        ["sense", "near", "off"] => BrainCmd::Sense(Sense::Near(Some(false))),
        ["sense", "arena", "off"] => BrainCmd::Sense(Sense::Arena(None)),
        ["sense", "arena", _, _, _, _] => {
            BrainCmd::Sense(Sense::Arena(Some((num(2)? as i32, num(3)? as i32, num(4)? as i32, num(5)? as i32))))
        }
        ["sense", "windows", "any"] => BrainCmd::Sense(Sense::Windows(None)),
        ["sense", "windows", ids @ ..] if !ids.is_empty() => {
            BrainCmd::Sense(Sense::Windows(Some(ids.iter().map(|i| win_id(i)).collect::<Result<_>>()?)))
        }
        _ => bail!(
            "usage: brain on|off|state|dnd [MIN|off]|scale K|seed N|force roam|seek|summon|sulk|nap|event prompt [ID]|done|error|abort|cleared|sense real|focus ID|none|real|cursor X Y|real|idle S|active|real|fullscreen on|off|real|near on|off|real|arena X Y W H|off|windows ID..|any"
        ),
    })
}

impl Command {
    pub fn parse(line: &str) -> Result<Command> {
        let line = line.trim();
        let (verb, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        Ok(match verb {
            "mood" => Command::Mood(Mood::parse(rest).with_context(|| {
                let all: Vec<_> = Mood::ALL.iter().map(|m| m.key()).collect();
                format!("unknown mood `{rest}` (one of: {})", all.join(", "))
            })?),
            "clip" if !rest.is_empty() => Command::Clip(rest.to_owned()),
            // `\n` in the line stands for a newline (the protocol is one command per line).
            "say" if !rest.is_empty() => Command::Say(rest.replace("\\n", "\n")),
            "pos" => {
                let mut it = rest.split_whitespace();
                let num = |v: Option<&str>| v.and_then(|v| v.parse::<i32>().ok());
                match (num(it.next()), num(it.next()), it.next(), it.next()) {
                    (Some(x), Some(y), None, None) => Command::Pos(x, y, false),
                    (Some(x), Some(y), Some("now"), None) => Command::Pos(x, y, true),
                    _ => bail!("usage: pos <x> <y> [now]"),
                }
            }
            "perch" => {
                let mut sit = true;
                let mut win = None;
                for w in rest.split_whitespace() {
                    match w {
                        "off" => return Ok(Command::Unperch),
                        "sit" => sit = true,
                        "stand" => sit = false,
                        _ => {
                            let v = w.trim_start_matches("0x");
                            let radix = if v.len() < w.len() { 16 } else { 10 };
                            win = Some(u32::from_str_radix(v, radix).with_context(|| format!("perch: bad window id `{w}`"))?);
                        }
                    }
                }
                Command::Perch { sit, win }
            }
            "page" => match rest {
                "next" | "+" => Command::Page(1),
                "prev" | "-" => Command::Page(-1),
                _ => bail!("usage: page next|prev"),
            },
            "drop" => Command::Drop,
            "stack" => Command::Stack,
            "menu" => Command::Menu,
            "collapse" => Command::Collapse(true),
            "expand" => Command::Collapse(false),
            "glitch" => Command::Glitch,
            "music" => match rest {
                "on" | "" => Command::Music(true),
                "off" => Command::Music(false),
                _ => bail!("usage: music on|off"),
            },
            "submit" if !rest.is_empty() => Command::Submit(rest.to_owned()),
            "abort" => Command::Abort,
            "prompt" => {
                let (kind, text) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                let text = if text.trim().is_empty() { "Allow the bash tool to run `ls ~`?".to_string() } else { text.trim().to_string() };
                match kind {
                    "confirm" => Command::FakePrompt { secret: false, text },
                    "secret" => Command::FakePrompt { secret: true, text },
                    "off" => Command::FakePromptOff,
                    _ => bail!("usage: prompt confirm|secret [text] | prompt off"),
                }
            }
            "state" => Command::State,
            "brain" => Command::Brain(parse_brain(rest)?),
            "settings" if rest == "reload" => Command::SettingsReload,
            "tap" => match rest {
                "head" => Command::Tap { head: true },
                "body" | "" => Command::Tap { head: false },
                _ => bail!("usage: tap head|body"),
            },
            "hide" => Command::Hide,
            "quit" => Command::Quit,
            _ => bail!(
                "unknown command `{line}` (mood <name> | clip <id> | say <text> | pos <x> <y> [now] | perch [sit|stand] [win] | perch off | drop | stack | page next|prev | collapse | expand | music on|off | glitch | submit <text> | abort | prompt confirm|secret [text] | brain … | settings reload | tap head|body | state | hide | quit)"
            ),
        })
    }
}

pub type Reply = std::sync::mpsc::Sender<String>;


pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("synaps-avatar.sock")
}

/// Bind the socket. A live socket means another avatar is running: refuse. A dead one
/// (left by a killed process) is replaced.
pub fn bind(path: &Path) -> Result<UnixListener> {
    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            bail!("another synaps-avatar is already running ({} answers)", path.display());
        }
        std::fs::remove_file(path).with_context(|| format!("cannot remove stale {}", path.display()))?;
    }
    UnixListener::bind(path).with_context(|| format!("cannot bind {}", path.display()))
}

/// Serve connections on a thread; every parsed command goes to `tx` with a reply channel.
pub fn serve(listener: UnixListener, tx: async_channel::Sender<(Command, Reply)>) {
    std::thread::Builder::new()
        .name("control".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let tx = tx.clone();
                std::thread::spawn(move || handle(stream, tx));
            }
        })
        .expect("spawn control thread");
}

fn handle(stream: UnixStream, tx: async_channel::Sender<(Command, Reply)>) {
    let Ok(mut out) = stream.try_clone() else { return };
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match Command::parse(&line) {
            Ok(cmd) => {
                let (rtx, rrx) = std::sync::mpsc::channel();
                if tx.send_blocking((cmd, rtx)).is_err() {
                    return;
                }
                rrx.recv_timeout(Duration::from_secs(3)).unwrap_or_else(|_| "ok".into())
            }
            Err(e) => format!("error: {e:#}"),
        };
        if writeln!(out, "{reply}").is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brain_commands_parse() {
        use crate::brain::{Force, SessionSignal};
        let p = |s: &str| match Command::parse(s).unwrap() {
            Command::Brain(b) => b,
            c => panic!("{c:?}"),
        };
        assert_eq!(p("brain dnd 30"), BrainCmd::Dnd(Some(30.0)));
        assert_eq!(p("brain dnd off"), BrainCmd::Dnd(None));
        assert_eq!(p("brain force seek"), BrainCmd::Force(Force::Seek));
        assert_eq!(p("brain event prompt 7"), BrainCmd::Event(SessionSignal::Prompt(7)));
        assert_eq!(p("brain sense focus 0x1a"), BrainCmd::Sense(Sense::Focus(Some(Some(26)))));
        assert_eq!(p("brain sense cursor 10 20"), BrainCmd::Sense(Sense::Cursor(Some((10, 20)))));
        assert_eq!(p("brain sense windows 0x10 17"), BrainCmd::Sense(Sense::Windows(Some(vec![16, 17]))));
        assert_eq!(p("brain scale 20"), BrainCmd::Scale(20.0));
        assert!(Command::parse("brain fly").is_err());
        assert!(matches!(Command::parse("tap head").unwrap(), Command::Tap { head: true }));
        assert!(matches!(Command::parse("settings reload").unwrap(), Command::SettingsReload));
    }
}
