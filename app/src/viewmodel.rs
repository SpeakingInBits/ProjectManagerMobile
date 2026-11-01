//! Builds the UI's view models from app data: project cards, task rows, the time-tracking list
//! and its summaries, and the category lists.

use chrono::NaiveDate;
use project_core::date::{format_date, format_naive, is_overdue, to_iso};
use project_core::domain::{project_progress, sort_tasks};
use project_core::model::{Project, Task, TimeEntry};
use project_core::store::Data;
use project_core::time::{
    NO_CATEGORY, ViewOptions, day_history, day_minutes, format_minutes, month_minutes,
    organize_entries, total_minutes, week_minutes,
};
use slint::{ModelRc, SharedString, VecModel};

use crate::{HistoryRow, NamedRow, ProjectCardData, SummaryData, TaskRowData, TimeItemData};

pub fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

pub fn strings(items: impl IntoIterator<Item = String>) -> ModelRc<SharedString> {
    model(items.into_iter().map(SharedString::from).collect())
}

fn due_label(due: Option<&str>) -> String {
    due.map_or_else(String::new, |d| format!("Due {}", format_date(Some(d))))
}

/// "Work / Client A", "Work", or empty.
pub fn category_label(data: &Data, category: Option<&str>, subcategory: Option<&str>) -> String {
    match (data.category(category), data.subcategory(subcategory)) {
        (Some(c), Some(s)) => format!("{} / {}", c.name, s.name),
        (Some(c), None) => c.name.clone(),
        _ => String::new(),
    }
}

pub fn project_card(data: &Data, project: &Project, index: usize, count: usize) -> ProjectCardData {
    let progress = project_progress(data.tasks_of_project(&project.id));
    ProjectCardData {
        id: project.id.as_str().into(),
        title: project.title.as_str().into(),
        description: project.description.as_str().into(),
        due_label: due_label(project.due_date.as_deref()).into(),
        overdue: progress.percent < 100 && is_overdue(project.due_date.as_deref()),
        category_label: category_label(
            data,
            project.category_id.as_deref(),
            project.subcategory_id.as_deref(),
        )
        .into(),
        fill: if progress.total == 0 {
            0.0
        } else {
            progress.done as f32 / progress.total as f32
        },
        progress_label: format!(
            "{} / {} tasks complete ({}%)",
            progress.done, progress.total, progress.percent
        )
        .into(),
        can_up: index > 0,
        can_down: index + 1 < count,
    }
}

pub fn project_cards(data: &Data) -> Vec<ProjectCardData> {
    let projects = data.projects_sorted();
    let count = projects.len();
    projects
        .iter()
        .enumerate()
        .map(|(i, p)| project_card(data, p, i, count))
        .collect()
}

/// Sorted task rows, hiding completed tasks unless asked. `with_projects` labels each task with
/// its project's title (for lists that mix projects).
pub fn task_rows<'a>(
    data: &Data,
    tasks: impl IntoIterator<Item = &'a Task>,
    show_completed: bool,
    with_projects: bool,
) -> Vec<TaskRowData> {
    sort_tasks(tasks.into_iter().filter(|t| show_completed || !t.completed))
        .into_iter()
        .map(|t| TaskRowData {
            id: t.id.as_str().into(),
            title: t.title.as_str().into(),
            completed: t.completed,
            pinned: t.pinned,
            project_label: if with_projects {
                t.project_id
                    .as_deref()
                    .and_then(|id| data.project(id))
                    .map_or_else(SharedString::new, |p| p.title.as_str().into())
            } else {
                SharedString::new()
            },
            due_label: due_label(t.due_date.as_deref()).into(),
            overdue: !t.completed && is_overdue(t.due_date.as_deref()),
            repeats: t.repeat.repeats(),
        })
        .collect()
}

/// "Today", or the entry date when backfilling.
pub fn day_label(entry_date: NaiveDate, today: NaiveDate) -> String {
    if entry_date == today {
        "Today".into()
    } else {
        format_naive(entry_date)
    }
}

/// Day (on the entry date), week and month totals for a group of items. Week and month follow
/// the real today, as on the web.
pub fn summary<'a>(
    name: &str,
    entries: impl IntoIterator<Item = &'a TimeEntry> + Clone,
    entry_date: NaiveDate,
    today: NaiveDate,
) -> SummaryData {
    let day = to_iso(entry_date);
    let sum = |f: &dyn Fn(&TimeEntry) -> u32| -> SharedString {
        format_minutes(entries.clone().into_iter().map(f).sum()).into()
    };
    SummaryData {
        name: name.into(),
        day: sum(&|e| day_minutes(e, &day)),
        week: sum(&|e| week_minutes(e, today)),
        month: sum(&|e| month_minutes(e, today)),
    }
}

/// One card per category with items, plus "No category"; none at all until some item has a
/// category, since a lone "No category" card would repeat the overall summary.
pub fn category_summaries(
    data: &Data,
    entries: &[&TimeEntry],
    entry_date: NaiveDate,
    today: NaiveDate,
) -> Vec<SummaryData> {
    if entries.iter().all(|e| e.category_id.is_none()) {
        return Vec::new();
    }
    let mut cards: Vec<SummaryData> = data
        .categories
        .iter()
        .filter_map(|c| {
            let group: Vec<&TimeEntry> = entries
                .iter()
                .copied()
                .filter(|e| e.category_id.as_deref() == Some(c.id.as_str()))
                .collect();
            (!group.is_empty()).then(|| summary(&c.name, group.iter().copied(), entry_date, today))
        })
        .collect();
    let uncategorized: Vec<&TimeEntry> = entries
        .iter()
        .copied()
        .filter(|e| e.category_id.is_none())
        .collect();
    if !uncategorized.is_empty() {
        cards.push(summary(
            NO_CATEGORY,
            uncategorized.iter().copied(),
            entry_date,
            today,
        ));
    }
    cards
}

pub fn time_items(
    data: &Data,
    entries: &[TimeEntry],
    options: ViewOptions,
    entry_date: NaiveDate,
    today: NaiveDate,
) -> Vec<TimeItemData> {
    let day = to_iso(entry_date);
    let mut rows = Vec::new();
    for group in organize_entries(entries, &data.categories, options) {
        for (i, e) in group.entries.iter().enumerate() {
            let minutes = day_minutes(e, &day);
            rows.push(TimeItemData {
                id: e.id.as_str().into(),
                group_label: match (&group.label, i) {
                    (Some(label), 0) => label.as_str().into(),
                    _ => SharedString::new(),
                },
                title: e.title.as_str().into(),
                category: data
                    .category(e.category_id.as_deref())
                    .map_or_else(SharedString::new, |c| c.name.as_str().into()),
                description: e.description.as_str().into(),
                worked: minutes > 0,
                day_value: format_minutes(minutes).into(),
                note: e
                    .daily_notes
                    .get(&day)
                    .map_or_else(SharedString::new, |n| n.as_str().into()),
                total: format_minutes(total_minutes(e)).into(),
                week: format_minutes(week_minutes(e, today)).into(),
                month: format_minutes(month_minutes(e, today)).into(),
                can_subtract: minutes > 0,
            });
        }
    }
    rows
}

pub fn history_rows(entry: &TimeEntry) -> Vec<HistoryRow> {
    day_history(entry)
        .into_iter()
        .map(|day| HistoryRow {
            label: format_date(Some(&day.date)).into(),
            date: day.date.into(),
            minutes: format_minutes(day.minutes).into(),
            note: day.note.unwrap_or_default().into(),
        })
        .collect()
}

pub fn named_row(id: &str, name: &str, active: bool) -> NamedRow {
    NamedRow {
        id: id.into(),
        name: name.into(),
        active,
    }
}
