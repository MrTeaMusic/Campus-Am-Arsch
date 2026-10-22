//! Gemeinsamer Kalender: Einträge, die alle App-Nutzer sehen und anlegen können.
//! Hier liegt auch die Datenbank des Schwarzen Bretts (`board.rs`).
//! Gespeichert in einer SQLite-Datei (`SHARED_DB_PATH`, Standard `shared_calendar.db` im
//! Arbeitsverzeichnis, bei blitz.cloud `/app/data`).

use std::sync::{Mutex, OnceLock};

use axum::{Extension, Json, extract::Query};
use chrono::NaiveDateTime;
use http::StatusCode;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::types::{CdAuthData, ResponseError};

const TIME_FORMAT: &str = "%Y-%m-%dT%H:%M";
const MAX_ENTRIES_PER_USER: i64 = 50;

static DB: OnceLock<Mutex<Connection>> = OnceLock::new();

pub fn db() -> Result<std::sync::MutexGuard<'static, Connection>, ResponseError> {
    let conn = DB.get_or_init(|| {
        let path = std::env::var("SHARED_DB_PATH").unwrap_or("shared_calendar.db".into());
        let conn = Connection::open(&path).expect("Unable to open the shared calendar database");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS shared_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                description TEXT NOT NULL,
                start TEXT NOT NULL,
                end TEXT NOT NULL,
                author TEXT NOT NULL,
                owner TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS board_posts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                text TEXT NOT NULL,
                author TEXT NOT NULL,
                owner TEXT NOT NULL,
                created_at TEXT NOT NULL
            );",
        )
        .expect("Unable to create the shared calendar table");
        log::info!("Shared calendar database: {path}");
        Mutex::new(conn)
    });
    conn.lock().map_err(|_| internal("database lock poisoned"))
}

pub fn internal(e: impl std::fmt::Display) -> ResponseError {
    log::error!("Shared calendar: {e}");
    ResponseError {
        message: "Gemeinsamer Kalender nicht verfügbar".to_string(),
        status_code: StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub fn bad_request(message: &str) -> ResponseError {
    ResponseError {
        message: message.to_string(),
        status_code: StatusCode::BAD_REQUEST,
    }
}

#[derive(Serialize)]
pub struct SharedEvent {
    id: i64,
    title: String,
    description: String,
    start: String,
    end: String,
    author: String,
    /// true, wenn der Eintrag vom anfragenden Nutzer stammt (nur dann änderbar)
    mine: bool,
}

#[derive(Deserialize)]
pub struct EventInput {
    title: String,
    #[serde(default)]
    description: String,
    start: String,
    end: String,
    #[serde(default)]
    author: String,
}

#[derive(Deserialize)]
pub struct UpdateInput {
    id: i64,
    #[serde(flatten)]
    event: EventInput,
}

#[derive(Deserialize)]
pub struct DeleteInput {
    id: i64,
}

#[derive(Deserialize)]
pub struct ListQuery {
    /// nur Einträge, die nach diesem Zeitpunkt enden (`YYYY-MM-DDTHH:MM`)
    from: Option<String>,
}

struct Valid {
    title: String,
    description: String,
    start: String,
    end: String,
    author: String,
}

fn validate(input: EventInput) -> Result<Valid, ResponseError> {
    let title = input.title.trim().to_string();
    if title.is_empty() || title.chars().count() > 100 {
        return Err(bad_request("Titel: 1 bis 100 Zeichen"));
    }
    let description = input.description.trim().to_string();
    if description.chars().count() > 500 {
        return Err(bad_request("Beschreibung: höchstens 500 Zeichen"));
    }
    let start = NaiveDateTime::parse_from_str(&input.start, TIME_FORMAT)
        .map_err(|_| bad_request("Ungültiger Beginn"))?;
    let end = NaiveDateTime::parse_from_str(&input.end, TIME_FORMAT)
        .map_err(|_| bad_request("Ungültiges Ende"))?;
    if end < start {
        return Err(bad_request("Das Ende liegt vor dem Beginn"));
    }
    // frei wählbarer Anzeigename, nicht geprüft: bewusst kein Vertrauen für Rechte (siehe `owner`)
    let author: String = input.author.trim().chars().take(40).collect();
    Ok(Valid {
        title,
        description,
        start: start.format(TIME_FORMAT).to_string(),
        end: end.format(TIME_FORMAT).to_string(),
        author,
    })
}

pub async fn list_events(
    Extension(auth): Extension<CdAuthData>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<SharedEvent>>, ResponseError> {
    let from = query.from.unwrap_or_default();
    let conn = db()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, title, description, start, end, author, owner FROM shared_events
             WHERE end >= ?1 ORDER BY start, id",
        )
        .map_err(internal)?;
    let events = stmt
        .query_map(params![from], |row| {
            let owner: String = row.get(6)?;
            Ok(SharedEvent {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                start: row.get(3)?,
                end: row.get(4)?,
                author: row.get(5)?,
                mine: owner == auth.user,
            })
        })
        .map_err(internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;
    Ok(Json(events))
}

pub async fn add_event(
    Extension(auth): Extension<CdAuthData>,
    Json(input): Json<EventInput>,
) -> Result<Json<i64>, ResponseError> {
    let event = validate(input)?;
    let conn = db()?;
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM shared_events WHERE owner = ?1",
            params![auth.user],
            |row| row.get(0),
        )
        .map_err(internal)?;
    if count >= MAX_ENTRIES_PER_USER {
        return Err(bad_request("Du hast schon zu viele Einträge, lösche zuerst alte"));
    }
    conn.execute(
        "INSERT INTO shared_events (title, description, start, end, author, owner)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            event.title,
            event.description,
            event.start,
            event.end,
            event.author,
            auth.user
        ],
    )
    .map_err(internal)?;
    Ok(Json(conn.last_insert_rowid()))
}

pub async fn update_event(
    Extension(auth): Extension<CdAuthData>,
    Json(input): Json<UpdateInput>,
) -> Result<StatusCode, ResponseError> {
    let event = validate(input.event)?;
    let changed = db()?
        .execute(
            "UPDATE shared_events SET title = ?1, description = ?2, start = ?3, end = ?4, author = ?5
             WHERE id = ?6 AND owner = ?7",
            params![
                event.title,
                event.description,
                event.start,
                event.end,
                event.author,
                input.id,
                auth.user
            ],
        )
        .map_err(internal)?;
    not_found_unless(changed)
}

pub async fn delete_event(
    Extension(auth): Extension<CdAuthData>,
    Json(input): Json<DeleteInput>,
) -> Result<StatusCode, ResponseError> {
    let changed = db()?
        .execute(
            "DELETE FROM shared_events WHERE id = ?1 AND owner = ?2",
            params![input.id, auth.user],
        )
        .map_err(internal)?;
    not_found_unless(changed)
}

/// Fremde Einträge verhalten sich wie nicht vorhandene
pub fn not_found_unless(changed: usize) -> Result<StatusCode, ResponseError> {
    if changed == 0 {
        Err(ResponseError {
            message: "Eintrag nicht gefunden oder nicht von dir".to_string(),
            status_code: StatusCode::NOT_FOUND,
        })
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth(user: &str) -> Extension<CdAuthData> {
        Extension(CdAuthData {
            cookie: String::new(),
            hash: String::new(),
            user: user.to_string(),
            password: String::new(),
        })
    }

    fn input(title: &str, start: &str, end: &str) -> Json<EventInput> {
        Json(EventInput {
            title: title.into(),
            description: String::new(),
            start: start.into(),
            end: end.into(),
            author: "Ben".into(),
        })
    }

    #[tokio::test]
    async fn only_the_owner_changes_entries() {
        unsafe { std::env::set_var("SHARED_DB_PATH", ":memory:") };

        let id = add_event(auth("a"), input("Lerngruppe", "2030-01-01T10:00", "2030-01-01T12:00"))
            .await
            .ok()
            .unwrap()
            .0;
        assert!(
            add_event(auth("a"), input("Falsch", "2030-01-02T10:00", "2030-01-01T10:00"))
                .await
                .is_err()
        );

        let query = || Query(ListQuery { from: Some("2029-12-31T00:00".into()) });
        let seen_by_b = list_events(auth("b"), query()).await.ok().unwrap().0;
        assert_eq!(seen_by_b.len(), 1);
        assert!(!seen_by_b[0].mine);
        assert!(list_events(auth("a"), query()).await.ok().unwrap().0[0].mine);

        assert!(delete_event(auth("b"), Json(DeleteInput { id })).await.is_err());
        assert!(delete_event(auth("a"), Json(DeleteInput { id })).await.is_ok());
        assert!(list_events(auth("b"), query()).await.ok().unwrap().0.is_empty());
    }
}
