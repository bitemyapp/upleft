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
    /// Foundation switches to the Julian calendar before the 1582 Gregorian
    /// cutover; this uses the proleptic Gregorian calendar throughout, so
    /// dates before 1582-10-15 differ. No Downright timestamp is that old.
    pub fn iso8601(self) -> String {
        let seconds = self.time_interval_since_1970().floor() as i64;
        let days = seconds.div_euclid(86_400);
        let second_of_day = seconds.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        let year_text = if year < 0 { format!("-{:04}", -year) } else { format!("{year:04}") };
        format!(
            "{year_text}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
            second_of_day / 3600,
            (second_of_day / 60) % 60,
            second_of_day % 60
        )
    }

    /// `JSONDecoder.DateDecodingStrategy.iso8601`: `yyyy-MM-ddTHH:mm:ss`,
    /// optional fractional seconds, then `Z` or `±HH:MM` / `±HHMM`.
    pub fn parse_iso8601(text: &str) -> Option<Date> {
        let bytes = text.as_bytes();
        let digits = |range: std::ops::Range<usize>| -> Option<i64> {
            let slice = bytes.get(range)?;
            if slice.is_empty() || !slice.iter().all(u8::is_ascii_digit) {
                return None;
            }
            std::str::from_utf8(slice).ok()?.parse().ok()
        };
        if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' || bytes[13] != b':' || bytes[16] != b':' {
            return None;
        }
        let (year, month, day) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
        let (hour, minute, second) = (digits(11..13)?, digits(14..16)?, digits(17..19)?);
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
            return None;
        }
        let mut index = 19;
        let mut fraction = 0.0;
        if bytes[index] == b'.' {
            let start = index + 1;
            index = start;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            if index == start {
                return None;
            }
            fraction = format!("0.{}", &text[start..index]).parse().ok()?;
        }
        let offset = match bytes.get(index)? {
            b'Z' if index + 1 == bytes.len() => 0,
            sign @ (b'+' | b'-') => {
                let rest = &text[index + 1..];
                let (h, m) = if rest.len() == 5 && rest.as_bytes()[2] == b':' {
                    (rest[0..2].parse::<i64>().ok()?, rest[3..5].parse::<i64>().ok()?)
                } else if rest.len() == 4 {
                    (rest[0..2].parse::<i64>().ok()?, rest[2..4].parse::<i64>().ok()?)
                } else {
                    return None;
                };
                let magnitude = h * 3600 + m * 60;
                if *sign == b'+' { magnitude } else { -magnitude }
            }
            _ => return None,
        };
        let days = days_from_civil(year, month, day);
        let seconds = days * 86_400 + hour * 3600 + minute * 60 + second - offset;
        Some(Date::from_1970(seconds as f64 + fraction))
    }
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

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
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

    #[test]
    fn iso8601_parses_what_json_decoder_accepts() {
        assert_eq!(Date::parse_iso8601("2023-11-14T22:13:20Z").unwrap().time_interval_since_1970(), 1_700_000_000.0);
        assert_eq!(Date::parse_iso8601("2023-11-14T22:13:20.5Z").unwrap().time_interval_since_1970(), 1_700_000_000.5);
        assert_eq!(Date::parse_iso8601("2023-11-14T22:13:20+01:00").unwrap().time_interval_since_1970(), 1_699_996_400.0);
        assert!(Date::parse_iso8601("2023-11-14 22:13:20Z").is_none());
    }
}
