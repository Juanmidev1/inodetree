//! Human-readable formatting shared by the CLI and the GUI.

const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
const SECONDS_PER_DAY: i64 = 86_400;

#[must_use]
pub fn format_size(bytes: u64) -> String {
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Groups digits in threes (`1,482,910`) for counters in the status bar.
#[must_use]
pub fn format_count(count: u64) -> String {
    let digits = count.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// Formats as UTC because local time zones would need a time library for one column.
#[must_use]
pub fn format_mtime(epoch_secs: i64) -> String {
    let (year, month, day) = civil_from_days(epoch_secs.div_euclid(SECONDS_PER_DAY));
    let seconds_of_day = epoch_secs.rem_euclid(SECONDS_PER_DAY);
    let (hour, minute) = (seconds_of_day / 3600, seconds_of_day % 3600 / 60);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

/// Howard Hinnant's days-to-civil-date algorithm, valid for the proleptic Gregorian calendar.
fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let shifted = days_since_epoch + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_switch_unit_at_1024() {
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(5 * 1024 * 1024 * 1024), "5.0 GiB");
    }

    #[test]
    fn counts_are_grouped_in_thousands() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1_482_910), "1,482,910");
    }

    #[test]
    fn mtime_is_formatted_in_utc() {
        assert_eq!(format_mtime(0), "1970-01-01 00:00");
        assert_eq!(format_mtime(1_700_000_000), "2023-11-14 22:13");
        assert_eq!(format_mtime(-86_400), "1969-12-31 00:00");
    }
}
