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

/// The same day `months` later, or the last day of that month when it's shorter
/// (Jan 31 + 1 month = Feb 28).
pub fn add_months(date: NaiveDate, months: i32) -> NaiveDate {
    add_months_on_day(date, months, date.day())
}

/// Day `day` of the month `months` after `date`'s, or that month's last day when it's shorter.
pub fn add_months_on_day(date: NaiveDate, months: i32, day: u32) -> NaiveDate {
    let total = date.year() * 12 + date.month0() as i32 + months;
    let first = NaiveDate::from_ymd_opt(total.div_euclid(12), total.rem_euclid(12) as u32 + 1, 1)
        .expect("valid month");
    first
        .with_day(day.clamp(1, last_day_of_month(first)))
        .expect("day within the month")
}

/// The same date `years` later; Feb 29 becomes Feb 28 in a non-leap year.
pub fn add_years(date: NaiveDate, years: i32) -> NaiveDate {
    add_months(date, years * 12)
}

/// The number of the last day in `date`'s month (28 to 31).
pub fn last_day_of_month(date: NaiveDate) -> u32 {
    let first = date.with_day(1).expect("day 1 exists");
    let next = first
        .checked_add_months(chrono::Months::new(1))
        .expect("in range");
    (next - first).num_days() as u32
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
    fn month_arithmetic_clamps_to_the_end_of_shorter_months() {
        assert_eq!(to_iso(add_months(d("2026-07-27"), 1)), "2026-08-27");
        assert_eq!(to_iso(add_months(d("2026-12-15"), 1)), "2027-01-15");
        assert_eq!(to_iso(add_months(d("2026-01-31"), 1)), "2026-02-28");
        assert_eq!(to_iso(add_months(d("2028-01-31"), 1)), "2028-02-29");
        assert_eq!(to_iso(add_months(d("2026-03-10"), -3)), "2025-12-10");
        assert_eq!(to_iso(add_years(d("2028-02-29"), 1)), "2029-02-28");
        assert_eq!(
            to_iso(add_months_on_day(d("2026-02-28"), 1, 31)),
            "2026-03-31"
        );
        assert_eq!(
            to_iso(add_months_on_day(d("2026-03-31"), 1, 31)),
            "2026-04-30"
        );
        assert_eq!(last_day_of_month(d("2026-02-10")), 28);
        assert_eq!(last_day_of_month(d("2026-12-31")), 31);
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
