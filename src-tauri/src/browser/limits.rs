//! How often the extension may ask. A client that is not the extension can
//! still reach the pipe (see ARCHITECTURE.md), so listing a silo's labels
//! and raising the fill dialog are both rationed: per connection, and for
//! lookups across all of them, since a new connection is cheap.

use std::time::{Duration, Instant};

/// A token bucket: `capacity` at once, then one more every `refill`.
#[derive(Debug, Clone)]
pub struct Bucket {
    capacity: u32,
    refill: Duration,
    tokens: u32,
    /// When the last token was added, or the bucket started.
    since: Option<Instant>,
}

impl Bucket {
    pub const fn new(capacity: u32, refill: Duration) -> Self {
        Self {
            capacity,
            refill,
            tokens: capacity,
            since: None,
        }
    }

    /// Takes a token if one is left.
    pub fn take(&mut self, now: Instant) -> bool {
        let since = *self.since.get_or_insert(now);
        let earned =
            now.saturating_duration_since(since).as_nanos() / self.refill.as_nanos().max(1);
        if earned > 0 {
            let earned = u32::try_from(earned).unwrap_or(u32::MAX);
            self.tokens = self.tokens.saturating_add(earned).min(self.capacity);
            self.since = Some(since + self.refill * earned.min(self.capacity));
            if self.tokens == self.capacity {
                self.since = Some(now);
            }
        }
        if self.tokens == 0 {
            return false;
        }
        self.tokens -= 1;
        true
    }
}

/// `logins` and `search` on one connection: a popup opening and someone
/// typing, not a sweep through the alphabet.
pub fn lookups_per_connection() -> Bucket {
    Bucket::new(20, Duration::from_secs(1))
}

/// `logins` and `search` across every connection.
pub fn lookups_overall() -> Bucket {
    Bucket::new(60, Duration::from_millis(500))
}

/// `fill` on one connection. Each one is a dialog in front of the person.
pub fn fills_per_connection() -> Bucket {
    Bucket::new(3, Duration::from_secs(20))
}

/// After a fill was declined or timed out, the dialog stays down for this
/// long whoever asks, so a client cannot raise it again at once.
pub const COOLDOWN_AFTER_CANCEL: Duration = Duration::from_secs(10);

#[derive(Debug, Default)]
pub struct Cooldown {
    until: Option<Instant>,
}

impl Cooldown {
    pub fn start(&mut self, now: Instant) {
        self.until = Some(now + COOLDOWN_AFTER_CANCEL);
    }

    pub fn active(&self, now: Instant) -> bool {
        self.until.is_some_and(|until| now < until)
    }
}

/// What one connection has used.
pub struct ConnectionLimits {
    pub lookups: Bucket,
    pub fills: Bucket,
}

impl Default for ConnectionLimits {
    fn default() -> Self {
        Self {
            lookups: lookups_per_connection(),
            fills: fills_per_connection(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bucket_gives_its_capacity_then_one_per_interval() {
        let start = Instant::now();
        let mut bucket = Bucket::new(3, Duration::from_secs(1));
        assert!((0..3).all(|_| bucket.take(start)));
        assert!(!bucket.take(start));
        assert!(!bucket.take(start + Duration::from_millis(999)));
        assert!(bucket.take(start + Duration::from_secs(1)));
        assert!(!bucket.take(start + Duration::from_secs(1)));
        // A long wait refills to capacity, never past it.
        let later = start + Duration::from_secs(3600);
        assert!((0..3).all(|_| bucket.take(later)));
        assert!(!bucket.take(later));
    }

    #[test]
    fn a_sweep_of_two_letter_searches_is_cut_short() {
        let start = Instant::now();
        let mut bucket = lookups_per_connection();
        let allowed = (0..676)
            .filter(|n| bucket.take(start + Duration::from_millis(10 * n)))
            .count();
        assert!(allowed < 30, "{allowed} of 676 searches went through");
    }

    #[test]
    fn the_dialog_stays_down_for_a_while_after_a_cancel() {
        let start = Instant::now();
        let mut cooldown = Cooldown::default();
        assert!(!cooldown.active(start));
        cooldown.start(start);
        assert!(cooldown.active(start));
        assert!(cooldown.active(start + COOLDOWN_AFTER_CANCEL - Duration::from_millis(1)));
        assert!(!cooldown.active(start + COOLDOWN_AFTER_CANCEL));
    }

    #[test]
    fn one_connection_gets_a_few_fills_then_waits() {
        let start = Instant::now();
        let mut limits = ConnectionLimits::default();
        assert!((0..3).all(|_| limits.fills.take(start)));
        assert!(!limits.fills.take(start + Duration::from_secs(5)));
        assert!(limits.fills.take(start + Duration::from_secs(20)));
    }
}
