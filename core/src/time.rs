//! Time-tracking rollups from the web app's `domain/timeTracking.ts`. Weeks run Monday–Sunday.

use chrono::{Datelike, NaiveDate};

use crate::date::{add_days, add_months, parse_iso, to_iso};
use crate::domain::text_cmp;
use crate::model::{Category, TimeEntry};

/// Time is logged in steps of this many minutes.
pub const INCREMENT_MINUTES: i32 = 15;
pub const NO_CATEGORY: &str = "No category";

pub fn total_minutes(entry: &TimeEntry) -> u32 {
    entry.daily_minutes.values().sum()
}

pub fn day_minutes(entry: &TimeEntry, day: &str) -> u32 {
    entry.daily_minutes.get(day).copied().unwrap_or(0)
}

/// Minutes logged on days within `[start, end]`, both inclusive.
pub fn range_minutes(entry: &TimeEntry, start: &str, end: &str) -> u32 {
    entry
        .daily_minutes
        .range(start.to_string()..=end.to_string())
        .map(|(_, m)| m)
        .sum()
}

/// The Monday–Sunday week containing `day`.
pub fn week_minutes(entry: &TimeEntry, day: NaiveDate) -> u32 {
    let monday = add_days(day, -i64::from(day.weekday().num_days_from_monday()));
    range_minutes(entry, &to_iso(monday), &to_iso(add_days(monday, 6)))
}

/// The calendar month containing `day`.
pub fn month_minutes(entry: &TimeEntry, day: NaiveDate) -> u32 {
    let first = day.with_day(1).expect("day 1 exists");
    let last = add_days(add_months(first, 1), -1);
    range_minutes(entry, &to_iso(first), &to_iso(last))
}

#[derive(Clone, PartialEq, Debug)]
pub struct DayLog {
    pub date: String,
    pub minutes: u32,
    pub note: Option<String>,
}

/// Every logged day with its minutes and note, newest first.
pub fn day_history(entry: &TimeEntry) -> Vec<DayLog> {
    entry
        .daily_minutes
        .iter()
        .rev()
        .filter(|(date, _)| parse_iso(date).is_some())
        .map(|(date, &minutes)| DayLog {
            date: date.clone(),
            minutes,
            note: entry.daily_notes.get(date).cloned(),
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ViewOptions {
    pub group_by_category: bool,
    pub sort_alphabetically: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub struct EntryGroup<'a> {
    /// Category name, or [`NO_CATEGORY`], when grouping; `None` for the single flat group.
    pub label: Option<String>,
    pub entries: Vec<&'a TimeEntry>,
}

/// Orders the time list for display. Grouping buckets items under their category name (groups
/// alphabetical, "No category" last); sorting applies within each group, or to the flat list.
/// With neither option the given (creation) order is kept.
pub fn organize_entries<'a>(
    entries: &'a [TimeEntry],
    categories: &[Category],
    options: ViewOptions,
) -> Vec<EntryGroup<'a>> {
    let sorted = |mut group: Vec<&'a TimeEntry>| {
        if options.sort_alphabetically {
            group.sort_by(|a, b| text_cmp(&a.title, &b.title));
        }
        group
    };
    if !options.group_by_category {
        return vec![EntryGroup {
            label: None,
            entries: sorted(entries.iter().collect()),
        }];
    }

    let mut groups: Vec<(String, Vec<&TimeEntry>)> = Vec::new();
    for entry in entries {
        let label = entry
            .category_id
            .as_ref()
            .and_then(|id| categories.iter().find(|c| &c.id == id))
            .map_or(NO_CATEGORY, |c| c.name.as_str());
        match groups.iter_mut().find(|(l, _)| l == label) {
            Some((_, group)) => group.push(entry),
            None => groups.push((label.to_string(), vec![entry])),
        }
    }
    groups.sort_by(|(a, _), (b, _)| {
        (a == NO_CATEGORY)
            .cmp(&(b == NO_CATEGORY))
            .then_with(|| text_cmp(a, b))
    });
    groups
        .into_iter()
        .map(|(label, group)| EntryGroup {
            label: Some(label),
            entries: sorted(group),
        })
        .collect()
}

/// "Xh Ym", "Xh" or "Ym" ("0m" when empty).
pub fn format_minutes(minutes: u32) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(minutes: &[(&str, u32)], notes: &[(&str, &str)]) -> TimeEntry {
        TimeEntry {
            id: "e1".into(),
            title: "Test".into(),
            description: String::new(),
            category_id: None,
            daily_minutes: minutes.iter().map(|(d, m)| (d.to_string(), *m)).collect(),
            daily_notes: notes
                .iter()
                .map(|(d, n)| (d.to_string(), n.to_string()))
                .collect(),
            created_at: "2026-01-01T00:00:00.000Z".into(),
            updated_at: "2026-01-01T00:00:00.000Z".into(),
        }
    }

    fn day(iso: &str) -> NaiveDate {
        parse_iso(iso).unwrap()
    }

    #[test]
    fn formats_minutes() {
        assert_eq!(format_minutes(0), "0m");
        assert_eq!(format_minutes(45), "45m");
        assert_eq!(format_minutes(60), "1h");
        assert_eq!(format_minutes(135), "2h 15m");
    }

    #[test]
    fn sums_totals_days_and_inclusive_ranges() {
        let e = entry(&[("2026-07-01", 30), ("2026-07-02", 45)], &[]);
        assert_eq!(total_minutes(&e), 75);
        assert_eq!(day_minutes(&e, "2026-07-02"), 45);
        assert_eq!(day_minutes(&e, "2026-07-03"), 0);

        let e = entry(
            &[("2026-07-01", 15), ("2026-07-05", 15), ("2026-07-10", 15)],
            &[],
        );
        assert_eq!(range_minutes(&e, "2026-07-01", "2026-07-05"), 30);
        assert_eq!(range_minutes(&e, "2026-07-02", "2026-07-04"), 0);
    }

    #[test]
    fn weeks_run_monday_to_sunday() {
        // 2026-07-29 is a Wednesday; its week is Jul 27 – Aug 2.
        let e = entry(
            &[
                ("2026-07-26", 15),
                ("2026-07-27", 30),
                ("2026-08-02", 45),
                ("2026-08-03", 60),
            ],
            &[],
        );
        assert_eq!(week_minutes(&e, day("2026-07-29")), 75);
        // A Sunday doesn't shift the week forward.
        assert_eq!(week_minutes(&e, day("2026-08-02")), 75);
        let e = entry(&[("2026-07-31", 15), ("2026-08-01", 15)], &[]);
        assert_eq!(week_minutes(&e, day("2026-07-29")), 30);
    }

    #[test]
    fn months_cover_only_their_calendar_month() {
        let e = entry(
            &[
                ("2026-06-30", 15),
                ("2026-07-01", 30),
                ("2026-07-31", 45),
                ("2026-08-01", 60),
            ],
            &[],
        );
        assert_eq!(month_minutes(&e, day("2026-07-15")), 75);
    }

    #[test]
    fn history_is_newest_first_with_notes() {
        let e = entry(
            &[("2026-07-01", 30), ("2026-07-03", 15)],
            &[("2026-07-01", "kickoff")],
        );
        assert_eq!(
            day_history(&e),
            [
                DayLog {
                    date: "2026-07-03".into(),
                    minutes: 15,
                    note: None
                },
                DayLog {
                    date: "2026-07-01".into(),
                    minutes: 30,
                    note: Some("kickoff".into())
                },
            ]
        );
    }

    #[test]
    fn organizes_by_group_and_sort_options() {
        let cat = |id: &str, name: &str| Category {
            id: id.into(),
            name: name.into(),
            created_at: String::new(),
        };
        let item = |title: &str, category: Option<&str>| TimeEntry {
            id: title.into(),
            title: title.into(),
            category_id: category.map(Into::into),
            ..entry(&[], &[])
        };
        let categories = [cat("w", "Work"), cat("p", "Personal")];
        let items = [
            item("Zebra", Some("w")),
            item("Errands", None),
            item("Alpha", Some("w")),
            item("Books", Some("p")),
        ];
        let titles = |g: &EntryGroup| {
            g.entries
                .iter()
                .map(|e| e.title.clone())
                .collect::<Vec<_>>()
        };
        let opts = |group, sort| ViewOptions {
            group_by_category: group,
            sort_alphabetically: sort,
        };

        let flat = organize_entries(&items, &categories, opts(false, false));
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].label, None);
        assert_eq!(titles(&flat[0]), ["Zebra", "Errands", "Alpha", "Books"]);

        let sorted = organize_entries(&items, &categories, opts(false, true));
        assert_eq!(titles(&sorted[0]), ["Alpha", "Books", "Errands", "Zebra"]);

        let grouped = organize_entries(&items, &categories, opts(true, false));
        let labels: Vec<_> = grouped.iter().map(|g| g.label.clone().unwrap()).collect();
        assert_eq!(labels, ["Personal", "Work", "No category"]);
        assert_eq!(titles(&grouped[1]), ["Zebra", "Alpha"]);

        let both = organize_entries(&items, &categories, opts(true, true));
        assert_eq!(titles(&both[1]), ["Alpha", "Zebra"]);

        let orphan = [item("Orphan", Some("gone"))];
        let groups = organize_entries(&orphan, &categories, opts(true, false));
        assert_eq!(groups[0].label.as_deref(), Some(NO_CATEGORY));
    }
}
