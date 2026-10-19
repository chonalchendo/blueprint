//! The clock and its UTC text: one place reads the time and formats it, so every verb stamps alike. Parsing a
//! stamp back is `bp_change::intents::timestamp`, which the pure crates also need.

use bp_change::intents;
use bp_vault::Refusal;

/// The clock in Unix seconds: `BP_NOW` when set, so parity runs and replays see the clock the reference saw.
pub fn now() -> Result<i64, Refusal> {
    if let Ok(s) = std::env::var("BP_NOW") {
        return intents::timestamp(&s)
            .ok_or_else(|| Refusal::usage(format!("BP_NOW={s}"), "set BP_NOW to YYYY-MM-DDTHH:MM:SSZ, or unset it"));
    }
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    Ok(i64::try_from(since.expect("the clock is past 1970").as_secs()).expect("the clock fits i64"))
}

/// Unix seconds as `YYYY-MM-DDTHH:MM:SSZ`, the form every vault stamp takes: the civil date from days (Howard
/// Hinnant's algorithm), so no calendar crate is needed.
pub fn instant(secs: i64) -> String {
    let days = secs.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 { shifted + 3 } else { shifted - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    let second = secs.rem_euclid(86_400);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", second / 3600, second / 60 % 60, second % 60)
}

/// The UTC date of Unix seconds, `YYYY-MM-DD`: the date part of the instant.
pub fn date(secs: i64) -> String {
    instant(secs)[..10].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_instant_is_utc_and_reads_back() {
        assert_eq!(instant(1_792_281_600), "2026-10-18T00:00:00Z");
        assert_eq!(instant(0), "1970-01-01T00:00:00Z");
        assert_eq!(instant(951_782_400 + 3_723), "2000-02-29T01:02:03Z");
        assert_eq!(date(1_792_281_599), "2026-10-17");
        for secs in [0, 951_782_400, 1_792_281_599, 4_102_444_800] {
            assert_eq!(intents::timestamp(&instant(secs)), Some(secs));
        }
    }
}
