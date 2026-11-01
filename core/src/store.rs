//! Local-only persistence (a single JSON document, written atomically) and the mutating actions
//! the UI performs, mirroring the web app's `db/*.repo.ts` and `domain/repeat.ts`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::date::{last_day_of_month, now_iso, parse_iso, to_iso, today};
use crate::domain::next_due_date;
use crate::model::{
    Category, Id, Project, ProjectInput, Settings, Subcategory, Task, TaskInput, ThemeMode,
    TimeEntry, TimeEntryInput,
};

/// Bumped whenever the stored shape changes; [`Data::migrate`] upgrades older documents.
pub const DATA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Data {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub categories: Vec<Category>,
    #[serde(default)]
    pub subcategories: Vec<Subcategory>,
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub time_entries: Vec<TimeEntry>,
    #[serde(default)]
    pub settings: Settings,
}

impl Data {
    fn migrate(mut self) -> Self {
        // Future migrations: if self.version < 2 { ... }; never edit earlier steps.
        self.version = DATA_VERSION;
        self
    }

    pub fn category(&self, id: Option<&str>) -> Option<&Category> {
        id.and_then(|id| self.categories.iter().find(|c| c.id == id))
    }

    pub fn subcategory(&self, id: Option<&str>) -> Option<&Subcategory> {
        id.and_then(|id| self.subcategories.iter().find(|s| s.id == id))
    }

    pub fn subcategories_of<'a>(
        &'a self,
        category_id: &'a str,
    ) -> impl Iterator<Item = &'a Subcategory> {
        self.subcategories
            .iter()
            .filter(move |s| s.category_id == category_id)
    }

    /// Projects in their manual order (ascending `order`, creation time as a stable tiebreak).
    pub fn projects_sorted(&self) -> Vec<&Project> {
        let mut v: Vec<&Project> = self.projects.iter().collect();
        v.sort_by(|a, b| {
            a.order
                .cmp(&b.order)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });
        v
    }

    pub fn project(&self, id: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn tasks_of_project<'a>(&'a self, project_id: &'a str) -> impl Iterator<Item = &'a Task> {
        self.tasks
            .iter()
            .filter(move |t| t.project_id.as_deref() == Some(project_id))
    }

    pub fn time_entry(&self, id: &str) -> Option<&TimeEntry> {
        self.time_entries.iter().find(|e| e.id == id)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Up,
    Down,
}

fn uuid() -> Id {
    uuid::Uuid::new_v4().to_string()
}

fn not_found(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, format!("{what} not found"))
}

/// In-memory app data plus the file it is saved to (none in tests).
pub struct Store {
    path: Option<PathBuf>,
    pub data: Data,
}

impl Store {
    /// Opens the data file at `path`, starting empty if it doesn't exist yet. A corrupt file is
    /// moved aside (never silently overwritten) and the app starts fresh.
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let data = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Data>(&bytes) {
                Ok(data) => data.migrate(),
                Err(_) => {
                    let _ = fs::rename(&path, path.with_extension("corrupt.json"));
                    Data::default().migrate()
                }
            },
            Err(_) => Data::default().migrate(),
        };
        Self {
            path: Some(path),
            data,
        }
    }

    pub fn in_memory(data: Data) -> Self {
        Self {
            path: None,
            data: data.migrate(),
        }
    }

    fn save(&self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        write_atomic(path, &serde_json::to_vec(&self.data)?)
    }

    /// Replaces all data (used by the desktop `--seed` option).
    pub fn replace(&mut self, data: Data) -> io::Result<()> {
        self.data = data.migrate();
        self.save()
    }

    // --- Categories -----------------------------------------------------------------------

    pub fn add_category(&mut self, name: &str) -> io::Result<Id> {
        let id = uuid();
        self.data.categories.push(Category {
            id: id.clone(),
            name: name.to_string(),
            created_at: now_iso(),
        });
        self.save()?;
        Ok(id)
    }

    pub fn rename_category(&mut self, id: &str, name: &str) -> io::Result<()> {
        let Some(c) = self.data.categories.iter_mut().find(|c| c.id == id) else {
            return Err(not_found("Category"));
        };
        c.name = name.to_string();
        self.save()
    }

    /// The cascade point for the shared taxonomy: deletes the category's subcategories and
    /// detaches every project, task and time entry that used it.
    pub fn remove_category(&mut self, id: &str) -> io::Result<()> {
        let d = &mut self.data;
        d.subcategories.retain(|s| s.category_id != id);
        for p in d
            .projects
            .iter_mut()
            .filter(|p| p.category_id.as_deref() == Some(id))
        {
            p.category_id = None;
            p.subcategory_id = None;
        }
        for t in d
            .tasks
            .iter_mut()
            .filter(|t| t.category_id.as_deref() == Some(id))
        {
            t.category_id = None;
            t.subcategory_id = None;
        }
        for e in d
            .time_entries
            .iter_mut()
            .filter(|e| e.category_id.as_deref() == Some(id))
        {
            e.category_id = None;
        }
        d.categories.retain(|c| c.id != id);
        self.save()
    }

    // --- Subcategories --------------------------------------------------------------------

    pub fn add_subcategory(&mut self, category_id: &str, name: &str) -> io::Result<Id> {
        let id = uuid();
        self.data.subcategories.push(Subcategory {
            id: id.clone(),
            category_id: category_id.to_string(),
            name: name.to_string(),
            created_at: now_iso(),
        });
        self.save()?;
        Ok(id)
    }

    pub fn rename_subcategory(&mut self, id: &str, name: &str) -> io::Result<()> {
        let Some(s) = self.data.subcategories.iter_mut().find(|s| s.id == id) else {
            return Err(not_found("Subcategory"));
        };
        s.name = name.to_string();
        self.save()
    }

    /// Deletes a subcategory and detaches the projects and tasks that used it.
    pub fn remove_subcategory(&mut self, id: &str) -> io::Result<()> {
        let d = &mut self.data;
        for p in d
            .projects
            .iter_mut()
            .filter(|p| p.subcategory_id.as_deref() == Some(id))
        {
            p.subcategory_id = None;
        }
        for t in d
            .tasks
            .iter_mut()
            .filter(|t| t.subcategory_id.as_deref() == Some(id))
        {
            t.subcategory_id = None;
        }
        d.subcategories.retain(|s| s.id != id);
        self.save()
    }

    // --- Projects -------------------------------------------------------------------------

    pub fn add_project(&mut self, input: ProjectInput) -> io::Result<Id> {
        let now = now_iso();
        let id = uuid();
        let order = self
            .data
            .projects
            .iter()
            .map(|p| p.order)
            .max()
            .unwrap_or(-1)
            + 1;
        self.data.projects.push(Project {
            id: id.clone(),
            title: input.title,
            description: input.description,
            due_date: input.due_date,
            category_id: input.category_id,
            subcategory_id: input.subcategory_id,
            order,
            created_at: now.clone(),
            updated_at: now,
        });
        self.save()?;
        Ok(id)
    }

    pub fn update_project(&mut self, id: &str, input: ProjectInput) -> io::Result<()> {
        let Some(p) = self.data.projects.iter_mut().find(|p| p.id == id) else {
            return Err(not_found("Project"));
        };
        p.title = input.title;
        p.description = input.description;
        p.due_date = input.due_date;
        p.category_id = input.category_id;
        p.subcategory_id = input.subcategory_id;
        p.updated_at = now_iso();
        self.save()
    }

    /// Moves a project one slot up or down in the manual order. No-op at the ends of the list.
    pub fn move_project(&mut self, id: &str, direction: Direction) -> io::Result<()> {
        let mut ordered: Vec<Id> = self
            .data
            .projects_sorted()
            .iter()
            .map(|p| p.id.clone())
            .collect();
        let Some(index) = ordered.iter().position(|p| p == id) else {
            return Ok(());
        };
        let neighbor = match direction {
            Direction::Up if index > 0 => index - 1,
            Direction::Down if index + 1 < ordered.len() => index + 1,
            _ => return Ok(()),
        };
        ordered.swap(index, neighbor);
        // Renumbering (rather than swapping two `order` values) also repairs duplicate orders.
        let now = now_iso();
        for p in &mut self.data.projects {
            let order = ordered.iter().position(|o| o == &p.id).expect("listed") as i64;
            if p.order != order {
                p.order = order;
                p.updated_at = now.clone();
            }
        }
        self.save()
    }

    /// Deletes a project. Its tasks are deleted too, or kept as standalone tasks.
    pub fn remove_project(&mut self, id: &str, delete_tasks: bool) -> io::Result<()> {
        let d = &mut self.data;
        if delete_tasks {
            d.tasks.retain(|t| t.project_id.as_deref() != Some(id));
        } else {
            for t in d
                .tasks
                .iter_mut()
                .filter(|t| t.project_id.as_deref() == Some(id))
            {
                t.project_id = None;
            }
        }
        d.projects.retain(|p| p.id != id);
        self.save()
    }

    // --- Tasks ----------------------------------------------------------------------------

    pub fn add_task(&mut self, input: TaskInput) -> io::Result<Id> {
        let now = now_iso();
        let id = uuid();
        self.data.tasks.push(Task {
            id: id.clone(),
            title: input.title,
            description: input.description,
            due_date: input.due_date,
            project_id: input.project_id,
            category_id: input.category_id,
            subcategory_id: input.subcategory_id,
            pinned: false,
            completed: false,
            completed_at: None,
            repeat: input.repeat,
            series_id: id.clone(),
            previous_instance_id: None,
            created_at: now.clone(),
            updated_at: now,
        });
        self.save()?;
        Ok(id)
    }

    fn task_mut(&mut self, id: &str) -> io::Result<&mut Task> {
        self.data
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| not_found("Task"))
    }

    pub fn update_task(&mut self, id: &str, input: TaskInput) -> io::Result<()> {
        let t = self.task_mut(id)?;
        t.title = input.title;
        t.description = input.description;
        t.due_date = input.due_date;
        t.project_id = input.project_id;
        t.category_id = input.category_id;
        t.subcategory_id = input.subcategory_id;
        t.repeat = input.repeat;
        t.updated_at = now_iso();
        self.save()
    }

    pub fn remove_task(&mut self, id: &str) -> io::Result<()> {
        self.data.tasks.retain(|t| t.id != id);
        self.save()
    }

    pub fn set_pinned(&mut self, id: &str, pinned: bool) -> io::Result<()> {
        let t = self.task_mut(id)?;
        t.pinned = pinned;
        t.updated_at = now_iso();
        self.save()
    }

    /// Completes a task today, or reopens a completed one.
    pub fn toggle_task(&mut self, id: &str) -> io::Result<()> {
        let completed = self
            .data
            .task(id)
            .ok_or_else(|| not_found("Task"))?
            .completed;
        if completed {
            self.uncomplete_task(id)
        } else {
            self.complete_task(id, today()).map(|_| ())
        }
    }

    /// Marks a task complete on `completion`. A repeating task is never reset in place: the
    /// completed row stays as history and a new row is spawned for the next occurrence, which is
    /// what lets a project's progress bar reach and hold 100%. Returns the new row's id.
    pub fn complete_task(&mut self, id: &str, completion: NaiveDate) -> io::Result<Option<Id>> {
        let now = now_iso();
        let t = self.task_mut(id)?;
        t.completed = true;
        t.completed_at = Some(now.clone());
        t.pinned = false;
        t.updated_at = now.clone();
        let task = t.clone();
        if !task.repeat.repeats() {
            self.save()?;
            return Ok(None);
        }

        let previous_due = task.due_date.as_deref().and_then(parse_iso);
        let anchor_day = previous_due.map(|d| self.series_anchor_day(&task, d));
        let next_id = uuid();
        self.data.tasks.push(Task {
            id: next_id.clone(),
            due_date: next_due_date(task.repeat, completion, previous_due, anchor_day).map(to_iso),
            pinned: false,
            completed: false,
            completed_at: None,
            previous_instance_id: Some(task.id.clone()),
            created_at: now.clone(),
            updated_at: now,
            ..task
        });
        self.save()?;
        Ok(Some(next_id))
    }

    /// The day of month a monthly or yearly series falls on: the previous due date's day, unless
    /// that was a short month's last day standing in for a later day. Then the series' first due
    /// date says which day it really is, so Jan 31, Feb 28 is followed by Mar 31, not Mar 28.
    fn series_anchor_day(&self, task: &Task, previous_due: NaiveDate) -> u32 {
        let day = previous_due.day();
        if day < last_day_of_month(previous_due) {
            return day;
        }
        self.data
            .task(&task.series_id)
            .and_then(|first| first.due_date.as_deref())
            .and_then(parse_iso)
            .map(|first| first.day())
            .filter(|&first| first > day)
            .unwrap_or(day)
    }

    /// Plain field flip. Un-completing a repeating task's history row doesn't retract the next
    /// occurrence that was already spawned (same as the web app).
    pub fn uncomplete_task(&mut self, id: &str) -> io::Result<()> {
        let t = self.task_mut(id)?;
        t.completed = false;
        t.completed_at = None;
        t.updated_at = now_iso();
        self.save()
    }

    // --- Time entries ---------------------------------------------------------------------

    /// Time entries in creation order.
    pub fn time_entries(&self) -> Vec<&TimeEntry> {
        let mut v: Vec<&TimeEntry> = self.data.time_entries.iter().collect();
        v.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        v
    }

    pub fn add_time_entry(&mut self, input: TimeEntryInput) -> io::Result<Id> {
        let now = now_iso();
        let id = uuid();
        self.data.time_entries.push(TimeEntry {
            id: id.clone(),
            title: input.title,
            description: input.description,
            category_id: input.category_id,
            daily_minutes: Default::default(),
            daily_notes: Default::default(),
            created_at: now.clone(),
            updated_at: now,
        });
        self.save()?;
        Ok(id)
    }

    fn entry_mut(&mut self, id: &str) -> io::Result<&mut TimeEntry> {
        self.data
            .time_entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| not_found("Time entry"))
    }

    pub fn update_time_entry(&mut self, id: &str, input: TimeEntryInput) -> io::Result<()> {
        let e = self.entry_mut(id)?;
        e.title = input.title;
        e.description = input.description;
        e.category_id = input.category_id;
        e.updated_at = now_iso();
        self.save()
    }

    pub fn remove_time_entry(&mut self, id: &str) -> io::Result<()> {
        self.data.time_entries.retain(|e| e.id != id);
        self.save()
    }

    /// Adds `delta` minutes (may be negative) to a day, never going below zero. A day that
    /// reaches zero is removed, and its note with it.
    pub fn add_minutes_on_day(&mut self, id: &str, date: &str, delta: i32) -> io::Result<()> {
        let e = self.entry_mut(id)?;
        let current = e.daily_minutes.get(date).copied().unwrap_or(0) as i64;
        let next = (current + i64::from(delta)).max(0) as u32;
        if next == 0 {
            e.daily_minutes.remove(date);
            e.daily_notes.remove(date);
        } else {
            e.daily_minutes.insert(date.to_string(), next);
        }
        e.updated_at = now_iso();
        self.save()
    }

    /// Sets or clears a day's note. Notes only exist on days with logged time; a blank note
    /// removes it.
    pub fn set_day_note(&mut self, id: &str, date: &str, note: &str) -> io::Result<()> {
        let e = self.entry_mut(id)?;
        let trimmed = note.trim();
        if trimmed.is_empty() || e.daily_minutes.get(date).copied().unwrap_or(0) == 0 {
            e.daily_notes.remove(date);
        } else {
            e.daily_notes.insert(date.to_string(), trimmed.to_string());
        }
        e.updated_at = now_iso();
        self.save()
    }

    /// Merges `source` into `target`: the source's minutes are added day by day, a note on a
    /// day both logged is appended on a new line, and the source is deleted. The target keeps
    /// its own title, description and category. Nothing changes if either id is invalid.
    pub fn merge_time_entry(&mut self, source_id: &str, target_id: &str) -> io::Result<()> {
        if source_id == target_id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Can't merge a time entry into itself",
            ));
        }
        let source = self
            .data
            .time_entry(source_id)
            .cloned()
            .ok_or_else(|| not_found("Time entry"))?;
        let target = self.entry_mut(target_id)?;
        for (date, minutes) in &source.daily_minutes {
            *target.daily_minutes.entry(date.clone()).or_insert(0) += minutes;
            if let Some(note) = source.daily_notes.get(date).filter(|n| !n.is_empty()) {
                target
                    .daily_notes
                    .entry(date.clone())
                    .and_modify(|existing| {
                        existing.push('\n');
                        existing.push_str(note);
                    })
                    .or_insert_with(|| note.clone());
            }
        }
        target.updated_at = now_iso();
        self.data.time_entries.retain(|e| e.id != source_id);
        self.save()
    }

    // --- Settings -------------------------------------------------------------------------

    pub fn update_settings(&mut self, f: impl FnOnce(&mut Settings)) -> io::Result<()> {
        f(&mut self.data.settings);
        self.save()
    }

    pub fn set_theme_mode(&mut self, mode: ThemeMode) -> io::Result<()> {
        self.update_settings(|s| s.theme_mode = mode)
    }
}

/// Writes via a temp file and rename so a crash mid-write can't leave a truncated data file.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RepeatConfig;

    fn store() -> Store {
        Store::in_memory(Data::default())
    }

    fn entry(s: &Store, id: &str) -> TimeEntry {
        s.data.time_entry(id).unwrap().clone()
    }

    fn new_entry(s: &mut Store, title: &str, category_id: Option<&Id>) -> Id {
        s.add_time_entry(TimeEntryInput {
            title: title.into(),
            description: String::new(),
            category_id: category_id.cloned(),
        })
        .unwrap()
    }

    #[test]
    fn add_minutes_accumulates_and_clamps_at_zero() {
        let mut s = store();
        let e = new_entry(&mut s, "Work", None);
        s.add_minutes_on_day(&e, "2026-07-29", 15).unwrap();
        s.add_minutes_on_day(&e, "2026-07-29", 15).unwrap();
        assert_eq!(entry(&s, &e).daily_minutes["2026-07-29"], 30);
        s.add_minutes_on_day(&e, "2026-07-29", -999).unwrap();
        assert!(!entry(&s, &e).daily_minutes.contains_key("2026-07-29"));
    }

    #[test]
    fn a_day_reaching_zero_takes_its_note_with_it() {
        let mut s = store();
        let e = new_entry(&mut s, "Work", None);
        s.add_minutes_on_day(&e, "2026-07-29", 15).unwrap();
        s.set_day_note(&e, "2026-07-29", "a note").unwrap();
        s.add_minutes_on_day(&e, "2026-07-29", -15).unwrap();
        assert!(entry(&s, &e).daily_minutes.is_empty());
        assert!(entry(&s, &e).daily_notes.is_empty());
    }

    #[test]
    fn day_notes_trim_clear_and_need_logged_time() {
        let mut s = store();
        let e = new_entry(&mut s, "Work", None);
        s.add_minutes_on_day(&e, "2026-07-29", 15).unwrap();
        s.set_day_note(&e, "2026-07-29", "  hello  ").unwrap();
        assert_eq!(entry(&s, &e).daily_notes["2026-07-29"], "hello");
        s.set_day_note(&e, "2026-07-29", "   ").unwrap();
        assert!(!entry(&s, &e).daily_notes.contains_key("2026-07-29"));
        s.set_day_note(&e, "2026-07-30", "orphan").unwrap();
        assert!(!entry(&s, &e).daily_notes.contains_key("2026-07-30"));
    }

    #[test]
    fn merge_sums_days_joins_notes_and_deletes_the_source() {
        let mut s = store();
        let cat = s.add_category("Work").unwrap();
        let target = new_entry(&mut s, "Target", Some(&cat));
        let source = new_entry(&mut s, "Source", None);
        s.add_minutes_on_day(&target, "2026-07-28", 30).unwrap();
        s.set_day_note(&target, "2026-07-28", "target note")
            .unwrap();
        s.add_minutes_on_day(&target, "2026-07-29", 15).unwrap();
        s.add_minutes_on_day(&source, "2026-07-28", 45).unwrap();
        s.set_day_note(&source, "2026-07-28", "source note")
            .unwrap();
        s.add_minutes_on_day(&source, "2026-07-29", 15).unwrap();
        s.set_day_note(&source, "2026-07-29", "only source")
            .unwrap();
        s.add_minutes_on_day(&source, "2026-07-30", 60).unwrap();

        s.merge_time_entry(&source, &target).unwrap();

        let merged = entry(&s, &target);
        let minutes: Vec<_> = merged
            .daily_minutes
            .iter()
            .map(|(d, m)| (d.as_str(), *m))
            .collect();
        assert_eq!(
            minutes,
            [("2026-07-28", 75), ("2026-07-29", 30), ("2026-07-30", 60)]
        );
        assert_eq!(merged.daily_notes["2026-07-28"], "target note\nsource note");
        assert_eq!(merged.daily_notes["2026-07-29"], "only source");
        assert_eq!(merged.title, "Target");
        assert_eq!(merged.category_id.as_ref(), Some(&cat));
        assert!(s.data.time_entry(&source).is_none());
    }

    #[test]
    fn merge_refuses_itself_or_a_missing_item() {
        let mut s = store();
        let e = new_entry(&mut s, "Solo", None);
        s.add_minutes_on_day(&e, "2026-07-29", 15).unwrap();
        assert!(s.merge_time_entry(&e, &e).is_err());
        assert!(s.merge_time_entry(&e, "missing").is_err());
        assert!(s.merge_time_entry("missing", &e).is_err());
        assert_eq!(entry(&s, &e).daily_minutes["2026-07-29"], 15);
    }

    #[test]
    fn removing_a_category_cascades() {
        let mut s = store();
        let cat = s.add_category("Work").unwrap();
        let other = s.add_category("Personal").unwrap();
        let sub = s.add_subcategory(&cat, "Internal").unwrap();
        let project = s
            .add_project(ProjectInput {
                title: "P".into(),
                category_id: Some(cat.clone()),
                subcategory_id: Some(sub.clone()),
                ..Default::default()
            })
            .unwrap();
        let task = s
            .add_task(TaskInput {
                title: "T".into(),
                category_id: Some(cat.clone()),
                subcategory_id: Some(sub.clone()),
                ..Default::default()
            })
            .unwrap();
        let e = new_entry(&mut s, "E", Some(&cat));
        let untouched = new_entry(&mut s, "U", Some(&other));

        s.remove_category(&cat).unwrap();

        assert!(s.data.category(Some(&cat)).is_none());
        assert!(s.data.subcategory(Some(&sub)).is_none());
        let p = s.data.project(&project).unwrap();
        assert_eq!(
            (p.category_id.as_ref(), p.subcategory_id.as_ref()),
            (None, None)
        );
        assert_eq!(s.data.task(&task).unwrap().category_id, None);
        assert_eq!(entry(&s, &e).category_id, None);
        assert_eq!(entry(&s, &untouched).category_id, Some(other));
    }

    #[test]
    fn removing_a_subcategory_detaches_projects_and_tasks() {
        let mut s = store();
        let cat = s.add_category("Work").unwrap();
        let sub = s.add_subcategory(&cat, "Internal").unwrap();
        let task = s
            .add_task(TaskInput {
                title: "T".into(),
                category_id: Some(cat.clone()),
                subcategory_id: Some(sub.clone()),
                ..Default::default()
            })
            .unwrap();
        s.remove_subcategory(&sub).unwrap();
        let t = s.data.task(&task).unwrap();
        assert_eq!(t.category_id, Some(cat));
        assert_eq!(t.subcategory_id, None);
    }

    #[test]
    fn completing_a_repeating_task_spawns_the_next_occurrence() {
        let mut s = store();
        let t = s
            .add_task(TaskInput {
                title: "Weekly review".into(),
                due_date: Some("2026-07-27".into()),
                repeat: RepeatConfig::Weekly,
                ..Default::default()
            })
            .unwrap();
        s.set_pinned(&t, true).unwrap();
        let next = s
            .complete_task(&t, parse_iso("2026-07-29").unwrap())
            .unwrap()
            .unwrap();

        assert_eq!(s.data.tasks.len(), 2);
        let done = s.data.task(&t).unwrap();
        assert!(done.completed && done.completed_at.is_some() && !done.pinned);
        let next = s.data.task(&next).unwrap();
        assert!(!next.completed);
        assert_eq!(next.due_date.as_deref(), Some("2026-08-03"));
        assert_eq!(next.series_id, t);
        assert_eq!(next.previous_instance_id.as_ref(), Some(&t));
    }

    /// Completes the open occurrence of a series again and again, returning each new due date.
    fn complete_repeatedly(s: &mut Store, first: &str, times: usize) -> Vec<String> {
        let mut current = first.to_string();
        (0..times)
            .map(|_| {
                current = s.complete_task(&current, today()).unwrap().unwrap();
                s.data.task(&current).unwrap().due_date.clone().unwrap()
            })
            .collect()
    }

    fn repeating(s: &mut Store, due: &str, repeat: RepeatConfig) -> Id {
        s.add_task(TaskInput {
            title: "Repeats".into(),
            due_date: Some(due.into()),
            repeat,
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn monthly_series_keep_their_day_through_short_months() {
        let mut s = store();
        let t = repeating(&mut s, "2026-01-31", RepeatConfig::Monthly);
        assert_eq!(
            complete_repeatedly(&mut s, &t, 4),
            ["2026-02-28", "2026-03-31", "2026-04-30", "2026-05-31"]
        );

        let t = repeating(&mut s, "2026-01-30", RepeatConfig::Monthly);
        assert_eq!(
            complete_repeatedly(&mut s, &t, 2),
            ["2026-02-28", "2026-03-30"]
        );

        let t = repeating(&mut s, "2028-02-29", RepeatConfig::Yearly);
        assert_eq!(
            complete_repeatedly(&mut s, &t, 4),
            ["2029-02-28", "2030-02-28", "2031-02-28", "2032-02-29"]
        );
    }

    #[test]
    fn an_edited_due_date_becomes_the_series_day() {
        let mut s = store();
        let t = repeating(&mut s, "2026-01-31", RepeatConfig::Monthly);
        let next = s.complete_task(&t, today()).unwrap().unwrap();
        // Moving an occurrence to mid-month changes the day from then on.
        s.data
            .tasks
            .iter_mut()
            .find(|x| x.id == next)
            .unwrap()
            .due_date = Some("2026-02-15".into());
        assert_eq!(
            complete_repeatedly(&mut s, &next, 2),
            ["2026-03-15", "2026-04-15"]
        );
    }

    #[test]
    fn completing_a_one_off_task_spawns_nothing() {
        let mut s = store();
        let t = s
            .add_task(TaskInput {
                title: "One-off".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(s.complete_task(&t, today()).unwrap(), None);
        assert_eq!(s.data.tasks.len(), 1);
        s.toggle_task(&t).unwrap();
        assert!(!s.data.task(&t).unwrap().completed);
    }

    #[test]
    fn projects_keep_a_manual_order() {
        let mut s = store();
        let ids: Vec<Id> = ["A", "B", "C"]
            .iter()
            .map(|t| {
                s.add_project(ProjectInput {
                    title: t.to_string(),
                    ..Default::default()
                })
                .unwrap()
            })
            .collect();
        let titles = |s: &Store| -> Vec<String> {
            s.data
                .projects_sorted()
                .iter()
                .map(|p| p.title.clone())
                .collect()
        };
        s.move_project(&ids[2], Direction::Up).unwrap();
        assert_eq!(titles(&s), ["A", "C", "B"]);
        s.move_project(&ids[0], Direction::Up).unwrap();
        assert_eq!(titles(&s), ["A", "C", "B"]);
        s.move_project(&ids[0], Direction::Down).unwrap();
        assert_eq!(titles(&s), ["C", "A", "B"]);
    }

    #[test]
    fn removing_a_project_deletes_or_keeps_its_tasks() {
        let mut s = store();
        let p = s.add_project(ProjectInput::default()).unwrap();
        let in_project = || TaskInput {
            title: "T".into(),
            project_id: Some(p.clone()),
            ..Default::default()
        };
        let kept = s.add_task(in_project()).unwrap();
        s.remove_project(&p, false).unwrap();
        assert_eq!(s.data.task(&kept).unwrap().project_id, None);

        let p2 = s.add_project(ProjectInput::default()).unwrap();
        s.add_task(TaskInput {
            project_id: Some(p2.clone()),
            ..in_project()
        })
        .unwrap();
        s.remove_project(&p2, true).unwrap();
        assert_eq!(s.data.tasks.len(), 1);
    }

    #[test]
    fn data_round_trips_through_the_file_with_web_field_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.json");
        let mut s = Store::open(&path);
        let t = s
            .add_task(TaskInput {
                title: "Water plants".into(),
                repeat: RepeatConfig::Movable { interval_days: 3 },
                ..Default::default()
            })
            .unwrap();
        let e = new_entry(&mut s, "Work", None);
        s.add_minutes_on_day(&e, "2026-07-29", 45).unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains(r#""repeat":{"kind":"movable","intervalDays":3}"#));
        assert!(raw.contains(r#""dailyMinutes":{"2026-07-29":45}"#));
        assert!(raw.contains(r#""previousInstanceId":null"#));

        let reopened = Store::open(&path);
        assert_eq!(reopened.data, s.data);
        assert_eq!(reopened.data.task(&t).unwrap().title, "Water plants");
    }

    #[test]
    fn a_corrupt_file_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.json");
        fs::write(&path, b"{not json").unwrap();
        let s = Store::open(&path);
        assert!(s.data.tasks.is_empty());
        assert!(dir.path().join("data.corrupt.json").exists());
    }
}
