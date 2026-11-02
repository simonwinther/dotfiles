use crate::{
    backend::Backend,
    protocol::{Activation, Item, State},
};
use anyhow::{Result, bail, ensure};
use chrono::{DateTime, Local, TimeZone};
use rusqlite::params;
use std::sync::atomic::{AtomicU64, Ordering};

static IDS: AtomicU64 = AtomicU64::new(0);

pub fn schedule(input: &str, now: DateTime<Local>) -> Result<(String, Option<i64>, bool)> {
    let urgent = input.trim_end().ends_with('!');
    let input = input.trim().trim_end_matches('!').trim_end();
    let (text, scheduled) = if let Some((date, text)) = input.split_once('>') {
        let date = date
            .trim()
            .trim_start_matches('+')
            .trim_start_matches("in ")
            .replace(" at ", " ");
        let date = chrono_english::parse_date_string(&date, now, chrono_english::Dialect::Uk)
            .map_err(|_| anyhow::anyhow!("Invalid scheduled date"))?;
        (text.trim(), Some(date.timestamp()))
    } else {
        (input, None)
    };
    ensure!(
        !text.is_empty() && text.len() <= 16 * 1024,
        "Task must contain 1–16384 bytes"
    );
    Ok((text.into(), scheduled, urgent))
}

pub fn query(backend: &Backend, query: &str, limit: usize) -> Result<Vec<Item>> {
    let store = backend.store.lock().unwrap();
    if store.setting("todo_creating")? == "true" {
        let (text, scheduled, urgent) =
            schedule(query, Local::now()).unwrap_or_else(|_| (query.into(), None, false));
        return Ok(vec![Item {
            identifier: "new".into(),
            text,
            subtext: scheduled
                .and_then(|time| Local.timestamp_opt(time, 0).single())
                .map(|date| date.format("%a %d %b %H:%M").to_string())
                .unwrap_or_else(|| "New task".into()),
            provider: "todo".into(),
            icon: "checkbox-symbolic".into(),
            state: if urgent {
                vec!["creating".into(), "urgent".into()]
            } else {
                vec!["creating".into()]
            },
            actions: vec!["save".into(), "save_next".into(), "search".into()],
            ..Default::default()
        }]);
    }
    let date = if matches!(query.trim(), "today" | "tomorrow" | "yesterday") {
        chrono_english::parse_date_string(query.trim(), Local::now(), chrono_english::Dialect::Uk)
            .ok()
            .map(|day| day.date_naive())
    } else {
        None
    };
    let words: Vec<_> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut statement=store.db.prepare("SELECT id,text,category,state,urgency,created,scheduled,started,finished FROM todos ORDER BY state='done',urgency='critical' DESC,COALESCE(scheduled,created),created DESC")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, Option<i64>>(6)?,
            row.get::<_, Option<i64>>(7)?,
            row.get::<_, Option<i64>>(8)?,
        ))
    })?;
    let mut items = vec![];
    for row in rows {
        let (id, text, category, state, urgency, created, scheduled, started, finished) = row?;
        if let Some(date) = date {
            if scheduled
                .and_then(|time| Local.timestamp_opt(time, 0).single())
                .is_none_or(|day| day.date_naive() != date)
            {
                continue;
            }
        } else {
            let haystack = format!("{text} {category} {state}").to_lowercase();
            if !words.iter().all(|word| haystack.contains(word)) {
                continue;
            }
        }
        let mut details = vec![];
        if !category.is_empty() {
            details.push(category);
        }
        if let Some(date) = Local
            .timestamp_opt(scheduled.unwrap_or(created), 0)
            .single()
        {
            details.push(date.format("%a %d %b %H:%M").to_string());
        }
        if let Some(started) = started {
            details.push(format!(
                "{} min",
                (finished.unwrap_or_else(|| Local::now().timestamp()) - started).max(0) / 60
            ));
        }
        let mut states = vec![state];
        if urgency == "critical" {
            states.push("urgent".into());
        }
        items.push(Item {
            identifier: id,
            text,
            subtext: details.join(" · "),
            icon: "checkbox-symbolic".into(),
            provider: "todo".into(),
            state: states,
            actions: ["done", "active", "inactive", "change_category", "delete"]
                .iter()
                .map(|value| (*value).into())
                .collect(),
            ..Default::default()
        });
        if items.len() >= limit {
            break;
        }
    }
    Ok(items)
}

pub fn activate(backend: &Backend, action: &Activation) -> Result<()> {
    let store = backend.store.lock().unwrap();
    let now = Local::now();
    match action.action.as_str() {
        "create" | "search" => store.set_setting(
            "todo_creating",
            if action.action == "create" {
                "true"
            } else {
                "false"
            },
        )?,
        "save" | "save_next" => {
            let (text, scheduled, urgent) = schedule(&action.query, now)?;
            let id = format!(
                "{}-{}",
                now.timestamp_micros(),
                IDS.fetch_add(1, Ordering::Relaxed)
            );
            store.db.execute("INSERT INTO todos(id,text,state,urgency,created,scheduled) VALUES(?1,?2,'pending',?3,?4,?5)",params![id,text,if urgent {"critical"}else{"normal"},now.timestamp(),scheduled])?;
            store.set_setting(
                "todo_creating",
                if action.action == "save_next" {
                    "true"
                } else {
                    "false"
                },
            )?;
        }
        "clear" => {
            store
                .db
                .execute("DELETE FROM todos WHERE state='done'", [])?;
        }
        "delete" => {
            ensure!(
                store
                    .db
                    .execute("DELETE FROM todos WHERE id=?1", [&action.identifier])?
                    == 1,
                "Task no longer exists"
            );
        }
        "active" => {
            ensure!(
                store.db.execute(
                    "UPDATE todos SET state='active',started=?2,finished=NULL WHERE id=?1",
                    params![action.identifier, now.timestamp()]
                )? == 1,
                "Task no longer exists"
            );
        }
        "inactive" => {
            ensure!(
                store.db.execute(
                    "UPDATE todos SET state='pending',started=NULL,finished=NULL WHERE id=?1",
                    [&action.identifier]
                )? == 1,
                "Task no longer exists"
            );
        }
        "done" => {
            ensure!(store.db.execute("UPDATE todos SET state=CASE WHEN state='done' THEN 'pending' ELSE 'done' END,finished=CASE WHEN state='done' THEN NULL ELSE ?2 END WHERE id=?1",params![action.identifier,now.timestamp()])?==1,"Task no longer exists");
        }
        "change_category" => {
            let category: String = store.db.query_row(
                "SELECT category FROM todos WHERE id=?1",
                [&action.identifier],
                |row| row.get(0),
            )?;
            let categories = &backend.config.todo_categories;
            let next = if category.is_empty() {
                categories.first()
            } else {
                categories
                    .iter()
                    .position(|value| value == &category)
                    .and_then(|index| categories.get(index + 1))
            };
            store.db.execute(
                "UPDATE todos SET category=?2 WHERE id=?1",
                params![action.identifier, next.map(String::as_str).unwrap_or("")],
            )?;
        }
        _ => bail!("Unsupported task action"),
    }
    drop(store);
    backend.events.notify("todo");
    Ok(())
}

pub fn state(backend: &Backend) -> Result<State> {
    let store = backend.store.lock().unwrap();
    let creating = store.setting("todo_creating")? == "true";
    Ok(State {
        provider: "todo".into(),
        states: vec![if creating { "creating" } else { "searching" }.into()],
        actions: if creating {
            vec!["search".into()]
        } else {
            vec!["create".into(), "clear".into()]
        },
    })
}

pub fn import(backend: &Backend, path: &std::path::Path) -> Result<usize> {
    if !path.is_file() {
        return Ok(0);
    }
    let mut store = backend.store.lock().unwrap();
    if store.setting("todo_imported")? == "true" {
        return Ok(0);
    }
    ensure!(
        path.metadata()?.len() <= 32 * 1024 * 1024,
        "Legacy task file is too large"
    );
    let mut reader = csv::ReaderBuilder::new().delimiter(b';').from_path(path)?;
    let transaction = store.db.transaction()?;
    let mut count = 0;
    for row in reader.records() {
        let row = row?;
        ensure!(row.len() == 9, "Invalid legacy task row");
        let date = |index| {
            DateTime::parse_from_rfc2822(&row[index])
                .ok()
                .filter(|date| date.timestamp() > 0)
                .map(|date| date.timestamp())
        };
        transaction.execute("INSERT OR IGNORE INTO todos(id,category,text,state,urgency,notified,scheduled,started,finished,created) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![format!("import-{count}"),&row[0],&row[1],&row[2],&row[3],row[4]==*"true",date(5),date(6),date(7),date(8).unwrap_or(0)])?;
        count += 1;
    }
    transaction.execute("INSERT INTO settings(key,value) VALUES('todo_imported','true') ON CONFLICT(key) DO UPDATE SET value='true'",[])?;
    transaction.commit()?;
    Ok(count)
}

pub fn due(backend: &Backend) -> Result<Vec<(String, String, String)>> {
    let store = backend.store.lock().unwrap();
    let mut statement = store.db.prepare(
        "SELECT id,text,urgency FROM todos WHERE scheduled<=?1 AND notified=0 AND state!='done'",
    )?;
    let values = statement
        .query_map([Local::now().timestamp()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scheduled_tasks_preserve_text_urgency_and_relative_time() {
        let now = Local.with_ymd_and_hms(2026, 11, 2, 10, 0, 0).unwrap();
        let (text, time, urgent) = schedule("in 10m > Literal $HOME; task !", now).unwrap();
        assert_eq!(text, "Literal $HOME; task");
        assert!(urgent);
        assert_eq!(time, Some(now.timestamp() + 600));
        assert!(schedule("broken date > task", now).is_err());
        assert!(schedule("", now).is_err());
    }
}
