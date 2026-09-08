//! Reference-style labels, calculated from UTC timestamps rather than demo copy.
use std::time::{SystemTime, UNIX_EPOCH};

const DAY_MS: u64 = 86_400_000;

fn today() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64 / 86_400
}

fn relative(day: i64, now: i64) -> Option<&'static str> {
    if day == now { Some("Today") }
    else if day == now - 1 { Some("Yesterday") }
    else { None }
}

fn civil_date(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    (year + i64::from(month <= 2), month, day)
}

fn iso_day(timestamp: &str) -> Option<i64> {
    // Offset-bearing values retain their explicit date in the caller. Do not
    // pretend their day is UTC without converting the offset.
    if timestamp.len() != 10 && !timestamp.ends_with('Z') { return None; }
    let date = timestamp.get(..10)?;
    if date.as_bytes().get(4) != Some(&b'-') || date.as_bytes().get(7) != Some(&b'-') { return None; }
    let year = date.get(..4)?.parse::<i64>().ok()?;
    let month = date.get(5..7)?.parse::<i64>().ok()?;
    let day = date.get(8..10)?.parse::<i64>().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) { return None; }
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let result = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    (civil_date(result) == (year, month, day)).then_some(result)
}

pub(super) fn relative_session_day(timestamp: &str) -> Option<&'static str> {
    relative(iso_day(timestamp)?, today())
}

pub(super) fn message_label(timestamp_ms: u64) -> String {
    label_at(timestamp_ms, today())
}

fn label_at(timestamp_ms: u64, now: i64) -> String {
    let day = (timestamp_ms / DAY_MS) as i64;
    let minutes = timestamp_ms / 60_000 % 1_440;
    let date = relative(day, now).map(str::to_owned).unwrap_or_else(|| {
        let (year, month, day) = civil_date(day);
        let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        if civil_date(now).0 == year { format!("{} {day}", months[(month - 1) as usize]) }
        else { format!("{} {day}, {year}", months[(month - 1) as usize]) }
    });
    format!("{date} · {:02}:{:02}", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_labels_roll_over_at_utc_midnight() {
        let day = iso_day("2026-09-06T11:40:00.000Z").unwrap();
        let stamp = day as u64 * DAY_MS + (11 * 60 + 40) * 60_000;
        assert_eq!(label_at(stamp, day), "Today · 11:40");
        assert_eq!(label_at(stamp, day + 1), "Yesterday · 11:40");
        assert_eq!(label_at(stamp, day + 2), "Sep 6 · 11:40");
    }

    #[test]
    fn civil_dates_round_trip_and_invalid_or_offset_dates_stay_unmodified() {
        for date in ["1970-01-01", "2000-02-29", "2024-12-31", "2026-01-01"] {
            let days = iso_day(date).unwrap();
            let (y, m, d) = civil_date(days);
            assert_eq!(format!("{y:04}-{m:02}-{d:02}"), date);
        }
        assert_eq!(iso_day("1970-01-01"), Some(0));
        for date in ["2026-02-29", "2026-13-01", "2026-04-31", "bad", "2026-09-06T01:00:00+05:30"] {
            assert_eq!(iso_day(date), None);
        }
    }
}
