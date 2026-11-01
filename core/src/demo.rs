//! Reproducible demo data, ported from the web app's `dev/seed.ts`: categories, projects (some
//! overdue), tasks covering every repeat kind plus pinned/completed states, and time items with
//! about ten weeks of history. Used by the desktop build's `--seed` option.

use chrono::{Datelike, Weekday};

use crate::date::{add_days, now_iso, to_iso, today};
use crate::model::{ProjectInput, RepeatConfig, TaskInput, TimeEntryInput};
use crate::store::{Data, Store};
use crate::time::INCREMENT_MINUTES;

/// mulberry32: a tiny deterministic PRNG, so every seed run builds the same dataset.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let a = self.0;
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }

    fn chance(&mut self, p: f64) -> bool {
        self.next() < p
    }

    fn int(&mut self, min: u32, max: u32) -> u32 {
        min + (self.next() * f64::from(max - min + 1)) as u32
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[(self.next() * items.len() as f64) as usize]
    }
}

fn days_from_today(days: i64) -> Option<String> {
    Some(to_iso(add_days(today(), days)))
}

pub fn demo_data() -> Data {
    let mut rng = Rng(42);
    let mut s = Store::in_memory(Data::default());
    let ok = "in-memory store never fails to save";

    let work = s.add_category("Work").expect(ok);
    let personal = s.add_category("Personal").expect(ok);
    let learning = s.add_category("Learning").expect(ok);
    let client_a = s.add_subcategory(&work, "Client A").expect(ok);
    let internal = s.add_subcategory(&work, "Internal").expect(ok);
    let home = s.add_subcategory(&personal, "Home").expect(ok);
    let health = s.add_subcategory(&personal, "Health").expect(ok);
    let courses = s.add_subcategory(&learning, "Courses").expect(ok);

    let mut project =
        |title: &str, description: &str, due: Option<i64>, cat: Option<&str>, sub: Option<&str>| {
            s.add_project(ProjectInput {
                title: title.into(),
                description: description.into(),
                due_date: due.and_then(days_from_today),
                category_id: cat.map(Into::into),
                subcategory_id: sub.map(Into::into),
            })
            .expect(ok)
        };
    let website = project(
        "Client A website redesign",
        "Full redesign of the marketing site: new IA, visual refresh, CMS migration.",
        Some(21),
        Some(&work),
        Some(&client_a),
    );
    let audit = project(
        "Q3 security audit",
        "Internal audit of auth flows and dependency versions.",
        Some(-3),
        Some(&work),
        Some(&internal),
    );
    let kitchen = project(
        "Kitchen renovation",
        "Cabinets, counters, and that leaky faucet.",
        Some(45),
        Some(&personal),
        Some(&home),
    );
    let ts_course = project(
        "TypeScript deep-dive course",
        "Work through the advanced-types course, one module at a time.",
        None,
        Some(&learning),
        Some(&courses),
    );
    let garden = project(
        "Spring garden prep",
        "No due date, no category — a bare-bones project.",
        None,
        None,
        None,
    );

    struct Seed<'a> {
        title: &'a str,
        project: Option<&'a str>,
        cat: Option<&'a str>,
        sub: Option<&'a str>,
        due: Option<i64>,
        repeat: RepeatConfig,
        completed: bool,
        pinned: bool,
        description: &'a str,
    }
    let t = |title, project, cat, sub, due| Seed {
        title,
        project,
        cat,
        sub,
        due,
        repeat: RepeatConfig::Never,
        completed: false,
        pinned: false,
        description: "",
    };
    let (w, p, l) = (
        Some(work.as_str()),
        Some(personal.as_str()),
        Some(learning.as_str()),
    );
    let (ca, int, hm, hl, co) = (
        Some(client_a.as_str()),
        Some(internal.as_str()),
        Some(home.as_str()),
        Some(health.as_str()),
        Some(courses.as_str()),
    );
    let (web, aud, kit, ts, gar) = (
        Some(website.as_str()),
        Some(audit.as_str()),
        Some(kitchen.as_str()),
        Some(ts_course.as_str()),
        Some(garden.as_str()),
    );
    let tasks = [
        Seed {
            completed: true,
            ..t("Audit current site content", web, w, ca, Some(-14))
        },
        Seed {
            completed: true,
            ..t("Wireframes for top 5 pages", web, w, ca, Some(-7))
        },
        Seed {
            pinned: true,
            ..t("Visual design review with client", web, w, ca, Some(2))
        },
        t("CMS content migration", web, w, ca, Some(14)),
        Seed {
            description: "DNS, redirects, analytics, uptime monitor.",
            ..t("Launch checklist", web, w, ca, Some(20))
        },
        t("Dependency version sweep", aud, w, int, Some(-5)),
        Seed {
            completed: true,
            ..t("Review auth session expiry", aud, w, int, Some(-10))
        },
        t("Write findings report", aud, w, int, Some(4)),
        t("Get three contractor quotes", kit, p, hm, Some(7)),
        t("Pick countertop material", kit, p, hm, None),
        Seed {
            completed: true,
            ..t("Order cabinet hardware", kit, p, hm, Some(-2))
        },
        Seed {
            completed: true,
            ..t("Module 3: conditional types", ts, l, co, None)
        },
        Seed {
            pinned: true,
            ..t("Module 4: template literal types", ts, l, co, None)
        },
        t("Module 5: variance annotations", ts, l, co, None),
        t("Order seeds", gar, None, None, Some(10)),
        t("Clear the raised beds", gar, None, None, None),
        Seed {
            repeat: RepeatConfig::Daily,
            ..t("Water the plants", None, p, hm, Some(0))
        },
        Seed {
            repeat: RepeatConfig::Weekly,
            ..t("Weekly planning review", None, w, None, Some(1))
        },
        Seed {
            repeat: RepeatConfig::Monthly,
            ..t("Pay rent", None, p, None, Some(3))
        },
        Seed {
            repeat: RepeatConfig::Yearly,
            ..t("Renew domain names", None, w, None, Some(90))
        },
        Seed {
            repeat: RepeatConfig::Custom { interval_days: 14 },
            ..t("Backup laptop", None, None, None, Some(-1))
        },
        Seed {
            repeat: RepeatConfig::Movable { interval_days: 30 },
            ..t("Change HVAC filter", None, p, hm, None)
        },
        t("Dentist appointment", None, p, hl, Some(12)),
        Seed {
            repeat: RepeatConfig::Daily,
            completed: true,
            ..t("Morning run", None, p, hl, Some(0))
        },
        t("Expense report", None, w, None, Some(-4)),
        t(
            "Read \"Working Effectively with Legacy Code\"",
            None,
            l,
            None,
            None,
        ),
    ];
    let mut task_ids = Vec::new();
    for seed in &tasks {
        task_ids.push(
            s.add_task(TaskInput {
                title: seed.title.into(),
                description: seed.description.into(),
                due_date: seed.due.and_then(days_from_today),
                project_id: seed.project.map(Into::into),
                category_id: seed.cat.map(Into::into),
                subcategory_id: seed.sub.map(Into::into),
                repeat: seed.repeat,
            })
            .expect(ok),
        );
    }
    // Set flags directly, as the web seeder does (completing here would spawn repeats).
    for (seed, id) in tasks.iter().zip(&task_ids) {
        let task = s
            .data
            .tasks
            .iter_mut()
            .find(|t| &t.id == id)
            .expect("just added");
        task.completed = seed.completed;
        task.completed_at = seed.completed.then(now_iso);
        task.pinned = seed.pinned;
    }

    let work_notes = [
        "Sprint work",
        "Meetings all afternoon",
        "Code review + pairing",
        "Deep focus block",
        "Bug triage",
    ];
    let personal_notes = [
        "Evening session",
        "Weekend catch-up",
        "Quick pass before dinner",
    ];
    let learning_notes = [
        "One course module",
        "Practice exercises",
        "Notes + flashcards",
    ];
    // (title, description, category, weekday rate, weekend rate, notes)
    type Item<'a> = (&'a str, &'a str, Option<&'a str>, f64, f64, &'a [&'a str]);
    let items: [Item; 7] = [
        (
            "Client A development",
            "Implementation work on the redesign.",
            w,
            0.85,
            0.05,
            &work_notes,
        ),
        (
            "Internal tooling",
            "CI, scripts, and dev-experience fixes.",
            w,
            0.4,
            0.0,
            &work_notes,
        ),
        ("Admin & email", "", w, 0.6, 0.0, &work_notes),
        (
            "Exercise",
            "Runs, gym, and long walks.",
            p,
            0.5,
            0.7,
            &personal_notes,
        ),
        (
            "Kitchen renovation work",
            "Hands-on reno time.",
            p,
            0.15,
            0.8,
            &personal_notes,
        ),
        ("TypeScript course", "", l, 0.3, 0.4, &learning_notes),
        (
            "Podcasts & reading",
            "Uncategorized on purpose.",
            None,
            0.35,
            0.5,
            &learning_notes,
        ),
    ];
    for (title, description, cat, work_rate, weekend_rate, notes) in items {
        let id = s
            .add_time_entry(TimeEntryInput {
                title: title.into(),
                description: description.into(),
                category_id: cat.map(Into::into),
            })
            .expect(ok);
        let entry = s
            .data
            .time_entries
            .iter_mut()
            .find(|e| e.id == id)
            .expect("just added");
        for ago in 0..70 {
            let date = add_days(today(), -ago);
            let weekend = matches!(date.weekday(), Weekday::Sat | Weekday::Sun);
            if !rng.chance(if weekend { weekend_rate } else { work_rate }) {
                continue;
            }
            let iso = to_iso(date);
            entry
                .daily_minutes
                .insert(iso.clone(), rng.int(1, 12) * INCREMENT_MINUTES as u32);
            if rng.chance(0.35) {
                entry.daily_notes.insert(iso, rng.pick(notes).to_string());
            }
        }
    }

    s.data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_data_has_the_web_seeders_shape() {
        let d = demo_data();
        assert_eq!(d.categories.len(), 3);
        assert_eq!(d.subcategories.len(), 5);
        assert_eq!(d.projects.len(), 5);
        assert_eq!(d.tasks.len(), 26);
        assert_eq!(d.time_entries.len(), 7);
        assert!(d.time_entries.iter().all(|e| !e.daily_minutes.is_empty()));
        assert_eq!(
            demo_data().time_entries[0].daily_minutes,
            d.time_entries[0].daily_minutes
        );
    }
}
