//! Calendar helpers over ISO date strings (YYYY-MM-DD) in local time, mirroring the web app's
//! `utils/dates.ts`.

use chrono::{Datelike, Duration, Local, NaiveDate, SecondsFormat, Utc};

const MONTHS_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

pub fn today_iso() -> String {
    to_iso(today())
}

pub fn to_iso(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

pub fn parse_iso(iso: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(iso, "%Y-%m-%d").ok()
}

/// Current instant as an ISO timestamp, like `new Date().toISOString()`.
pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn add_days(date: NaiveDate, days: i64) -> NaiveDate {
    date + Duration::days(days)
}

/// Like JavaScript's `setMonth(getMonth() + months)`: the day of month is kept and overflows into
/// the following month when the target month is shorter (Jan 31 + 1 month = Mar 3).
pub fn add_months(date: NaiveDate, months: i32) -> NaiveDate {
    let total = date.year() * 12 + date.month0() as i32 + months;
    let first = NaiveDate::from_ymd_opt(total.div_euclid(12), total.rem_euclid(12) as u32 + 1, 1)
        .expect("valid month");
    add_days(first, i64::from(date.day()) - 1)
}

/// Like JavaScript's `setFullYear(getFullYear() + years)`: Feb 29 rolls over to Mar 1.
pub fn add_years(date: NaiveDate, years: i32) -> NaiveDate {
    add_months(date, years * 12)
}

/// "Nov 1, 2026", or "—" with no date.
pub fn format_date(iso: Option<&str>) -> String {
    match iso.and_then(parse_iso) {
        Some(d) => format_naive(d),
        None => "—".to_string(),
    }
}

pub fn format_naive(d: NaiveDate) -> String {
    format!(
        "{} {}, {}",
        MONTHS_SHORT[d.month0() as usize],
        d.day(),
        d.year()
    )
}

/// Whether the date is before today.
pub fn is_overdue(iso: Option<&str>) -> bool {
    iso.and_then(parse_iso).is_some_and(|d| d < today())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(iso: &str) -> NaiveDate {
        parse_iso(iso).unwrap()
    }

    #[test]
    fn month_arithmetic_overflows_like_javascript() {
        assert_eq!(to_iso(add_months(d("2026-07-27"), 1)), "2026-08-27");
        assert_eq!(to_iso(add_months(d("2026-12-15"), 1)), "2027-01-15");
        assert_eq!(to_iso(add_months(d("2026-01-31"), 1)), "2026-03-03");
        assert_eq!(to_iso(add_months(d("2026-03-10"), -3)), "2025-12-10");
        assert_eq!(to_iso(add_years(d("2028-02-29"), 1)), "2029-03-01");
    }

    #[test]
    fn formats_dates_for_display() {
        assert_eq!(format_date(Some("2026-11-01")), "Nov 1, 2026");
        assert_eq!(format_date(None), "—");
        assert_eq!(format_date(Some("garbage")), "—");
    }

    #[test]
    fn timestamps_match_to_iso_string() {
        let ts = now_iso();
        assert_eq!(ts.len(), "2026-11-01T18:54:00.000Z".len());
        assert!(ts.ends_with('Z'));
    }
}
