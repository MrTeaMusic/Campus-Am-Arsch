//! Push-Benachrichtigungen (Web Push) für den gemeinsamen Kalender: einen Tag vor Beginn eines
//! Eintrags bekommen alle angemeldeten Geräte eine Erinnerung.
//!
//! Die VAPID-Schlüssel werden beim ersten Start erzeugt und in der SQLite-Datei abgelegt, es ist
//! keine Einstellung nötig. Geht die Datei verloren, müssen sich die Geräte neu anmelden.

use std::{sync::OnceLock, time::Duration};

use axum::{Extension, Json};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use chrono::{NaiveDateTime, TimeZone, Utc};
use chrono_tz::Europe::Berlin;
use http::StatusCode;
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use web_push_native::{
    Auth, WebPushBuilder,
    jwt_simple::algorithms::{ECDSAP256PublicKeyLike, ES256KeyPair},
    p256::PublicKey,
};

use crate::{
    shared_calendar::{bad_request, db, internal, not_found_unless},
    types::{CdAuthData, ResponseError},
};

const CHECK_INTERVAL: Duration = Duration::from_secs(60);
const LEAD_TIME: chrono::Duration = chrono::Duration::hours(24);
const MAX_SUBSCRIPTIONS_PER_USER: i64 = 10;

static VAPID: OnceLock<ES256KeyPair> = OnceLock::new();

fn vapid() -> Result<&'static ES256KeyPair, ResponseError> {
    if let Some(key) = VAPID.get() {
        return Ok(key);
    }
    let conn = db()?;
    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM kv WHERE key = 'vapid_private'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(internal)?;
    let key = match stored
        .and_then(|s| B64.decode(s).ok())
        .and_then(|b| ES256KeyPair::from_bytes(&b).ok())
    {
        Some(key) => key,
        None => {
            let key = ES256KeyPair::generate();
            conn.execute(
                "INSERT OR REPLACE INTO kv (key, value) VALUES ('vapid_private', ?1)",
                params![B64.encode(key.to_bytes())],
            )
            .map_err(internal)?;
            log::info!("Generated new VAPID keys for push notifications");
            key
        }
    };
    Ok(VAPID.get_or_init(|| key))
}

/// Öffentlicher Schlüssel für `pushManager.subscribe` im Browser
pub async fn get_key() -> Result<String, ResponseError> {
    Ok(B64.encode(vapid()?.public_key().public_key().to_bytes_uncompressed()))
}

#[derive(Deserialize)]
pub struct SubscribeInput {
    endpoint: String,
    p256dh: String,
    auth: String,
}

#[derive(Deserialize)]
pub struct UnsubscribeInput {
    endpoint: String,
}

pub async fn subscribe(
    Extension(auth): Extension<CdAuthData>,
    Json(input): Json<SubscribeInput>,
) -> Result<StatusCode, ResponseError> {
    // reject what could not be sent to later
    if !input.endpoint.starts_with("https://") || input.endpoint.len() > 2000 {
        return Err(bad_request("Ungültige Push-Adresse"));
    }
    if parse_subscription(&input.endpoint, &input.p256dh, &input.auth).is_none() {
        return Err(bad_request("Ungültige Push-Schlüssel"));
    }

    let conn = db()?;
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM push_subscriptions WHERE owner = ?1 AND endpoint != ?2",
            params![auth.user, input.endpoint],
            |row| row.get(0),
        )
        .map_err(internal)?;
    if count >= MAX_SUBSCRIPTIONS_PER_USER {
        return Err(bad_request("Zu viele Geräte angemeldet"));
    }
    conn.execute(
        "INSERT OR REPLACE INTO push_subscriptions (endpoint, p256dh, auth, owner)
         VALUES (?1, ?2, ?3, ?4)",
        params![input.endpoint, input.p256dh, input.auth, auth.user],
    )
    .map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unsubscribe(
    Extension(auth): Extension<CdAuthData>,
    Json(input): Json<UnsubscribeInput>,
) -> Result<StatusCode, ResponseError> {
    let changed = db()?
        .execute(
            "DELETE FROM push_subscriptions WHERE endpoint = ?1 AND owner = ?2",
            params![input.endpoint, auth.user],
        )
        .map_err(internal)?;
    not_found_unless(changed)
}

fn parse_subscription(endpoint: &str, p256dh: &str, auth: &str) -> Option<WebPushBuilder> {
    let public = PublicKey::from_sec1_bytes(&B64.decode(p256dh).ok()?).ok()?;
    let auth = B64.decode(auth).ok()?;
    if auth.len() != 16 {
        return None;
    }
    Some(WebPushBuilder::new(
        endpoint.parse().ok()?,
        public,
        Auth::clone_from_slice(&auth),
    ))
}

struct Due {
    id: i64,
    title: String,
    start: NaiveDateTime,
}

/// Einträge, die in höchstens 24 Stunden beginnen und noch keine Erinnerung hatten
fn due_events() -> Result<Vec<Due>, ResponseError> {
    let now = Utc::now();
    let conn = db()?;
    // start is local time (Europe/Berlin), the window is made generous and filtered precisely below
    let earliest = (now.with_timezone(&Berlin).naive_local() - chrono::Duration::hours(1))
        .format("%Y-%m-%dT%H:%M")
        .to_string();
    let mut stmt = conn
        .prepare(
            "SELECT id, title, start FROM shared_events
             WHERE start >= ?1 AND id NOT IN (SELECT event_id FROM push_sent)",
        )
        .map_err(internal)?;
    let rows = stmt
        .query_map(params![earliest], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;

    Ok(rows
        .into_iter()
        .filter_map(|(id, title, start)| {
            let start = NaiveDateTime::parse_from_str(&start, "%Y-%m-%dT%H:%M").ok()?;
            let start_utc = Berlin
                .from_local_datetime(&start)
                .earliest()?
                .with_timezone(&Utc);
            (start_utc - LEAD_TIME <= now && now < start_utc).then_some(Due { id, title, start })
        })
        .collect())
}

fn subscriptions() -> Result<Vec<(String, String, String)>, ResponseError> {
    let conn = db()?;
    let mut stmt = conn
        .prepare("SELECT endpoint, p256dh, auth FROM push_subscriptions")
        .map_err(internal)?;
    stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)
}

async fn send(
    client: &reqwest::Client,
    (endpoint, p256dh, auth): &(String, String, String),
    payload: &str,
) {
    let Some(builder) = parse_subscription(endpoint, p256dh, auth) else {
        forget(endpoint);
        return;
    };
    let Ok(key) = vapid() else { return };
    let builder = builder.with_vapid(key, "https://github.com/MrTeaMusic/Campus-Am-Arsch");
    let request = match builder
        .build(payload)
        .map_err(|e| e.to_string())
        .and_then(|r| reqwest::Request::try_from(r).map_err(|e| e.to_string()))
    {
        Ok(request) => request,
        Err(e) => {
            log::warn!("Push: could not build the request: {e}");
            return;
        }
    };
    match client.execute(request).await {
        Ok(res) if res.status().is_success() => {}
        // the device removed the subscription (app uninstalled, permission revoked)
        Ok(res) if matches!(res.status().as_u16(), 404 | 410) => forget(endpoint),
        Ok(res) => log::warn!("Push: service answered {}", res.status()),
        Err(e) => log::warn!("Push: sending failed: {e}"),
    }
}

fn forget(endpoint: &str) {
    if let Ok(conn) = db() {
        let _ = conn.execute(
            "DELETE FROM push_subscriptions WHERE endpoint = ?1",
            params![endpoint],
        );
    }
}

async fn notify_due(client: &reqwest::Client) -> Result<(), ResponseError> {
    for event in due_events()? {
        // marked first: a failing device must not cause a message every minute
        db()?
            .execute(
                "INSERT OR IGNORE INTO push_sent (event_id) VALUES (?1)",
                params![event.id],
            )
            .map_err(internal)?;
        let payload = serde_json::json!({
            "title": format!("Morgen: {}", event.title),
            "body": format!("Beginn {} Uhr", event.start.format("%d.%m. um %H:%M")),
            "url": "/dashboard",
        })
        .to_string();
        for subscription in subscriptions()? {
            send(client, &subscription, &payload).await;
        }
    }
    Ok(())
}

pub async fn run_scheduler() {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Unable to build the push client");
    loop {
        if let Err(e) = notify_due(&client).await {
            log::error!("Push scheduler: {}", e.message);
        }
        tokio::time::sleep(CHECK_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert_event(title: &str, hours_from_now: i64) -> i64 {
        let start = (Utc::now().with_timezone(&Berlin).naive_local()
            + chrono::Duration::hours(hours_from_now))
        .format("%Y-%m-%dT%H:%M")
        .to_string();
        let conn = db().ok().unwrap();
        conn.execute(
            "INSERT INTO shared_events (title, description, start, end, author, owner)
             VALUES (?1, '', ?2, ?2, '', 'x')",
            params![title, start],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[tokio::test]
    async fn reminds_one_day_before_and_only_once() {
        unsafe { std::env::set_var("SHARED_DB_PATH", ":memory:") };

        let soon = insert_event("Morgen", 20);
        let far = insert_event("Spaeter", 72);
        let past = insert_event("Vorbei", -3);

        let due: Vec<i64> = due_events().ok().unwrap().iter().map(|d| d.id).collect();
        assert!(due.contains(&soon));
        assert!(!due.contains(&far));
        assert!(!due.contains(&past));

        db().ok()
            .unwrap()
            .execute(
                "INSERT INTO push_sent (event_id) VALUES (?1)",
                params![soon],
            )
            .unwrap();
        assert!(!due_events().ok().unwrap().iter().any(|d| d.id == soon));
    }

    #[tokio::test]
    async fn key_is_stable_and_subscriptions_are_checked() {
        unsafe { std::env::set_var("SHARED_DB_PATH", ":memory:") };

        let key = get_key().await.ok().unwrap();
        assert_eq!(key.len(), 87); // 65 bytes, base64url without padding
        assert_eq!(get_key().await.ok().unwrap(), key);

        // a real client key pair, as `pushManager.subscribe` would deliver it
        let device = web_push_native::p256::SecretKey::random(
            &mut web_push_native::p256::elliptic_curve::rand_core::OsRng,
        );
        let p256dh = B64.encode(
            web_push_native::p256::elliptic_curve::sec1::ToEncodedPoint::to_encoded_point(
                &device.public_key(),
                false,
            ),
        );
        let auth = B64.encode([7u8; 16]);
        assert!(parse_subscription("https://push.example/abc", &p256dh, &auth).is_some());
        assert!(parse_subscription("https://push.example/abc", &p256dh, "kurz").is_none());
        assert!(parse_subscription("https://push.example/abc", "kaputt", &auth).is_none());
    }
}
