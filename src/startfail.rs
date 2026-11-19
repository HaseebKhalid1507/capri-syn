//! A start of hers that fails where no one reads her log (S5): a click in the Start menu or the app
//! menu, a sign-in or login autostart. Her log has the error, as always; she also says it on
//! screen, once, as she exits (`platform::fail_notice`: Windows a message box, Linux a desktop
//! notification), bounded: she never waits long and never starts again. Only a start: `--check`,
//! `--set`, `-h`, `-V` and a bad command line are a script's or a terminal's, and say nothing more.
//! `SA_NO_NOTICE=1` keeps it to her log (tests), with a line saying what it would have said.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// The title of what she shows.
pub const TITLE: &str = "capri-syn";

/// What failed, as he reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// Her pack: not there, not a pack, a frame that doesn't decode, a name she can't take.
    Pack,
    /// Anything else on her way up: her window, the display, her identity.
    Other,
}

/// This run is a start (`starting`), so a failure from here on is said on screen too.
static STARTING: AtomicBool = AtomicBool::new(false);

/// From here on this run is a start of hers.
pub fn starting() {
    STARTING.store(true, Ordering::Relaxed);
}

/// An error from her pack on her way up: its text is the error's own, so her log reads as before.
#[derive(Debug)]
pub struct PackError(pub anyhow::Error);

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#}", self.0)
    }
}

impl std::error::Error for PackError {}

/// What a failed run's error is, as he reads it.
pub fn why_of(e: &anyhow::Error) -> Why {
    if e.downcast_ref::<PackError>().is_some() { Why::Pack } else { Why::Other }
}

/// Whether a failed run says it on screen too: a start's failure (`start`), where no one reads her
/// stderr (`seen`: Windows, a console or a redirect, not her own log file; Linux, a terminal), and
/// not "she's running already" (`running`: she's up, nothing failed).
pub fn on_screen(start: bool, seen: bool, running: bool) -> bool {
    start && !seen && !running
}

/// What she says: one plain sentence, then where her log is (`log`: the file her stderr goes to;
/// None, where it isn't a file: a journal).
pub fn text(why: Why, log: Option<&Path>) -> String {
    let what = match why {
        Why::Pack => "Capri couldn't start: her character pack is missing or damaged, and installing capri-syn again puts it back.",
        Why::Other => "Capri couldn't start.",
    };
    match log {
        Some(p) => format!("{what}\n\nWhat went wrong is in her log: {}", p.display()),
        None => format!("{what}\n\nTo see what went wrong, run synaps-avatar in a terminal."),
    }
}

/// A failed run, said on screen too when `on_screen` says so (after the error is in her log).
/// `running`: the error is "she's running already".
pub fn say(why: Why, running: bool) {
    if !on_screen(STARTING.load(Ordering::Relaxed), crate::platform::stderr_seen(), running) {
        return;
    }
    let text = text(why, crate::platform::stderr_file().as_deref());
    if std::env::var_os("SA_NO_NOTICE").is_some_and(|v| v == "1") {
        elog!("synaps-avatar: not said on screen (SA_NO_NOTICE=1): {}", text.lines().next().unwrap_or(""));
        return;
    }
    crate::platform::fail_notice(TITLE, &text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_start_no_one_watches_is_said_on_screen() {
        assert!(on_screen(true, false, false), "the Start menu, autostart: said");
        assert!(!on_screen(true, true, false), "a terminal (or a redirect on Windows): her stderr is read");
        assert!(!on_screen(true, false, true), "she's running already: nothing failed");
        assert!(!on_screen(false, false, false), "--check, --set, a bad command line: a script's");
        assert!(!on_screen(false, true, true));
    }

    #[test]
    fn it_says_what_failed_and_where_her_log_is() {
        let log = Path::new("/home/h/.local/state/synaps-avatar/launch.log");
        let t = text(Why::Pack, Some(log));
        assert!(t.starts_with("Capri couldn't start: her character pack is missing or damaged"), "{t}");
        assert!(t.ends_with("What went wrong is in her log: /home/h/.local/state/synaps-avatar/launch.log"), "{t}");
        assert_eq!(t.lines().next().map(|l| l.matches(". ").count()), Some(0), "one sentence: {t}");
        let t = text(Why::Other, None);
        assert_eq!(t, "Capri couldn't start.\n\nTo see what went wrong, run synaps-avatar in a terminal.");
    }

    /// A pack's error keeps its own text in her log (the chain, as `{:#}` prints it), and is told
    /// from any other.
    #[test]
    fn a_pack_error_reads_as_before_and_is_known() {
        use anyhow::Context as _;
        let inner = Err::<(), _>(std::io::Error::from(std::io::ErrorKind::NotFound)).context("pack: cannot read /p/sprites.json");
        let plain = format!("{:#}", inner.as_ref().unwrap_err());
        let e: anyhow::Error = PackError(inner.unwrap_err()).into();
        assert_eq!(format!("{e:#}"), plain);
        assert_eq!(why_of(&e), Why::Pack);
        assert_eq!(why_of(&anyhow::anyhow!("cannot open the window")), Why::Other);
        let running: anyhow::Error = crate::control::Running("another synaps-avatar is already running".into()).into();
        assert!(running.downcast_ref::<crate::control::Running>().is_some());
        assert_eq!(why_of(&running), Why::Other);
    }
}
