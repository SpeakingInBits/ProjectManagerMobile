//! Stored records. Field names and shapes match the web app's `models/types.ts`, so records
//! serialize the same way as its IndexedDB rows.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub type Id = String;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: Id,
    pub name: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Subcategory {
    pub id: Id,
    pub category_id: Id,
    pub name: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: Id,
    pub title: String,
    pub description: String,
    /// ISO date (YYYY-MM-DD).
    pub due_date: Option<String>,
    pub category_id: Option<Id>,
    pub subcategory_id: Option<Id>,
    /// Manual sort position on the projects list; lower comes first.
    pub order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RepeatConfig {
    #[default]
    Never,
    Daily,
    Weekly,
    Monthly,
    Yearly,
    /// Every N days, anchored to the previous due date.
    Custom {
        #[serde(rename = "intervalDays")]
        interval_days: u32,
    },
    /// N days after the task is actually completed.
    Movable {
        #[serde(rename = "intervalDays")]
        interval_days: u32,
    },
}

impl RepeatConfig {
    pub fn repeats(self) -> bool {
        self != Self::Never
    }

    pub fn interval_days(self) -> Option<u32> {
        match self {
            Self::Custom { interval_days } | Self::Movable { interval_days } => Some(interval_days),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: Id,
    pub title: String,
    pub description: String,
    /// ISO date (YYYY-MM-DD).
    pub due_date: Option<String>,
    pub project_id: Option<Id>,
    pub category_id: Option<Id>,
    pub subcategory_id: Option<Id>,
    /// Pinned tasks float to the top of task lists until unpinned or completed.
    #[serde(default)]
    pub pinned: bool,
    pub completed: bool,
    pub completed_at: Option<String>,
    pub repeat: RepeatConfig,
    pub series_id: Id,
    pub previous_instance_id: Option<Id>,
    pub created_at: String,
    pub updated_at: String,
}

/// A time-tracked item, independent of tasks. Time is logged in 15-minute increments;
/// `daily_minutes` maps an ISO date to the minutes worked that day, and `daily_notes` maps the
/// same dates to an optional note. Days that drop to zero are pruned from both maps.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TimeEntry {
    pub id: Id,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub category_id: Option<Id>,
    #[serde(default)]
    pub daily_minutes: BTreeMap<String, u32>,
    #[serde(default)]
    pub daily_notes: BTreeMap<String, String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub fn next(self) -> Self {
        match self {
            Self::System => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::System,
        }
    }
}

/// Lasting preferences. The web app keeps these in localStorage; here they live in the data file.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme_mode: ThemeMode,
    /// Ask before marking a task complete.
    pub confirm_completion: bool,
    /// Time page: group items under their category.
    pub group_by_category: bool,
    /// Time page: sort items by title.
    pub sort_alphabetically: bool,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct ProjectInput {
    pub title: String,
    pub description: String,
    pub due_date: Option<String>,
    pub category_id: Option<Id>,
    pub subcategory_id: Option<Id>,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct TaskInput {
    pub title: String,
    pub description: String,
    pub due_date: Option<String>,
    pub project_id: Option<Id>,
    pub category_id: Option<Id>,
    pub subcategory_id: Option<Id>,
    pub repeat: RepeatConfig,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct TimeEntryInput {
    pub title: String,
    pub description: String,
    pub category_id: Option<Id>,
}
