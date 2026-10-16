//! Calendar conversion without a time-zone database: reports always show UTC.

/// A UTC calendar date and time of day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

const SECS_PER_DAY: i64 = 86_400;

/// Converts Unix seconds to a proleptic-Gregorian UTC date.
///
/// Uses Howard Hinnant's `civil_from_days` algorithm
/// (<https://howardhinnant.github.io/date_algorithms.html#civil_from_days>), which
/// shifts the year to start on 1 March so the leap day is the last day of the cycle.
pub fn civil_from_unix(secs: i64) -> Civil {
    let days = secs.div_euclid(SECS_PER_DAY);
    let rem = secs.rem_euclid(SECS_PER_DAY);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    Civil {
        year,
        month,
        day,
        hour: (rem / 3600) as u8,
        minute: (rem % 3600 / 60) as u8,
        second: (rem % 60) as u8,
    }
}

/// `YYYY-MM-DD` (ISO 8601), unambiguous across locales.
pub fn iso_date(secs: i64) -> String {
    let c = civil_from_unix(secs);
    format!("{:04}-{:02}-{:02}", c.year, c.month, c.day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_known_dates() {
        assert_eq!(iso_date(0), "1970-01-01");
        // 2000-03-01 00:00:00 UTC, the day after a leap day in a 400-year leap year.
        assert_eq!(iso_date(951_868_800), "2000-03-01");
        assert_eq!(iso_date(951_868_800 - 1), "2000-02-29");
        // 2024-02-29 12:34:56 UTC
        let c = civil_from_unix(1_709_210_096);
        assert_eq!((c.year, c.month, c.day, c.hour, c.minute, c.second), (2024, 2, 29, 12, 34, 56));
        // 1900 is not a leap year; negative times work.
        assert_eq!(iso_date(-2_203_891_200), "1900-03-01");
        assert_eq!(iso_date(-1), "1969-12-31");
    }
}
