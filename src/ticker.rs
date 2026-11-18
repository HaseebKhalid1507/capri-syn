//! One wake-up thread for the whole view: it sleeps until the next frame/typing/fade
//! deadline and pokes the main loop. Idle means this thread is parked, nothing else runs.
//! (A plain thread is one hop; GPUI's timer goes timer thread → worker → main loop.)

use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Instant;

/// A new deadline, or one at the latest (an earlier one stands).
enum Wake {
    Set(Option<Instant>),
    By(Instant),
}

#[derive(Clone)]
pub struct Ticker {
    tx: mpsc::Sender<Wake>,
}

impl Ticker {
    /// Returns the ticker and the receiver the main loop awaits.
    pub fn start() -> (Ticker, async_channel::Receiver<()>) {
        let (tx, rx) = mpsc::channel::<Wake>();
        let (wake_tx, wake_rx) = async_channel::bounded(1);
        std::thread::Builder::new()
            .name("avatar-ticker".into())
            .spawn(move || {
                let mut deadline: Option<Instant> = None;
                loop {
                    let msg = match deadline {
                        Some(d) => rx.recv_timeout(d.saturating_duration_since(Instant::now())),
                        None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                    };
                    match msg {
                        Ok(Wake::Set(d)) => deadline = d,
                        Ok(Wake::By(t)) => deadline = Some(deadline.map_or(t, |d| d.min(t))),
                        Err(RecvTimeoutError::Timeout) => {
                            deadline = None;
                            // Full means a wake-up is already pending; that one will do.
                            if let Err(async_channel::TrySendError::Closed(_)) = wake_tx.try_send(()) {
                                return;
                            }
                        }
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            })
            .expect("spawn ticker thread");
        (Ticker { tx }, wake_rx)
    }

    /// Wake the main loop at `at` (replacing any earlier request), or never.
    pub fn set(&self, at: Option<Instant>) {
        self.tx.send(Wake::Set(at)).ok();
    }

    /// Wake the main loop by `at` at the latest (an earlier request stands).
    pub fn wake_by(&self, at: Instant) {
        self.tx.send(Wake::By(at)).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// `wake_by` brings a later deadline forward and never puts an earlier one off.
    #[test]
    fn wake_by_keeps_the_earlier_deadline() {
        let (t, rx) = Ticker::start();
        let t0 = Instant::now();
        t.set(Some(t0 + Duration::from_secs(5)));
        t.wake_by(t0 + Duration::from_millis(40));
        rx.recv_blocking().unwrap();
        let e = t0.elapsed();
        assert!(e >= Duration::from_millis(40) && e < Duration::from_millis(1000), "woke after {e:?}");
        let t1 = Instant::now();
        t.set(Some(t1 + Duration::from_millis(40)));
        t.wake_by(t1 + Duration::from_secs(5));
        rx.recv_blocking().unwrap();
        let e = t1.elapsed();
        assert!(e < Duration::from_millis(1000), "an earlier deadline put off: woke after {e:?}");
    }
}
