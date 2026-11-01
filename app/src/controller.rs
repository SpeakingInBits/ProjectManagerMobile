//! Owns the app data and UI state, handles every UI callback, and pushes fresh view models to
//! the window after each change.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use chrono::{Datelike, NaiveDate};
use project_core::date::{format_naive, parse_iso, to_iso, today};
use project_core::demo::demo_data;
use project_core::domain::text_cmp;
use project_core::model::{
    Id, ProjectInput, RepeatConfig, TaskInput, ThemeMode, TimeEntry, TimeEntryInput,
};
use project_core::store::{Direction, Store};
use project_core::time::{INCREMENT_MINUTES, ViewOptions, format_minutes, total_minutes};
use slint::{ComponentHandle, SharedString, Weak};

use crate::viewmodel::{self, model, strings};
use crate::{App, AppWindow, Date, Theme};

/// What the confirm dialog does when accepted.
enum Pending {
    CompleteTask(Id),
    DeleteTask(Id),
    /// Confirm deletes the project's tasks too; the alternative keeps them as standalone tasks.
    DeleteProject(Id),
    DeleteTimeItem(Id),
    DeleteCategory(Id),
    DeleteSubcategory(Id),
}

enum Rename {
    Category(Id),
    Subcategory(Id),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sheet {
    Closed = 0,
    Project = 1,
    Task = 2,
    TimeItem = 3,
    Merge = 4,
    TimeSettings = 5,
    TaskSettings = 6,
    DayNote = 7,
    History = 8,
    Rename = 9,
}

const REPEAT_KINDS: usize = 7;
const DEFAULT_INTERVAL_DAYS: u32 = 7;

fn repeat_index(r: RepeatConfig) -> i32 {
    match r {
        RepeatConfig::Never => 0,
        RepeatConfig::Daily => 1,
        RepeatConfig::Weekly => 2,
        RepeatConfig::Monthly => 3,
        RepeatConfig::Yearly => 4,
        RepeatConfig::Custom { .. } => 5,
        RepeatConfig::Movable { .. } => 6,
    }
}

fn repeat_from(index: i32, interval: &str) -> RepeatConfig {
    // Like the web form: a whole number of days, at least 1.
    let interval_days = interval
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .map_or(1, |n| n.trunc().max(1.0).min(f64::from(u32::MAX)) as u32);
    match index.clamp(0, REPEAT_KINDS as i32 - 1) {
        1 => RepeatConfig::Daily,
        2 => RepeatConfig::Weekly,
        3 => RepeatConfig::Monthly,
        4 => RepeatConfig::Yearly,
        5 => RepeatConfig::Custom { interval_days },
        6 => RepeatConfig::Movable { interval_days },
        _ => RepeatConfig::Never,
    }
}

fn to_date(d: NaiveDate) -> Date {
    Date {
        year: d.year(),
        month: d.month() as i32,
        day: d.day() as i32,
    }
}

fn from_date(d: &Date) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.year, d.month.clamp(1, 12) as u32, d.day.max(1) as u32)
        .unwrap_or_else(today)
}

fn index_of(ids: &[Option<Id>], id: Option<&str>) -> i32 {
    ids.iter().position(|x| x.as_deref() == id).unwrap_or(0) as i32
}

fn trimmed(s: SharedString) -> String {
    s.trim().to_string()
}

struct State {
    store: Store,
    /// Project shown in the detail view.
    detail: Option<Id>,
    /// Day that time and notes are logged to: normally today, changeable to backfill missed days.
    /// Kept for the session only, like the web app.
    entry_date: NaiveDate,
    selected_category: Option<Id>,
    editing_project: Option<Id>,
    editing_task: Option<Id>,
    editing_time_item: Option<Id>,
    /// Ids behind the category options (index 0 = none).
    category_ids: Vec<Option<Id>>,
    /// Ids behind the task form's project options (index 0 = standalone).
    project_ids: Vec<Option<Id>>,
    /// Ids behind the merge options (index 0 = none chosen).
    merge_ids: Vec<Option<Id>>,
    /// Time item and date whose note is being edited, and whether to return to its history.
    note_target: Option<(Id, String)>,
    note_from_history: bool,
    history_item: Option<Id>,
    rename: Option<Rename>,
    pending: Option<Pending>,
}

impl State {
    fn subcategory_ids(&self, category_index: i32) -> Vec<Option<Id>> {
        let category = self
            .category_ids
            .get(category_index as usize)
            .cloned()
            .flatten();
        std::iter::once(None)
            .chain(
                category
                    .iter()
                    .flat_map(|c| self.store.data.subcategories_of(c))
                    .map(|s| Some(s.id.clone()))
                    .collect::<Vec<_>>(),
            )
            .collect()
    }

    fn view_options(&self) -> ViewOptions {
        let s = self.store.data.settings;
        ViewOptions {
            group_by_category: s.group_by_category,
            sort_alphabetically: s.sort_alphabetically,
        }
    }
}

pub struct Controller {
    ui: Weak<AppWindow>,
    state: RefCell<State>,
}

/// Wires a controller to `ui`, loading data from `data_path` (or replacing it with demo data).
pub fn attach(ui: &AppWindow, data_path: PathBuf, seed: bool) -> Rc<Controller> {
    let mut store = Store::open(data_path);
    if seed {
        // Keep the theme preference; everything else is replaced.
        let mut data = demo_data();
        data.settings = store.data.settings;
        let _ = store.replace(data);
    }
    ui.global::<Theme>()
        .set_mode(theme_index(store.data.settings.theme_mode));
    let c = Rc::new(Controller {
        ui: ui.as_weak(),
        state: RefCell::new(State {
            store,
            detail: None,
            entry_date: today(),
            selected_category: None,
            editing_project: None,
            editing_task: None,
            editing_time_item: None,
            category_ids: Vec::new(),
            project_ids: Vec::new(),
            merge_ids: Vec::new(),
            note_target: None,
            note_from_history: false,
            history_item: None,
            rename: None,
            pending: None,
        }),
    });
    c.bind(ui);
    c.refresh();
    c
}

fn theme_index(mode: ThemeMode) -> i32 {
    match mode {
        ThemeMode::System => 0,
        ThemeMode::Light => 1,
        ThemeMode::Dark => 2,
    }
}

impl Controller {
    fn bind(self: &Rc<Self>, ui: &AppWindow) {
        let app = ui.global::<App>();
        macro_rules! on {
            ($setter:ident, |$c:ident $(, $arg:ident)*| $body:expr) => {{
                let weak = Rc::downgrade(self);
                app.$setter(move |$($arg),*| {
                    let $c = weak.upgrade().expect("controller outlives the window");
                    $body
                });
            }};
        }

        on!(on_page_changed, |c, _page| {
            c.state.borrow_mut().detail = None;
            c.close_sheet();
        });
        on!(on_cycle_theme, |c| c.cycle_theme());
        on!(on_format_date, |_c, d| format_naive(from_date(&d)).into());
        on!(on_subcategory_options, |c, index| {
            let st = c.state.borrow();
            strings(
                std::iter::once("No subcategory".to_string()).chain(
                    st.subcategory_ids(index)
                        .iter()
                        .flatten()
                        .filter_map(|id| st.store.data.subcategory(Some(id)))
                        .map(|s| s.name.clone()),
                ),
            )
        });

        // Projects
        on!(on_add_project, |c| c.open_project_form(None));
        on!(on_open_project, |c, id| {
            c.state.borrow_mut().detail = Some(id.to_string());
            c.with_app(|app| app.set_detail_show_completed(false));
            c.refresh();
        });
        on!(on_move_project, |c, id, up| c.save(|s| s.move_project(
            &id,
            if up { Direction::Up } else { Direction::Down }
        )));
        on!(on_close_project, |c| {
            c.state.borrow_mut().detail = None;
            c.refresh();
        });
        on!(on_edit_project, |c| {
            let id = c.state.borrow().detail.clone();
            c.open_project_form(id);
        });
        on!(on_delete_project, |c| c.request_delete_project());
        on!(on_add_project_task, |c| {
            let project = c.state.borrow().detail.clone();
            c.open_task_form(None, project);
        });
        on!(on_detail_filter_changed, |c| c.refresh());
        on!(on_submit_project, |c| c.submit_project());

        // Tasks
        on!(on_add_task, |c| c.open_task_form(None, None));
        on!(on_edit_task, |c, id| c
            .open_task_form(Some(id.to_string()), None));
        on!(on_delete_task, |c, id| {
            let title = c.task_title(&id);
            c.confirm(
                &format!("Delete task \"{title}\"?"),
                "Delete",
                Pending::DeleteTask(id.to_string()),
            );
        });
        on!(on_toggle_task, |c, id| c.toggle_task(id.to_string()));
        on!(on_pin_task, |c, id| {
            let pinned = c
                .state
                .borrow()
                .store
                .data
                .task(&id)
                .is_some_and(|t| t.pinned);
            c.save(|s| s.set_pinned(&id, !pinned));
        });
        on!(on_tasks_filter_changed, |c| c.refresh());
        on!(on_submit_task, |c| c.submit_task());
        on!(on_open_task_settings, |c| {
            let confirm = c.state.borrow().store.data.settings.confirm_completion;
            c.with_app(|app| app.set_ks_confirm(confirm));
            c.open_sheet(Sheet::TaskSettings, "Task settings");
        });
        on!(on_submit_task_settings, |c| {
            let Some(confirm) = c.read_app(|app| app.get_ks_confirm()) else {
                return;
            };
            c.close_sheet();
            c.save(|s| s.update_settings(|st| st.confirm_completion = confirm));
        });

        // Time
        on!(on_add_time_item, |c| c.open_time_item_form(None));
        on!(on_edit_time_item, |c, id| c
            .open_time_item_form(Some(id.to_string())));
        on!(on_delete_time_item, |c, id| {
            let title = c.time_item_title(&id);
            c.confirm(
                &format!(
                    "Delete time-tracked item \"{title}\"? This also deletes its logged time."
                ),
                "Delete",
                Pending::DeleteTimeItem(id.to_string()),
            );
        });
        on!(on_step_time, |c, id, add| {
            let day = to_iso(c.state.borrow().entry_date);
            let delta = if add {
                INCREMENT_MINUTES
            } else {
                -INCREMENT_MINUTES
            };
            c.save(|s| s.add_minutes_on_day(&id, &day, delta));
        });
        on!(on_edit_note, |c, id| {
            let day = to_iso(c.state.borrow().entry_date);
            c.open_note(id.to_string(), day, false);
        });
        on!(on_open_history, |c, id| {
            c.state.borrow_mut().history_item = Some(id.to_string());
            c.open_history();
        });
        on!(on_history_pick, |c, date| {
            let Some(id) = c.state.borrow().history_item.clone() else {
                return;
            };
            c.open_note(id, date.to_string(), true);
        });
        on!(on_submit_note, |c| c.submit_note());
        on!(on_submit_time_item, |c| c.submit_time_item());
        on!(on_open_merge, |c| c.open_merge());
        on!(on_submit_merge, |c| c.submit_merge());
        on!(on_open_time_settings, |c| {
            let (date, s) = {
                let st = c.state.borrow();
                (st.entry_date, st.store.data.settings)
            };
            c.with_app(|app| {
                app.set_ts_date(to_date(date));
                app.set_ts_group(s.group_by_category);
                app.set_ts_sort(s.sort_alphabetically);
            });
            c.open_sheet(Sheet::TimeSettings, "Time tracking settings");
        });
        on!(on_submit_time_settings, |c| {
            let Some((date, group, sort)) = c.read_app(|app| {
                (
                    from_date(&app.get_ts_date()),
                    app.get_ts_group(),
                    app.get_ts_sort(),
                )
            }) else {
                return;
            };
            c.state.borrow_mut().entry_date = date;
            c.close_sheet();
            c.save(|s| {
                s.update_settings(|st| {
                    st.group_by_category = group;
                    st.sort_alphabetically = sort;
                })
            });
        });
        on!(on_time_use_today, |c| {
            c.state.borrow_mut().entry_date = today();
            c.refresh();
        });

        // Categories
        on!(on_select_category, |c, id| {
            c.state.borrow_mut().selected_category = Some(id.to_string());
            c.refresh();
        });
        on!(on_add_category, |c| {
            let Some(name) = c.read_app(|app| trimmed(app.get_new_category_name())) else {
                return;
            };
            if name.is_empty() {
                return;
            }
            let result = c.state.borrow_mut().store.add_category(&name);
            match result {
                Ok(id) => {
                    c.state.borrow_mut().selected_category = Some(id);
                    c.with_app(|app| app.set_new_category_name("".into()));
                    c.refresh();
                }
                Err(e) => c.save_failed(&e),
            }
        });
        on!(on_add_subcategory, |c| {
            let Some(name) = c.read_app(|app| trimmed(app.get_new_subcategory_name())) else {
                return;
            };
            let Some(category) = c.state.borrow().selected_category.clone() else {
                return;
            };
            if name.is_empty() {
                return;
            }
            c.with_app(|app| app.set_new_subcategory_name("".into()));
            c.save(|s| s.add_subcategory(&category, &name).map(|_| ()));
        });
        on!(on_rename_category, |c, id| c
            .open_rename(Rename::Category(id.to_string())));
        on!(on_rename_subcategory, |c, id| c
            .open_rename(Rename::Subcategory(id.to_string())));
        on!(on_delete_category, |c, id| {
            let name = c.category_name(&id);
            c.confirm(
                &format!(
                    "Delete category \"{name}\"? Its subcategories will be removed and any \
                     projects, tasks and time items using it will become uncategorized."
                ),
                "Delete",
                Pending::DeleteCategory(id.to_string()),
            );
        });
        on!(on_delete_subcategory, |c, id| {
            let name = c
                .state
                .borrow()
                .store
                .data
                .subcategory(Some(&id))
                .map(|s| s.name.clone())
                .unwrap_or_default();
            c.confirm(
                &format!("Delete subcategory \"{name}\"?"),
                "Delete",
                Pending::DeleteSubcategory(id.to_string()),
            );
        });
        on!(on_submit_rename, |c| c.submit_rename());

        // Sheet and dialog
        on!(on_close_sheet, |c| c.close_sheet());
        on!(on_dialog_confirmed, |c| c.dialog_done(true));
        on!(on_dialog_alt, |c| c.dialog_done(false));
        on!(on_dialog_cancelled, |c| {
            c.state.borrow_mut().pending = None;
            c.with_app(|app| app.set_dialog_visible(false));
        });
    }

    fn with_app(&self, f: impl FnOnce(App<'_>)) {
        if let Some(ui) = self.ui.upgrade() {
            f(ui.global::<App>());
        }
    }

    fn read_app<T>(&self, f: impl FnOnce(App<'_>) -> T) -> Option<T> {
        self.ui.upgrade().map(|ui| f(ui.global::<App>()))
    }

    /// Runs a store mutation, reporting a failed save, then refreshes the UI.
    fn save(&self, f: impl FnOnce(&mut Store) -> std::io::Result<()>) {
        let result = f(&mut self.state.borrow_mut().store);
        if let Err(e) = result {
            self.save_failed(&e);
        }
        self.refresh();
    }

    fn save_failed(&self, e: &std::io::Error) {
        self.alert(&format!("Couldn't save your changes: {e}"));
    }

    fn cycle_theme(&self) {
        let next = self.state.borrow().store.data.settings.theme_mode.next();
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<Theme>().set_mode(theme_index(next));
        }
        self.save(|s| s.set_theme_mode(next));
    }

    // --- Refresh --------------------------------------------------------------------------

    /// Recomputes every view model from the data and pushes it to the window.
    fn refresh(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let app = ui.global::<App>();
        let mut st = self.state.borrow_mut();
        let now = today();
        app.set_today(to_date(now));

        // Drop references to things that were deleted.
        if st
            .detail
            .as_deref()
            .is_some_and(|id| st.store.data.project(id).is_none())
        {
            st.detail = None;
        }
        let selected_ok = st
            .selected_category
            .as_deref()
            .is_some_and(|id| st.store.data.category(Some(id)).is_some());
        if !selected_ok {
            st.selected_category = st.store.data.categories.first().map(|c| c.id.clone());
        }

        let data = &st.store.data;
        app.set_projects(model(viewmodel::project_cards(data)));

        app.set_detail_open(st.detail.is_some());
        if let Some(project) = st.detail.as_deref().and_then(|id| data.project(id)) {
            let projects = data.projects_sorted();
            let index = projects
                .iter()
                .position(|p| p.id == project.id)
                .unwrap_or(0);
            app.set_detail(viewmodel::project_card(
                data,
                project,
                index,
                projects.len(),
            ));
            app.set_detail_tasks(model(viewmodel::task_rows(
                data,
                data.tasks_of_project(&project.id),
                app.get_detail_show_completed(),
                false,
            )));
        }

        let all_tasks = app.get_tasks_scope() == 0;
        app.set_tasks(model(viewmodel::task_rows(
            data,
            data.tasks
                .iter()
                .filter(|t| all_tasks || t.project_id.is_none()),
            app.get_tasks_show_completed(),
            all_tasks,
        )));

        let entries = st.store.time_entries();
        let entry_date = st.entry_date;
        let day_label = viewmodel::day_label(entry_date, now);
        app.set_time_is_today(entry_date == now);
        app.set_time_day_label(day_label.as_str().into());
        app.set_time_warning(
            format!(
                "You are logging time for {}, not today. New time and notes will be saved to \
                 that day.",
                format_naive(entry_date)
            )
            .into(),
        );
        app.set_time_overall(viewmodel::summary(
            "All items",
            entries.iter().copied(),
            entry_date,
            now,
        ));
        app.set_time_category_cards(model(viewmodel::category_summaries(
            data, &entries, entry_date, now,
        )));
        let ordered: Vec<TimeEntry> = entries.iter().map(|e| (*e).clone()).collect();
        app.set_time_items(model(viewmodel::time_items(
            data,
            &ordered,
            st.view_options(),
            entry_date,
            now,
        )));

        let selected = st.selected_category.as_deref();
        app.set_categories(model(
            data.categories
                .iter()
                .map(|c| viewmodel::named_row(&c.id, &c.name, Some(c.id.as_str()) == selected))
                .collect(),
        ));
        app.set_selected_category_name(
            data.category(selected)
                .map_or_else(SharedString::new, |c| c.name.as_str().into()),
        );
        app.set_subcategories(model(
            selected
                .map(|id| {
                    data.subcategories_of(id)
                        .map(|s| viewmodel::named_row(&s.id, &s.name, false))
                        .collect()
                })
                .unwrap_or_default(),
        ));

        // Picker options for the forms.
        let category_ids: Vec<Option<Id>> = std::iter::once(None)
            .chain(data.categories.iter().map(|c| Some(c.id.clone())))
            .collect();
        app.set_category_options(strings(
            std::iter::once("No category".to_string())
                .chain(data.categories.iter().map(|c| c.name.clone())),
        ));
        let projects = data.projects_sorted();
        let project_ids: Vec<Option<Id>> = std::iter::once(None)
            .chain(projects.iter().map(|p| Some(p.id.clone())))
            .collect();
        app.set_project_options(strings(
            std::iter::once("Standalone (no project)".to_string())
                .chain(projects.iter().map(|p| p.title.clone())),
        ));
        st.category_ids = category_ids;
        st.project_ids = project_ids;
    }

    // --- Sheets ---------------------------------------------------------------------------

    fn open_sheet(&self, sheet: Sheet, title: &str) {
        self.with_app(|app| {
            app.set_sheet_title(title.into());
            app.set_sheet(sheet as i32);
        });
    }

    fn close_sheet(&self) {
        let back_to_history = {
            let mut st = self.state.borrow_mut();
            let is_note = self.read_app(|app| app.get_sheet()) == Some(Sheet::DayNote as i32);
            let back = is_note && st.note_from_history;
            st.note_target = None;
            st.note_from_history = false;
            st.editing_project = None;
            st.editing_task = None;
            st.editing_time_item = None;
            st.rename = None;
            back
        };
        if back_to_history {
            self.open_history();
        } else {
            self.with_app(|app| app.set_sheet(Sheet::Closed as i32));
        }
    }

    fn open_project_form(&self, id: Option<Id>) {
        let project = id
            .as_deref()
            .and_then(|id| self.state.borrow().store.data.project(id).cloned());
        let (category, subcategory) = {
            let st = self.state.borrow();
            let category = index_of(
                &st.category_ids,
                project.as_ref().and_then(|p| p.category_id.as_deref()),
            );
            let subcategory = index_of(
                &st.subcategory_ids(category),
                project.as_ref().and_then(|p| p.subcategory_id.as_deref()),
            );
            (category, subcategory)
        };
        let due = project
            .as_ref()
            .and_then(|p| p.due_date.as_deref())
            .and_then(parse_iso);
        self.with_app(|app| {
            app.set_pf_title(project.as_ref().map_or("", |p| p.title.as_str()).into());
            app.set_pf_description(
                project
                    .as_ref()
                    .map_or("", |p| p.description.as_str())
                    .into(),
            );
            app.set_pf_has_due(due.is_some());
            app.set_pf_due(to_date(due.unwrap_or_else(today)));
            app.set_pf_category(category);
            app.set_pf_subcategory(subcategory);
            app.set_pf_title_missing(false);
            app.set_pf_editing(project.is_some());
        });
        let title = if project.is_some() {
            "Edit project"
        } else {
            "New project"
        };
        self.state.borrow_mut().editing_project = project.map(|p| p.id);
        self.open_sheet(Sheet::Project, title);
    }

    fn submit_project(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let app = ui.global::<App>();
        let title = trimmed(app.get_pf_title());
        if title.is_empty() {
            app.set_pf_title_missing(true);
            return;
        }
        let input = {
            let st = self.state.borrow();
            let category = app.get_pf_category();
            ProjectInput {
                title,
                description: trimmed(app.get_pf_description()),
                due_date: app
                    .get_pf_has_due()
                    .then(|| to_iso(from_date(&app.get_pf_due()))),
                category_id: st.category_ids.get(category as usize).cloned().flatten(),
                subcategory_id: st
                    .subcategory_ids(category)
                    .get(app.get_pf_subcategory() as usize)
                    .cloned()
                    .flatten(),
            }
        };
        let editing = self.state.borrow().editing_project.clone();
        let result = {
            let mut st = self.state.borrow_mut();
            match editing {
                Some(id) => st.store.update_project(&id, input),
                // Like the web app, a new project opens straight away.
                None => st.store.add_project(input).map(|id| st.detail = Some(id)),
            }
        };
        self.close_sheet();
        if let Err(e) = result {
            self.save_failed(&e);
        }
        self.refresh();
    }

    fn request_delete_project(&self) {
        let Some(id) = self.state.borrow().detail.clone() else {
            return;
        };
        let (title, count) = {
            let st = self.state.borrow();
            let title = st
                .store
                .data
                .project(&id)
                .map(|p| p.title.clone())
                .unwrap_or_default();
            (title, st.store.data.tasks_of_project(&id).count())
        };
        if count == 0 {
            self.confirm(
                &format!("Delete project \"{title}\"? This cannot be undone."),
                "Delete project",
                Pending::DeleteProject(id),
            );
            return;
        }
        let tasks = if count == 1 {
            "1 task".to_string()
        } else {
            format!("{count} tasks")
        };
        self.show_dialog(
            &format!(
                "Delete project \"{title}\"? This cannot be undone.\n\nIt has {tasks}. Delete \
                 them too, or keep them as standalone tasks?"
            ),
            "Delete project and tasks",
            "Keep tasks as standalone",
            true,
            Some(Pending::DeleteProject(id)),
        );
    }

    fn open_task_form(&self, id: Option<Id>, project: Option<Id>) {
        let (task, values) = {
            let st = self.state.borrow();
            let data = &st.store.data;
            let task = id.as_deref().and_then(|id| data.task(id)).cloned();
            let project_id = task.as_ref().map_or(project, |t| t.project_id.clone());
            // A new task in a project starts in the project's category.
            let (category_id, subcategory_id) = match &task {
                Some(t) => (t.category_id.clone(), t.subcategory_id.clone()),
                None => project_id
                    .as_deref()
                    .and_then(|p| data.project(p))
                    .map_or((None, None), |p| {
                        (p.category_id.clone(), p.subcategory_id.clone())
                    }),
            };
            let category = index_of(&st.category_ids, category_id.as_deref());
            let subcategory = index_of(&st.subcategory_ids(category), subcategory_id.as_deref());
            let project = index_of(&st.project_ids, project_id.as_deref());
            (task, (project, category, subcategory))
        };
        let (project, category, subcategory) = values;
        let due = task
            .as_ref()
            .and_then(|t| t.due_date.as_deref())
            .and_then(parse_iso);
        let repeat = task.as_ref().map_or(RepeatConfig::Never, |t| t.repeat);
        self.with_app(|app| {
            app.set_tf_title(task.as_ref().map_or("", |t| t.title.as_str()).into());
            app.set_tf_description(task.as_ref().map_or("", |t| t.description.as_str()).into());
            app.set_tf_has_due(due.is_some());
            app.set_tf_due(to_date(due.unwrap_or_else(today)));
            app.set_tf_project(project);
            app.set_tf_category(category);
            app.set_tf_subcategory(subcategory);
            app.set_tf_repeat(repeat_index(repeat));
            app.set_tf_interval(
                repeat
                    .interval_days()
                    .unwrap_or(DEFAULT_INTERVAL_DAYS)
                    .to_string()
                    .into(),
            );
            app.set_tf_title_missing(false);
            app.set_tf_editing(task.is_some());
        });
        let title = if task.is_some() {
            "Edit task"
        } else {
            "New task"
        };
        self.state.borrow_mut().editing_task = task.map(|t| t.id);
        self.open_sheet(Sheet::Task, title);
    }

    fn submit_task(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let app = ui.global::<App>();
        let title = trimmed(app.get_tf_title());
        if title.is_empty() {
            app.set_tf_title_missing(true);
            return;
        }
        let input = {
            let st = self.state.borrow();
            let category = app.get_tf_category();
            TaskInput {
                title,
                description: trimmed(app.get_tf_description()),
                due_date: app
                    .get_tf_has_due()
                    .then(|| to_iso(from_date(&app.get_tf_due()))),
                project_id: st
                    .project_ids
                    .get(app.get_tf_project() as usize)
                    .cloned()
                    .flatten(),
                category_id: st.category_ids.get(category as usize).cloned().flatten(),
                subcategory_id: st
                    .subcategory_ids(category)
                    .get(app.get_tf_subcategory() as usize)
                    .cloned()
                    .flatten(),
                repeat: repeat_from(app.get_tf_repeat(), &app.get_tf_interval()),
            }
        };
        let editing = self.state.borrow().editing_task.clone();
        self.close_sheet();
        self.save(|s| match editing {
            Some(id) => s.update_task(&id, input),
            None => s.add_task(input).map(|_| ()),
        });
    }

    fn toggle_task(&self, id: Id) {
        let (task, confirm) = {
            let st = self.state.borrow();
            (
                st.store.data.task(&id).cloned(),
                st.store.data.settings.confirm_completion,
            )
        };
        let Some(task) = task else { return };
        // Only marking complete is guarded (reopening is harmless), and only when turned on.
        if !task.completed && confirm {
            let repeat_note = if task.repeat.repeats() {
                "\n\nThis task repeats — its next occurrence will be created."
            } else {
                ""
            };
            self.show_dialog(
                &format!("Mark \"{}\" as completed?{repeat_note}", task.title),
                "Complete",
                "",
                false,
                Some(Pending::CompleteTask(id)),
            );
            return;
        }
        self.save(|s| s.toggle_task(&id));
    }

    fn open_time_item_form(&self, id: Option<Id>) {
        let (entry, category, can_merge) = {
            let st = self.state.borrow();
            let entry = id
                .as_deref()
                .and_then(|id| st.store.data.time_entry(id))
                .cloned();
            let category = index_of(
                &st.category_ids,
                entry.as_ref().and_then(|e| e.category_id.as_deref()),
            );
            let can_merge = entry.is_some() && st.store.data.time_entries.len() > 1;
            (entry, category, can_merge)
        };
        self.with_app(|app| {
            app.set_ef_title(entry.as_ref().map_or("", |e| e.title.as_str()).into());
            app.set_ef_description(entry.as_ref().map_or("", |e| e.description.as_str()).into());
            app.set_ef_category(category);
            app.set_ef_title_missing(false);
            app.set_ef_editing(entry.is_some());
            app.set_ef_can_merge(can_merge);
        });
        let title = if entry.is_some() {
            "Edit time item"
        } else {
            "New time item"
        };
        self.state.borrow_mut().editing_time_item = entry.map(|e| e.id);
        self.open_sheet(Sheet::TimeItem, title);
    }

    fn submit_time_item(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let app = ui.global::<App>();
        let title = trimmed(app.get_ef_title());
        if title.is_empty() {
            app.set_ef_title_missing(true);
            return;
        }
        let input = TimeEntryInput {
            title,
            description: trimmed(app.get_ef_description()),
            category_id: self
                .state
                .borrow()
                .category_ids
                .get(app.get_ef_category() as usize)
                .cloned()
                .flatten(),
        };
        let editing = self.state.borrow().editing_time_item.clone();
        self.close_sheet();
        self.save(|s| match editing {
            Some(id) => s.update_time_entry(&id, input),
            None => s.add_time_entry(input).map(|_| ()),
        });
    }

    fn open_merge(&self) {
        let Some(source_id) = self.state.borrow().editing_time_item.clone() else {
            return;
        };
        let (summary, options, ids, title) = {
            let st = self.state.borrow();
            let Some(source) = st.store.data.time_entry(&source_id) else {
                return;
            };
            let mut others: Vec<&TimeEntry> = st
                .store
                .data
                .time_entries
                .iter()
                .filter(|e| e.id != source_id)
                .collect();
            others.sort_by(|a, b| text_cmp(&a.title, &b.title));
            let days = source.daily_minutes.len();
            let notes = source.daily_notes.len();
            let plural =
                |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
            let logged = if days == 0 {
                "It has no logged time.".to_string()
            } else {
                format!(
                    "Its {} across {}{} will be added to the chosen item.",
                    format_minutes(total_minutes(source)),
                    plural(days, "day"),
                    if notes > 0 {
                        format!(" (with {})", plural(notes, "note"))
                    } else {
                        String::new()
                    }
                )
            };
            let summary = format!(
                "{logged} \"{}\" will then be deleted. The chosen item keeps its own title, \
                 description and category. This cannot be undone.",
                source.title
            );
            let options: Vec<String> = std::iter::once("Choose an item…".to_string())
                .chain(others.iter().map(|e| e.title.clone()))
                .collect();
            let ids: Vec<Option<Id>> = std::iter::once(None)
                .chain(others.iter().map(|e| Some(e.id.clone())))
                .collect();
            (summary, options, ids, format!("Merge \"{}\"", source.title))
        };
        self.state.borrow_mut().merge_ids = ids;
        self.with_app(|app| {
            app.set_merge_summary(summary.into());
            app.set_merge_options(strings(options));
            app.set_merge_index(0);
        });
        self.open_sheet(Sheet::Merge, &title);
    }

    fn submit_merge(&self) {
        let Some(index) = self.read_app(|app| app.get_merge_index()) else {
            return;
        };
        let (source, target) = {
            let st = self.state.borrow();
            (
                st.editing_time_item.clone(),
                st.merge_ids.get(index as usize).cloned().flatten(),
            )
        };
        let (Some(source), Some(target)) = (source, target) else {
            return;
        };
        self.close_sheet();
        self.save(|s| s.merge_time_entry(&source, &target));
    }

    fn open_note(&self, id: Id, date: String, from_history: bool) {
        let (title, note) = {
            let st = self.state.borrow();
            let Some(entry) = st.store.data.time_entry(&id) else {
                return;
            };
            (
                entry.title.clone(),
                entry.daily_notes.get(&date).cloned().unwrap_or_default(),
            )
        };
        let day = parse_iso(&date).unwrap_or_else(today);
        let day_label = if day == today() {
            "today".to_string()
        } else {
            format_naive(day)
        };
        let heading = if note.is_empty() {
            format!("Add note for {day_label}")
        } else {
            format!("Edit note for {day_label}")
        };
        self.with_app(|app| {
            app.set_note_text(note.into());
            app.set_note_subtitle(format!("{title} — {}", format_naive(day)).into());
        });
        {
            let mut st = self.state.borrow_mut();
            st.note_target = Some((id, date));
            st.note_from_history = from_history;
        }
        self.open_sheet(Sheet::DayNote, &heading);
    }

    fn submit_note(&self) {
        let Some(note) = self.read_app(|app| app.get_note_text().to_string()) else {
            return;
        };
        let Some((id, date)) = self.state.borrow().note_target.clone() else {
            return;
        };
        let result = self
            .state
            .borrow_mut()
            .store
            .set_day_note(&id, &date, &note);
        if let Err(e) = result {
            self.save_failed(&e);
        }
        self.refresh();
        self.close_sheet();
    }

    fn open_history(&self) {
        let (title, rows) = {
            let st = self.state.borrow();
            let Some(entry) = st
                .history_item
                .as_deref()
                .and_then(|id| st.store.data.time_entry(id))
            else {
                drop(st);
                self.with_app(|app| app.set_sheet(Sheet::Closed as i32));
                return;
            };
            (entry.title.clone(), viewmodel::history_rows(entry))
        };
        self.with_app(|app| app.set_history_rows(model(rows)));
        self.open_sheet(Sheet::History, &title);
    }

    fn open_rename(&self, target: Rename) {
        let (name, title) = {
            let st = self.state.borrow();
            let data = &st.store.data;
            match &target {
                Rename::Category(id) => (
                    data.category(Some(id)).map(|c| c.name.clone()),
                    "Rename category",
                ),
                Rename::Subcategory(id) => (
                    data.subcategory(Some(id)).map(|s| s.name.clone()),
                    "Rename subcategory",
                ),
            }
        };
        let Some(name) = name else { return };
        self.with_app(|app| {
            app.set_rename_text(name.into());
            app.set_rename_missing(false);
        });
        self.state.borrow_mut().rename = Some(target);
        self.open_sheet(Sheet::Rename, title);
    }

    fn submit_rename(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let app = ui.global::<App>();
        let name = trimmed(app.get_rename_text());
        if name.is_empty() {
            app.set_rename_missing(true);
            return;
        }
        let target = self.state.borrow_mut().rename.take();
        self.close_sheet();
        self.save(|s| match target {
            Some(Rename::Category(id)) => s.rename_category(&id, &name),
            Some(Rename::Subcategory(id)) => s.rename_subcategory(&id, &name),
            None => Ok(()),
        });
    }

    // --- Dialogs --------------------------------------------------------------------------

    fn show_dialog(
        &self,
        message: &str,
        confirm_label: &str,
        alt_label: &str,
        danger: bool,
        pending: Option<Pending>,
    ) {
        let can_cancel = pending.is_some();
        self.state.borrow_mut().pending = pending;
        self.with_app(|app| {
            app.set_dialog_message(message.into());
            app.set_dialog_confirm_label(confirm_label.into());
            app.set_dialog_alt_label(alt_label.into());
            app.set_dialog_can_cancel(can_cancel);
            app.set_dialog_danger(danger);
            app.set_dialog_visible(true);
        });
    }

    fn confirm(&self, message: &str, confirm_label: &str, pending: Pending) {
        self.show_dialog(message, confirm_label, "", true, Some(pending));
    }

    fn alert(&self, message: &str) {
        self.show_dialog(message, "OK", "", false, None);
    }

    /// `confirmed` is the main action; false is the dialog's alternative action.
    fn dialog_done(&self, confirmed: bool) {
        let pending = self.state.borrow_mut().pending.take();
        self.with_app(|app| app.set_dialog_visible(false));
        let Some(pending) = pending else { return };
        match pending {
            Pending::CompleteTask(id) => self.save(|s| s.toggle_task(&id)),
            Pending::DeleteTask(id) => self.save(|s| s.remove_task(&id)),
            Pending::DeleteProject(id) => self.save(|s| s.remove_project(&id, confirmed)),
            Pending::DeleteTimeItem(id) => self.save(|s| s.remove_time_entry(&id)),
            Pending::DeleteCategory(id) => self.save(|s| s.remove_category(&id)),
            Pending::DeleteSubcategory(id) => self.save(|s| s.remove_subcategory(&id)),
        }
    }

    fn task_title(&self, id: &str) -> String {
        let st = self.state.borrow();
        st.store
            .data
            .task(id)
            .map(|t| t.title.clone())
            .unwrap_or_default()
    }

    fn time_item_title(&self, id: &str) -> String {
        let st = self.state.borrow();
        st.store
            .data
            .time_entry(id)
            .map(|e| e.title.clone())
            .unwrap_or_default()
    }

    fn category_name(&self, id: &str) -> String {
        let st = self.state.borrow();
        st.store
            .data
            .category(Some(id))
            .map(|c| c.name.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_options_round_trip() {
        for r in [
            RepeatConfig::Never,
            RepeatConfig::Daily,
            RepeatConfig::Weekly,
            RepeatConfig::Monthly,
            RepeatConfig::Yearly,
            RepeatConfig::Custom { interval_days: 10 },
            RepeatConfig::Movable { interval_days: 30 },
        ] {
            let interval = r
                .interval_days()
                .unwrap_or(DEFAULT_INTERVAL_DAYS)
                .to_string();
            assert_eq!(repeat_from(repeat_index(r), &interval), r);
        }
    }

    #[test]
    fn interval_is_a_whole_number_of_days_at_least_one() {
        assert_eq!(
            repeat_from(5, "2.9"),
            RepeatConfig::Custom { interval_days: 2 }
        );
        assert_eq!(
            repeat_from(5, "0"),
            RepeatConfig::Custom { interval_days: 1 }
        );
        assert_eq!(
            repeat_from(6, "-4"),
            RepeatConfig::Movable { interval_days: 1 }
        );
        assert_eq!(
            repeat_from(6, "abc"),
            RepeatConfig::Movable { interval_days: 1 }
        );
    }
}
