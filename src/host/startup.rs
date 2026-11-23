//! Bounded output discovery before the event watcher/control server can start.

use std::time::Duration;

/// Yield between lookups, never extending the five-second startup budget.
pub(super) fn retry_after(elapsed: Duration) -> Option<Duration> {
    let left = Duration::from_secs(5).checked_sub(elapsed)?;
    (!left.is_zero()).then(|| left.min(Duration::from_millis(50)))
}

#[cfg(test)]
mod tests {
    use super::retry_after;
    use std::time::Duration;

    #[test]
    fn discovery_retries_yield_for_fifty_milliseconds() {
        assert_eq!(retry_after(Duration::ZERO), Some(Duration::from_millis(50)));
        assert_eq!(retry_after(Duration::from_millis(120)), Some(Duration::from_millis(50)));
    }

    #[test]
    fn final_retry_cannot_extend_the_deadline() {
        assert_eq!(retry_after(Duration::from_millis(4999)), Some(Duration::from_millis(1)));
        assert_eq!(retry_after(Duration::from_secs(5)), None);
        assert_eq!(retry_after(Duration::from_secs(6)), None);
    }

    #[test]
    fn a_missing_output_does_not_wait_forever() {
        let mut elapsed = Duration::ZERO;
        let mut attempts = 0;
        while let Some(delay) = retry_after(elapsed) {
            assert!(!delay.is_zero());
            elapsed += delay;
            attempts += 1;
            assert!(attempts <= 100, "startup must stop even if no wl_output arrives");
        }
        assert_eq!(attempts, 100);
        assert_eq!(elapsed, Duration::from_secs(5));
    }
}
