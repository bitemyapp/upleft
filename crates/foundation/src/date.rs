//! `Date` conversions the app layer needs byte for byte.

/// Seconds between 1970-01-01 and 2001-01-01, Foundation's
/// `Date.timeIntervalBetween1970AndReferenceDate`.
pub const TIME_INTERVAL_BETWEEN_1970_AND_REFERENCE_DATE: f64 = 978_307_200.0;

/// A point in time as Foundation stores it: seconds since 2001-01-01 UTC.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Default)]
pub struct Date {
    pub time_interval_since_reference_date: f64,
}

impl Date {
    pub fn from_reference(seconds: f64) -> Date {
        Date { time_interval_since_reference_date: seconds }
    }

    /// `Date(timeIntervalSince1970:)`.
    pub fn from_1970(seconds: f64) -> Date {
        Date { time_interval_since_reference_date: seconds - TIME_INTERVAL_BETWEEN_1970_AND_REFERENCE_DATE }
    }

    /// `Date()`: `CFAbsoluteTimeGetCurrent()`, wall-clock time.
    pub fn now() -> Date {
        let since_1970 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs_f64())
            .unwrap_or_else(|error| -error.duration().as_secs_f64());
        Date::from_1970(since_1970)
    }

    pub fn time_interval_since_1970(self) -> f64 {
        self.time_interval_since_reference_date + TIME_INTERVAL_BETWEEN_1970_AND_REFERENCE_DATE
    }

    /// `timeIntervalSince(_:)`.
    pub fn time_interval_since(self, other: Date) -> f64 {
        self.time_interval_since_reference_date - other.time_interval_since_reference_date
    }

    /// `addingTimeInterval(_:)`.
    pub fn adding(self, seconds: f64) -> Date {
        Date { time_interval_since_reference_date: self.time_interval_since_reference_date + seconds }
    }

    /// `JSONEncoder.DateEncodingStrategy.iso8601` (and `ISO8601DateFormatter`
    /// with its default `.withInternetDateTime` options): UTC, whole seconds,
    /// fractional seconds dropped toward the past, `Z` suffix.
    ///
    /// Before the 1582-10-15 cutover the ISO 8601 calendar is Julian, with
    /// astronomical year numbers (`0000`, then `-0001`…), as Foundation's is;
    /// a date before Julian Day 0 (-4712-01-01T12:00:00Z) prints as that day
    /// (probed at one point, -3e11 seconds).
    pub fn iso8601(self) -> String {
        let seconds = (self.time_interval_since_1970().floor() as i64).max(-210_866_760_000);
        let days = seconds.div_euclid(86_400);
        let second_of_day = seconds.rem_euclid(86_400);
        let (year, month, day) =
            if days < GREGORIAN_CUTOVER_DAYS { julian_from_days(days) } else { civil_from_days(days) };
        let year_text = if year < 0 { format!("-{:04}", -year) } else { format!("{year:04}") };
        format!(
            "{year_text}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
            second_of_day / 3600,
            (second_of_day / 60) % 60,
            second_of_day % 60
        )
    }

    /// `JSONDecoder.DateDecodingStrategy.iso8601`: swift-foundation's lenient
    /// ISO 8601 parser, as probed; see [`crate::decodable::parse_iso8601`].
    pub fn parse_iso8601(text: &str) -> Option<Date> {
        crate::decodable::parse_iso8601(text)
    }
}

/// Days from 1970-01-01 to 1582-10-15, the first Gregorian day.
const GREGORIAN_CUTOVER_DAYS: i64 = -141_427;

/// The Julian calendar date of a day number (days since 1970-01-01), counting
/// years astronomically; Julian 1582-10-05 is Gregorian 1582-10-15.
fn julian_from_days(days: i64) -> (i64, i64, i64) {
    // Days since Julian 0000-03-01, in March-based years.
    let anchor = 1582 * 365 + 1582 / 4 + (153 * 7 + 2) / 5 + 4;
    let n = days - GREGORIAN_CUTOVER_DAYS + anchor;
    let cycle = n.div_euclid(1461);
    let remainder = n.rem_euclid(1461);
    let year_of_cycle = (remainder / 365).min(3);
    let doy = remainder - year_of_cycle * 365;
    let y = cycle * 4 + year_of_cycle;
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Howard Hinnant's `civil_from_days`: proleptic Gregorian.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Values printed by Swift's JSONEncoder with `.iso8601` (see the crate's
    // tests/json_encoder.rs for how they were recorded).
    #[test]
    fn iso8601_matches_foundation() {
        for (seconds, text) in [
            (1_700_000_000.999, "2023-11-14T22:13:20Z"),
            (1_700_000_000.5, "2023-11-14T22:13:20Z"),
            (-0.5, "1969-12-31T23:59:59Z"),
            (-1.0, "1969-12-31T23:59:59Z"),
            (0.0, "1970-01-01T00:00:00Z"),
            (253_402_300_800.0, "10000-01-01T00:00:00Z"),
            (100_000_000_000.0, "5138-11-16T09:46:40Z"),
        ] {
            assert_eq!(Date::from_1970(seconds).iso8601(), text, "{seconds}");
        }
    }

    // Swift 6.4, macOS 26: `JSONEncoder` with `.iso8601` on
    // `Date(timeIntervalSince1970:)`.
    #[test]
    fn iso8601_is_julian_before_the_cutover() {
        for (seconds, text) in [
            (-12_219_379_200.0, "1582-10-04T00:00:00Z"),
            (-12_219_292_800.0, "1582-10-15T00:00:00Z"),
            (-12_218_515_200.0, "1582-10-24T00:00:00Z"),
            (-62_135_769_600.0, "0001-01-01T00:00:00Z"),
            (-62_167_392_000.0, "0000-01-01T00:00:00Z"),
            (-62_135_596_800.0, "0001-01-03T00:00:00Z"),
            (-11_676_096_000.0, "1600-01-01T00:00:00Z"),
            (-12_219_292_800.5, "1582-10-04T23:59:59Z"),
            (-99_999_999_999.0, "-1199-02-26T14:13:21Z"),
            (-300_000_000_000.0, "-4712-01-01T12:00:00Z"),
        ] {
            assert_eq!(Date::from_1970(seconds).iso8601(), text, "{seconds}");
        }
    }

    #[test]
    fn iso8601_parses_what_json_decoder_accepts() {
        assert_eq!(Date::parse_iso8601("2023-11-14T22:13:20Z").unwrap().time_interval_since_1970(), 1_700_000_000.0);
        assert_eq!(Date::parse_iso8601("2023-11-14T22:13:20.5Z").unwrap().time_interval_since_1970(), 1_700_000_000.5);
        assert_eq!(Date::parse_iso8601("2023-11-14T22:13:20+01:00").unwrap().time_interval_since_1970(), 1_699_996_400.0);
        assert!(Date::parse_iso8601("2023-11-14 22:13:20Z").is_none());
    }
}
