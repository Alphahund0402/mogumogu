//! Time as UTC unix seconds behind an injectable clock, so review periods,
//! heartbeats and backoff are testable without sleeping.
use std::fmt::Debug;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// UTC seconds since the unix epoch.
pub type Timestamp = i64;

pub const MINUTE: i64 = 60;
pub const HOUR: i64 = 60 * MINUTE;
pub const DAY: i64 = 24 * HOUR;

pub trait Clock: Send + Sync + Debug {
    fn now(&self) -> Timestamp;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
    }
}

/// Manually advanced clock for tests and deterministic fixtures.
#[derive(Debug)]
pub struct FixedClock(AtomicI64);

impl FixedClock {
    pub fn new(at: Timestamp) -> Self {
        Self(AtomicI64::new(at))
    }
    pub fn advance(&self, seconds: i64) {
        self.0.fetch_add(seconds, Ordering::SeqCst);
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0.load(Ordering::SeqCst)
    }
}

/// `2026-10-04T14:20:00Z`, the storage and JSON representation.
pub fn iso_utc(ts: Timestamp) -> String {
    let (y, m, d, hh, mm, ss) = civil(ts);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Parses the storage representation produced by [`iso_utc`].
pub fn parse_iso(text: &str) -> Option<Timestamp> {
    let b = text.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' || b[19] != b'Z'
    {
        return None;
    }
    let num = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    // Inverse of `civil` (days_from_civil).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * DAY + hh * HOUR + mm * MINUTE + ss)
}

/// `04.10.2026 14:20` in local time using the current UTC offset.
pub fn display_local(ts: Timestamp) -> String {
    let (y, m, d, hh, mm, _) = civil(ts + crate::platform::local_utc_offset_seconds());
    format!("{d:02}.{m:02}.{y:04} {hh:02}:{mm:02}")
}

/// `04.10.2026` in local time.
pub fn display_date(ts: Timestamp) -> String {
    let (y, m, d, ..) = civil(ts + crate::platform::local_utc_offset_seconds());
    format!("{d:02}.{m:02}.{y:04}")
}

/// Relative German description ("vor 3 Tagen") for activity lists.
pub fn display_relative(ts: Timestamp, now: Timestamp) -> String {
    let delta = now - ts;
    match delta {
        d if d < MINUTE => "gerade eben".into(),
        d if d < HOUR => format!("vor {} Min.", d / MINUTE),
        d if d < DAY => format!("vor {} Std.", d / HOUR),
        d if d < 2 * DAY => "gestern".into(),
        d if d < 30 * DAY => format!("vor {} Tagen", d / DAY),
        _ => display_date(ts),
    }
}

/// Days-to-civil conversion (H. Hinnant), valid for the proleptic Gregorian calendar.
fn civil(ts: Timestamp) -> (i64, u32, u32, i64, i64, i64) {
    let days = ts.div_euclid(DAY);
    let secs = ts.rem_euclid(DAY);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, secs / HOUR, (secs % HOUR) / MINUTE, secs % MINUTE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_formatting_matches_known_dates() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_791_123_600), "2026-10-04T14:20:00Z");
        assert_eq!(iso_utc(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn iso_parsing_is_the_inverse() {
        for ts in [0, 951_782_400, 1_791_123_600, 4_102_444_799] {
            assert_eq!(parse_iso(&iso_utc(ts)), Some(ts));
        }
        assert_eq!(parse_iso("Beispiel"), None);
        assert_eq!(parse_iso("2026-13-01T00:00:00Z"), None);
    }

    #[test]
    fn relative_time_is_human_readable() {
        assert_eq!(display_relative(100, 130), "gerade eben");
        assert_eq!(display_relative(0, 3 * DAY), "vor 3 Tagen");
    }

    #[test]
    fn fixed_clock_advances() {
        let clock = FixedClock::new(10);
        clock.advance(5);
        assert_eq!(clock.now(), 15);
    }
}
