//! Pure UTC timestamp formatting — no dependencies, so the audit log gets
//! human-readable times without pulling in a date crate.

/// Format seconds since the Unix epoch as RFC3339 UTC, e.g. `2026-06-17T14:03:09Z`.
///
/// Uses Howard Hinnant's `civil_from_days` algorithm; valid across the full
/// proleptic Gregorian range that fits in `i64`.
pub fn rfc3339_utc(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86_400);
    let sod = unix_secs.rem_euclid(86_400);
    let (hour, minute, second) = (sod / 3600, (sod % 3600) / 60, sod % 60);

    // civil_from_days: days-since-epoch -> (year, month, day)
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097; // day of era, [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year_civ = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if month <= 2 { year_civ + 1 } else { year_civ };

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_instants() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(rfc3339_utc(86_400), "1970-01-02T00:00:00Z");
    }

    proptest::proptest! {
        /// Never panics, and always produces a well-formed-looking RFC3339 stamp.
        #[test]
        fn total_and_shaped(secs in i64::MIN / 2..i64::MAX / 2) {
            let s = rfc3339_utc(secs);
            proptest::prop_assert!(s.contains('T') && s.ends_with('Z'));
            proptest::prop_assert!(s.len() >= 20);
        }
    }
}
