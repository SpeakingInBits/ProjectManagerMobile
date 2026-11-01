//! Pure domain logic from the web app's `domain/` folder: repeat scheduling, project progress
//! and task ordering. Time tracking lives in [`crate::time`].

use std::cmp::Ordering;

use chrono::NaiveDate;

use crate::date::{add_days, add_months, add_years};
use crate::model::{RepeatConfig, Task};

/// Next due date of a repeating task. `daily`/`weekly`/`monthly`/`yearly`/`custom` are
/// schedule-anchored: the next date is computed from the previous due date, so a weekly Monday
/// task completed on a Wednesday is still next due the following Monday (no drift). `movable` is
/// completion-anchored: it always counts from when the task was actually completed.
pub fn next_due_date(
    repeat: RepeatConfig,
    completion: NaiveDate,
    previous_due: Option<NaiveDate>,
) -> Option<NaiveDate> {
    let base = previous_due.unwrap_or(completion);
    match repeat {
        RepeatConfig::Never => None,
        RepeatConfig::Daily => Some(add_days(base, 1)),
        RepeatConfig::Weekly => Some(add_days(base, 7)),
        RepeatConfig::Monthly => Some(add_months(base, 1)),
        RepeatConfig::Yearly => Some(add_years(base, 1)),
        RepeatConfig::Custom { interval_days } => Some(add_days(base, i64::from(interval_days))),
        RepeatConfig::Movable { interval_days } => {
            Some(add_days(completion, i64::from(interval_days)))
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub percent: u32,
}

pub fn project_progress<'a>(tasks: impl IntoIterator<Item = &'a Task>) -> Progress {
    let (done, total) = tasks.into_iter().fold((0, 0), |(d, t), task| {
        (d + usize::from(task.completed), t + 1)
    });
    let percent = if total == 0 {
        0
    } else {
        (done as f64 / total as f64 * 100.0).round() as u32
    };
    Progress {
        done,
        total,
        percent,
    }
}

/// Case-insensitive text order with a case-sensitive tiebreak, approximating `localeCompare`.
pub fn text_cmp(a: &str, b: &str) -> Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

/// Ordering for task lists: incomplete before completed; among incomplete, pinned first; then by
/// closest due date (no due date last); finally by title so the order is stable.
pub fn compare_tasks(a: &Task, b: &Task) -> Ordering {
    if a.completed != b.completed {
        return a.completed.cmp(&b.completed);
    }
    if !a.completed && a.pinned != b.pinned {
        return b.pinned.cmp(&a.pinned);
    }
    let due = match (&a.due_date, &b.due_date) {
        (Some(x), Some(y)) => x.cmp(y),
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (None, None) => Ordering::Equal,
    };
    due.then_with(|| text_cmp(&a.title, &b.title))
}

pub fn sort_tasks<'a>(tasks: impl IntoIterator<Item = &'a Task>) -> Vec<&'a Task> {
    let mut v: Vec<&Task> = tasks.into_iter().collect();
    v.sort_by(|a, b| compare_tasks(a, b));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::{parse_iso, to_iso};

    fn iso(d: Option<NaiveDate>) -> Option<String> {
        d.map(to_iso)
    }

    #[test]
    fn next_due_date_follows_each_repeat_kind() {
        let completed_wed = parse_iso("2026-07-29");
        let due_mon = parse_iso("2026-07-27");
        let next = |r| iso(next_due_date(r, completed_wed.unwrap(), due_mon));

        assert_eq!(next(RepeatConfig::Never), None);
        // Schedule-anchored kinds advance from the previous due date, not the completion date.
        assert_eq!(next(RepeatConfig::Weekly).as_deref(), Some("2026-08-03"));
        assert_eq!(next(RepeatConfig::Daily).as_deref(), Some("2026-07-28"));
        assert_eq!(next(RepeatConfig::Monthly).as_deref(), Some("2026-08-27"));
        assert_eq!(next(RepeatConfig::Yearly).as_deref(), Some("2027-07-27"));
        assert_eq!(
            next(RepeatConfig::Custom { interval_days: 10 }).as_deref(),
            Some("2026-08-06")
        );
        // Movable is completion-anchored.
        assert_eq!(
            next(RepeatConfig::Movable { interval_days: 30 }).as_deref(),
            Some("2026-08-28")
        );
        // With no previous due date, the completion date is the base.
        assert_eq!(
            iso(next_due_date(
                RepeatConfig::Weekly,
                completed_wed.unwrap(),
                None
            ))
            .as_deref(),
            Some("2026-08-05")
        );
    }

    fn task(title: &str) -> Task {
        Task {
            id: title.into(),
            title: title.into(),
            description: String::new(),
            due_date: None,
            project_id: None,
            category_id: None,
            subcategory_id: None,
            pinned: false,
            completed: false,
            completed_at: None,
            repeat: RepeatConfig::Never,
            series_id: title.into(),
            previous_instance_id: None,
            created_at: "2026-01-01T00:00:00.000Z".into(),
            updated_at: "2026-01-01T00:00:00.000Z".into(),
        }
    }

    #[test]
    fn sorts_pinned_then_due_then_title_with_completed_last() {
        let tasks = vec![
            Task {
                completed: true,
                due_date: Some("2026-01-01".into()),
                ..task("done early")
            },
            task("no due B"),
            task("no due A"),
            Task {
                due_date: Some("2026-08-10".into()),
                ..task("due later")
            },
            Task {
                due_date: Some("2026-08-01".into()),
                ..task("due soon")
            },
            Task {
                pinned: true,
                due_date: Some("2026-12-01".into()),
                ..task("pinned late due")
            },
        ];
        let titles: Vec<&str> = sort_tasks(&tasks)
            .iter()
            .map(|t| t.title.as_str())
            .collect();
        assert_eq!(
            titles,
            [
                "pinned late due",
                "due soon",
                "due later",
                "no due A",
                "no due B",
                "done early"
            ]
        );
    }

    #[test]
    fn progress_rounds_and_handles_empty_projects() {
        let tasks = vec![
            Task {
                completed: true,
                ..task("a")
            },
            Task {
                completed: true,
                ..task("b")
            },
            task("c"),
        ];
        assert_eq!(
            project_progress(&tasks),
            Progress {
                done: 2,
                total: 3,
                percent: 67
            }
        );
        assert_eq!(project_progress(&[]), Progress::default());
    }
}
