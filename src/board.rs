//! Schwarzes Brett: kurze Nachrichten, die alle App-Nutzer lesen und schreiben können.
//! Liegt in derselben SQLite-Datei wie der gemeinsame Kalender (`shared_calendar::db`).

use axum::{Extension, Json};
use chrono::Utc;
use http::StatusCode;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::{
    shared_calendar::{bad_request, db, internal, not_found_unless},
    types::{CdAuthData, ResponseError},
};

const MAX_POSTS_PER_USER: i64 = 20;
/// Auf dem Brett bleiben nur die neuesten Nachrichten, ältere werden beim Schreiben gelöscht
const MAX_POSTS_TOTAL: i64 = 200;
const LIST_LIMIT: i64 = 100;

#[derive(Serialize)]
pub struct BoardPost {
    id: i64,
    text: String,
    author: String,
    /// UTC, RFC 3339
    created_at: String,
    /// true, wenn die Nachricht vom anfragenden Nutzer stammt (nur dann löschbar)
    mine: bool,
}

#[derive(Deserialize)]
pub struct PostInput {
    text: String,
    #[serde(default)]
    author: String,
}

#[derive(Deserialize)]
pub struct DeleteInput {
    id: i64,
}

pub async fn list_posts(
    Extension(auth): Extension<CdAuthData>,
) -> Result<Json<Vec<BoardPost>>, ResponseError> {
    let conn = db()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, text, author, created_at, owner FROM board_posts
             ORDER BY id DESC LIMIT ?1",
        )
        .map_err(internal)?;
    let posts = stmt
        .query_map(params![LIST_LIMIT], |row| {
            let owner: String = row.get(4)?;
            Ok(BoardPost {
                id: row.get(0)?,
                text: row.get(1)?,
                author: row.get(2)?,
                created_at: row.get(3)?,
                mine: owner == auth.user,
            })
        })
        .map_err(internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;
    Ok(Json(posts))
}

pub async fn add_post(
    Extension(auth): Extension<CdAuthData>,
    Json(input): Json<PostInput>,
) -> Result<Json<i64>, ResponseError> {
    let text = input.text.trim().to_string();
    if text.is_empty() || text.chars().count() > 500 {
        return Err(bad_request("Nachricht: 1 bis 500 Zeichen"));
    }
    // frei wählbarer Anzeigename, nicht geprüft: bewusst kein Vertrauen für Rechte (siehe `owner`)
    let author: String = input.author.trim().chars().take(40).collect();

    let conn = db()?;
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM board_posts WHERE owner = ?1",
            params![auth.user],
            |row| row.get(0),
        )
        .map_err(internal)?;
    if count >= MAX_POSTS_PER_USER {
        return Err(bad_request(
            "Du hast schon zu viele Nachrichten, lösche zuerst alte",
        ));
    }
    conn.execute(
        "INSERT INTO board_posts (text, author, owner, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![text, author, auth.user, Utc::now().to_rfc3339()],
    )
    .map_err(internal)?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "DELETE FROM board_posts WHERE id NOT IN
         (SELECT id FROM board_posts ORDER BY id DESC LIMIT ?1)",
        params![MAX_POSTS_TOTAL],
    )
    .map_err(internal)?;
    Ok(Json(id))
}

pub async fn delete_post(
    Extension(auth): Extension<CdAuthData>,
    Json(input): Json<DeleteInput>,
) -> Result<StatusCode, ResponseError> {
    let changed = db()?
        .execute(
            "DELETE FROM board_posts WHERE id = ?1 AND owner = ?2",
            params![input.id, auth.user],
        )
        .map_err(internal)?;
    not_found_unless(changed)
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

    fn post(text: &str) -> Json<PostInput> {
        Json(PostInput {
            text: text.into(),
            author: "Ben".into(),
        })
    }

    #[tokio::test]
    async fn everyone_reads_and_only_the_owner_deletes() {
        unsafe { std::env::set_var("SHARED_DB_PATH", ":memory:") };

        let id = add_post(auth("a"), post("Klausur verschoben")).await.ok().unwrap().0;
        assert!(add_post(auth("a"), post("   ")).await.is_err());

        let seen_by_b = list_posts(auth("b")).await.ok().unwrap().0;
        assert_eq!(seen_by_b.len(), 1);
        assert!(!seen_by_b[0].mine);

        assert!(delete_post(auth("b"), Json(DeleteInput { id })).await.is_err());
        assert!(delete_post(auth("a"), Json(DeleteInput { id })).await.is_ok());
        assert!(list_posts(auth("b")).await.ok().unwrap().0.is_empty());
    }
}
