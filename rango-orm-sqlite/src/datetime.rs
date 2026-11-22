//! Dates are stored as TEXT. Databases written by rango up to 0.4 hold chrono's strings, and
//! SQL compares and sorts these columns as text, so new writes reproduce chrono's format exactly.

use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime, Time, UtcOffset};

pub(crate) fn format_datetime(v: OffsetDateTime) -> String {
    let v = v.to_offset(UtcOffset::UTC);
    let mut s = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        v.year(),
        v.month() as u8,
        v.day(),
        v.hour(),
        v.minute(),
        v.second()
    );
    push_fraction(&mut s, v.nanosecond());
    s.push_str("+00:00");
    s
}

pub(crate) fn format_date(v: Date) -> String {
    format!("{:04}-{:02}-{:02}", v.year(), v.month() as u8, v.day())
}

pub(crate) fn format_time(v: Time) -> String {
    let mut s = format!("{:02}:{:02}:{:02}", v.hour(), v.minute(), v.second());
    push_fraction(&mut s, v.nanosecond());
    s
}

fn push_fraction(s: &mut String, nanosecond: u32) {
    if nanosecond == 0 {
        return;
    }
    if nanosecond.is_multiple_of(1_000_000) {
        s.push_str(&format!(".{:03}", nanosecond / 1_000_000));
    } else if nanosecond.is_multiple_of(1_000) {
        s.push_str(&format!(".{:06}", nanosecond / 1_000));
    } else {
        s.push_str(&format!(".{:09}", nanosecond));
    }
}

pub(crate) fn parse_datetime(s: &str) -> Result<OffsetDateTime, String> {
    let mut s = s.to_string();
    if s.len() > 10 && s.as_bytes()[10] == b' ' {
        s.replace_range(10..11, "T");
    }
    OffsetDateTime::parse(&s, &Rfc3339)
        .map(|v| v.to_offset(UtcOffset::UTC))
        .map_err(|e| e.to_string())
}

pub(crate) fn parse_date(s: &str) -> Result<Date, String> {
    Date::parse(
        s,
        &time::macros::format_description!("[year]-[month]-[day]"),
    )
    .map_err(|e| e.to_string())
}

pub(crate) fn parse_time(s: &str) -> Result<Time, String> {
    Time::parse(
        s,
        &time::macros::format_description!(
            "[hour]:[minute]:[second][optional [.[subsecond digits:1+]]]"
        ),
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime, time};

    #[test]
    fn format_datetime_matches_chrono_rfc3339() {
        assert_eq!(
            format_datetime(datetime!(2026-11-05 22:00:00 UTC)),
            "2026-11-05T22:00:00+00:00"
        );
        assert_eq!(
            format_datetime(datetime!(2026-11-05 22:00:00.654321 UTC)),
            "2026-11-05T22:00:00.654321+00:00"
        );
        assert_eq!(
            format_datetime(datetime!(2026-11-05 22:00:00.500 UTC)),
            "2026-11-05T22:00:00.500+00:00"
        );
        assert_eq!(
            format_datetime(datetime!(2026-11-05 22:00:00.000000001 UTC)),
            "2026-11-05T22:00:00.000000001+00:00"
        );
        assert_eq!(
            format_datetime(datetime!(2026-11-05 23:00:00 +01:00)),
            "2026-11-05T22:00:00+00:00"
        );
    }

    #[test]
    fn format_date_and_time_match_chrono_display() {
        assert_eq!(format_date(date!(2026 - 11 - 05)), "2026-11-05");
        assert_eq!(format_time(time!(13:37:00)), "13:37:00");
        assert_eq!(format_time(time!(13:37:00.123456)), "13:37:00.123456");
    }

    #[test]
    fn parse_round_trips_formatted_values() {
        let dt = datetime!(2026-11-05 22:00:00.654321 UTC);
        assert_eq!(parse_datetime(&format_datetime(dt)).unwrap(), dt);

        let d = date!(2026 - 11 - 05);
        assert_eq!(parse_date(&format_date(d)).unwrap(), d);

        let t = time!(13:37:00.123456);
        assert_eq!(parse_time(&format_time(t)).unwrap(), t);
    }

    #[test]
    fn parse_datetime_accepts_space_and_offsets() {
        let space = parse_datetime("2026-11-05 22:00:00.5Z").unwrap();
        assert_eq!(space, datetime!(2026-11-05 22:00:00.500 UTC));
        assert_eq!(space.offset(), UtcOffset::UTC);

        let offset = parse_datetime("2026-11-05T23:00:00+01:00").unwrap();
        assert_eq!(offset, datetime!(2026-11-05 22:00:00 UTC));
        assert_eq!(offset.offset(), UtcOffset::UTC);
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse_datetime("nope").is_err());
        assert!(parse_datetime("2026-13-01").is_err());
        assert!(parse_date("nope").is_err());
        assert!(parse_date("2026-13-01").is_err());
    }
}
