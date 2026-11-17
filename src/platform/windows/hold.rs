//! Hold-to-talk on a key RegisterHotKey reports only the press of (`[voice] hold_to_talk`, opt-in;
//! a tap stays the default). The press is `TalkPress`. From then on the key is polled every
//! `POLL` from her own key thread (`GetAsyncKeyState`, OBS's pattern for its hotkeys) until it is
//! up: `TalkRelease`. No low-level keyboard hook. A key the poll can't see as down (released, or
//! the secure desktop: the lock, a UAC prompt) ends the hold. The avatar tells a tap from a hold by
//! the time between the two, as for the Linux binding's `talk press` / `talk release`.

use super::KeyEv;
use std::time::Duration;

/// Between two looks at a held key.
pub(super) const POLL: Duration = Duration::from_millis(25);

#[derive(Debug, Default)]
pub(super) struct Hold {
    held: bool,
}

impl Hold {
    /// Whether a press is held (the key thread polls only then).
    pub(super) fn held(&self) -> bool {
        self.held
    }

    /// Her key's hotkey fired: a press, unless one is held already (`MOD_NOREPEAT` keeps the
    /// auto-repeats out; this keeps out a second press before the poll saw the first's release).
    pub(super) fn pressed(&mut self) -> Option<KeyEv> {
        (!std::mem::replace(&mut self.held, true)).then_some(KeyEv::TalkPress)
    }

    /// A poll: `down` is the key's state now.
    pub(super) fn poll(&mut self, down: bool) -> Option<KeyEv> {
        (self.held && !down).then(|| {
            self.held = false;
            KeyEv::TalkRelease
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_then_its_release() {
        let mut h = Hold::default();
        assert_eq!(h.poll(false), None, "nothing held: a poll says nothing");
        assert_eq!(h.pressed(), Some(KeyEv::TalkPress));
        assert!(h.held());
        assert_eq!(h.poll(true), None, "still down");
        assert_eq!(h.pressed(), None, "a second hotkey while held is no new press");
        assert_eq!(h.poll(true), None);
        assert_eq!(h.poll(false), Some(KeyEv::TalkRelease));
        assert!(!h.held());
        assert_eq!(h.poll(false), None, "one release per press");
    }

    #[test]
    fn a_tap_shorter_than_a_poll_is_a_press_and_a_release() {
        let mut h = Hold::default();
        // Released before the first look (or the lock took the key): the avatar sees a tap.
        assert_eq!(h.pressed(), Some(KeyEv::TalkPress));
        assert_eq!(h.poll(false), Some(KeyEv::TalkRelease));
        assert_eq!(h.pressed(), Some(KeyEv::TalkPress), "the next press is a press again");
    }
}
