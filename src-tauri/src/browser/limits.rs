//! How often the extension may ask. A client that is not the extension can
//! still reach the pipe (see ARCHITECTURE.md), so listing a silo's labels,
//! raising the window and raising the fill dialog are all rationed: per
//! connection, and for lookups and `show` across all of them, since a new
//! connection is cheap.

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

/// `logins`, `search` and `show` on one connection: a popup opening and someone
/// typing, not a sweep through the alphabet.
pub fn lookups_per_connection() -> Bucket {
    Bucket::new(20, Duration::from_secs(1))
}

/// `logins`, `search` and `show` across every connection.
pub fn lookups_overall() -> Bucket {
    Bucket::new(60, Duration::from_millis(500))
}

/// `fill` on one connection. Each one is a dialog in front of the person.
pub fn fills_per_connection() -> Bucket {
    Bucket::new(3, Duration::from_secs(20))
}

/// `fill` across every connection. The per-connection ration alone was
/// reset by opening a new connection, so a client could raise the dialog
/// over and over by reconnecting.
pub fn fills_overall() -> Bucket {
    Bucket::new(4, Duration::from_secs(30))
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

/// Takes one lookup from this connection's ration, then from the app-wide
/// one. A connection past its own ration spends nothing of the other.
pub fn take_lookup(mine: &mut Bucket, overall: &mut Bucket, now: Instant) -> bool {
    mine.take(now) && overall.take(now)
}

/// A `show` this soon after the last one the app acted on is `busy`, so a
/// client cannot keep raising the window.
pub const SHOW_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Default)]
pub struct ShowGate {
    last: Option<Instant>,
}

impl ShowGate {
    /// Whether a `show` may raise the window now. Only one that may counts
    /// as the last.
    pub fn take(&mut self, now: Instant) -> bool {
        if self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) < SHOW_INTERVAL)
        {
            return false;
        }
        self.last = Some(now);
        true
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ShowRefused {
    TooMany,
    TooSoon,
}

/// A `show` spends the same rations as a lookup, then must be clear of the
/// last one.
pub fn admit_show(
    mine: &mut Bucket,
    overall: &mut Bucket,
    gate: &mut ShowGate,
    now: Instant,
) -> Result<(), ShowRefused> {
    if !take_lookup(mine, overall, now) {
        return Err(ShowRefused::TooMany);
    }
    if !gate.take(now) {
        return Err(ShowRefused::TooSoon);
    }
    Ok(())
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
    fn a_show_waits_three_seconds_after_the_last_one() {
        let start = Instant::now();
        let mut gate = ShowGate::default();
        assert!(gate.take(start));
        assert!(!gate.take(start));
        assert!(!gate.take(start + SHOW_INTERVAL - Duration::from_millis(1)));
        // A refused one does not push the next one further out.
        assert!(gate.take(start + SHOW_INTERVAL));
        assert!(!gate.take(start + SHOW_INTERVAL + Duration::from_secs(1)));
    }

    #[test]
    fn a_show_spends_the_lookup_rations() {
        let start = Instant::now();
        let later = start + SHOW_INTERVAL;
        let mut mine = lookups_per_connection();
        let mut overall = lookups_overall();
        let mut gate = ShowGate::default();
        assert_eq!(
            admit_show(&mut mine, &mut overall, &mut gate, start),
            Ok(())
        );
        assert_eq!(
            admit_show(&mut mine, &mut overall, &mut gate, start),
            Err(ShowRefused::TooSoon)
        );

        // This connection's lookups used up: refused, three seconds or not.
        while take_lookup(&mut mine, &mut lookups_overall(), start) {}
        assert_eq!(
            admit_show(&mut mine, &mut overall, &mut gate, start),
            Err(ShowRefused::TooMany)
        );

        // The app-wide ration used up by other connections: refused too.
        let mut fresh = lookups_per_connection();
        let mut drained = Bucket::new(1, Duration::from_secs(3600));
        assert!(drained.take(start));
        assert_eq!(
            admit_show(&mut fresh, &mut drained, &mut gate, later),
            Err(ShowRefused::TooMany)
        );
        assert_eq!(
            admit_show(&mut fresh, &mut overall, &mut gate, later),
            Ok(())
        );
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
