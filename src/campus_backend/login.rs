use anyhow::{Context, Result, bail};
use std::{fmt, sync::Arc};

use reqwest::{Client, Url};
use reqwest_cookie_store::{CookieStore, CookieStoreMutex};
use scraper::{Html, Selector};

use crate::{
    constants::CD_CERT_PEM,
    types::{CampusLoginData, CdAuthData, UserBasicInfo},
};

pub const CD_BASE: &str = "https://fep.campus-dual.de";

/// The IdP accepted the request but did not issue a SAML response (wrong user/password)
#[derive(Debug)]
pub struct InvalidCredentials(pub String);

impl fmt::Display for InvalidCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid credentials (IdP says: {})", self.0)
    }
}

impl std::error::Error for InvalidCredentials {}

pub fn build_login_client(cookie_store: Arc<CookieStoreMutex>) -> Result<Client> {
    Ok(Client::builder()
        .add_root_certificate(CD_CERT_PEM.get().unwrap().clone())
        .cookie_provider(cookie_store)
        .build()?)
}

/// The old CampusDual logged in with the bare number (`5002851`), the IdP wants `s5002851`
fn normalize_username(raw: &str) -> String {
    let user = raw.trim();
    if !user.is_empty() && user.chars().all(|c| c.is_ascii_digit()) {
        format!("s{user}")
    } else {
        user.to_string()
    }
}

pub async fn cdlogin_get_jcookie_and_meta(
    login_data: CampusLoginData,
) -> Result<(CdAuthData, UserBasicInfo)> {
    let display_user = login_data.username.trim().trim_start_matches(['s', 'S']).to_string();
    let login_data = CampusLoginData {
        username: normalize_username(&login_data.username),
        password: login_data.password,
    };
    let cookie_store = Arc::new(CookieStoreMutex::new(CookieStore::new()));
    let client = build_login_client(cookie_store.clone())?;

    saml_login(&client, &login_data).await?;

    let user_basic_info = get_userinfo(&client, &display_user).await?;

    let cd_auth_data = CdAuthData {
        cookie: serialize_cookie_store(&cookie_store)?,
        hash: String::new(),
        user: login_data.username,
        password: login_data.password,
    };

    Ok((cd_auth_data, user_basic_info))
}

/// SP-initiated SAML flow: campus-dual -> idp.dhsn.de (credentials) -> campus-dual ACS -> portal
async fn saml_login(client: &Client, login_data: &CampusLoginData) -> Result<()> {
    // 1: portal redirects to the IdP login page
    let resp = client
        .get(format!("{CD_BASE}/portal"))
        .send()
        .await?
        .error_for_status()?;
    let idp_url = resp.url().clone();
    let html = resp.text().await?;

    let (action, mut fields) = {
        let doc = Html::parse_document(&html);
        parse_form(&doc, &idp_url, r#"form[action$="/login"]"#)
            .context("CD login stage 1: IdP login form missing")?
    };
    set_field(&mut fields, "username", &login_data.username);
    set_field(&mut fields, "password", &login_data.password);

    // 2: submit credentials. The IdP redirects until it serves the auto-submit SAML form
    let resp = client
        .post(action)
        .form(&fields)
        .send()
        .await?
        .error_for_status()?;
    let url = resp.url().clone();
    let html = resp.text().await?;

    let (acs_url, acs_fields) = {
        let doc = Html::parse_document(&html);
        match parse_form(&doc, &url, "form")
            .filter(|(_, f)| f.iter().any(|(k, _)| k == "SAMLResponse"))
        {
            Some(form) => form,
            None => {
                let msg = Selector::parse(".invalid-feedback, .alert")
                    .ok()
                    .and_then(|sel| doc.select(&sel).next())
                    .map(|e| e.text().collect::<String>().trim().to_string())
                    .unwrap_or_else(|| format!("no SAML form at {url}"));
                return Err(InvalidCredentials(msg).into());
            }
        }
    };

    // 3: hand the assertion to the SAP service provider
    let resp = client
        .post(acs_url)
        .form(&acs_fields)
        .send()
        .await?
        .error_for_status()?;
    let url = resp.url().clone();
    let html = resp.text().await?;

    // 4: SAP answers with another auto-submit form targeting /portal
    let portal = {
        let doc = Html::parse_document(&html);
        parse_form(&doc, &url, "form")
    };
    if let Some((portal_url, portal_fields)) = portal {
        client
            .post(portal_url)
            .form(&portal_fields)
            .send()
            .await?
            .error_for_status()?;
    }

    log::info!("campus_login success");
    Ok(())
}

fn set_field(fields: &mut Vec<(String, String)>, name: &str, value: &str) {
    match fields.iter_mut().find(|(k, _)| k == name) {
        Some((_, v)) => *v = value.to_string(),
        None => fields.push((name.to_string(), value.to_string())),
    }
}

/// Returns the absolute form action and all named input values
fn parse_form(doc: &Html, base: &Url, selector: &str) -> Option<(Url, Vec<(String, String)>)> {
    let form = doc.select(&Selector::parse(selector).ok()?).next()?;
    let action = base.join(form.value().attr("action")?).ok()?;
    let input_sel = Selector::parse("input").unwrap();
    let fields = form
        .select(&input_sel)
        .filter_map(|i| {
            let name = i.value().attr("name")?;
            Some((name.to_string(), i.value().attr("value").unwrap_or("").to_string()))
        })
        .collect();
    Some((action, fields))
}

/// Serializes only the cookies the portal needs for later requests. The IdP cookies are only
/// required during login, and the JWT travels in a browser cookie, which is capped at 4 KiB
pub fn serialize_cookie_store(store: &CookieStoreMutex) -> Result<String> {
    let store = store.lock().unwrap();
    let mut buf = Vec::new();
    cookie_store::serde::json::save_incl_expired_and_nonpersistent(&store, &mut buf)
        .map_err(|e| anyhow::anyhow!("cookie store serialisation failed: {e}"))?;

    // the store is serialised as a JSON array of cookies
    let mut cookies: Vec<serde_json::Value> = serde_json::from_slice(&buf)?;
    // keep the SAP session cookies, drop the one-off SAML relay state cookie (~1 KB)
    cookies.retain(|c| {
        let name = c["raw_cookie"]
            .as_str()
            .and_then(|raw| raw.split('=').next())
            .unwrap_or_default()
            .to_ascii_lowercase();
        c["domain"].to_string().contains("campus-dual.de")
            && (name.starts_with("sap") || name.starts_with("mysap"))
    });
    Ok(serde_json::to_string(&cookies)?)
}

pub fn deserialize_cookie_store(json: &str) -> Result<CookieStore> {
    cookie_store::serde::json::load_all(json.as_bytes())
        .map_err(|e| anyhow::anyhow!("cookie store deserialisation failed: {e}"))
}

async fn get_userinfo(client: &Client, username: &str) -> Result<UserBasicInfo> {
    let resp = client
        .get(format!("{CD_BASE}/sap/opu/odata/sap/ZPIQ_COURSE_RESULTS_SRV/ProgramOfStudySet"))
        .header("Accept", "application/json")
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("CD user info: HTTP {}", resp.status());
    }
    // TODO: names/seminar group still need a source in the new CampusDual
    Ok(UserBasicInfo {
        user: username.to_string(),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::normalize_username;

    #[test]
    fn username_gets_s_prefix_only_for_bare_numbers() {
        assert_eq!(normalize_username("5002851"), "s5002851");
        assert_eq!(normalize_username(" 5002851 "), "s5002851");
        assert_eq!(normalize_username("s5002851"), "s5002851");
        assert_eq!(normalize_username("S5002851"), "S5002851");
        assert_eq!(normalize_username(""), "");
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::campus_backend::req_client_funcs::get_client_with_cd_cookie;
    use serde_json::Value;

    fn init_cert() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            // SAFETY: set before any other thread reads the environment in these tests
            unsafe {
                std::env::set_var("JWT_SECRET", "testtesttesttest");
                std::env::set_var("AES_KEY", "0123456789abcdef0123456789abcdef");
            }
            crate::constants::set_statics_from_env();
        });
        let pem = include_bytes!("../GEANT TLS RSA 1.pem");
        let _ = CD_CERT_PEM.set(reqwest::Certificate::from_pem(pem).unwrap());
    }

    /// CD_USER=... CD_PASS=... cargo test live_login -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn live_login() {
        init_cert();
        let login = CampusLoginData {
            username: std::env::var("CD_USER").unwrap(),
            password: std::env::var("CD_PASS").unwrap(),
        };
        println!(
            "input lengths: user={} pass={}",
            login.username.len(),
            login.password.len()
        );
        let (auth, info) = cdlogin_get_jcookie_and_meta(login).await.unwrap();
        println!("login ok, user info filled: {}", !info.user.is_empty());
        println!("cookie store json bytes: {}", auth.cookie.len());
        let cookies: Vec<Value> = serde_json::from_str(&auth.cookie).unwrap();
        for c in &cookies {
            let raw = c["raw_cookie"].as_str().unwrap_or("");
            let name = raw.split('=').next().unwrap_or("?");
            println!("  cookie {name:.40}: {} bytes, domain {}", c.to_string().len(), c["domain"]);
        }
        let jwt_len = crate::auth::encode_jwt(auth.clone()).unwrap().len();
        println!("jwt bytes: {jwt_len} (browser cookie limit is 4096)");

        // roundtrip: rebuild the client purely from the serialized cookies
        let client = get_client_with_cd_cookie(false, auth.cookie).unwrap();
        for path in [
            "ZCM_EM_STUDENT_TIMETABLE_SRV/EventListSet/$count?sap-client=100",
            "ZPIQ_COURSE_RESULTS_SRV/CourseResultSet?$top=1",
        ] {
            let r = client
                .get(format!("{CD_BASE}/sap/opu/odata/sap/{path}"))
                .header("Accept", "application/json")
                .send()
                .await
                .unwrap();
            println!("{path} -> {}", r.status());
            if path.contains("CourseResultSet") {
                // print field names only, never values
                let v: Value = r.json().await.unwrap();
                if let Some(o) = v["d"]["results"][0].as_object() {
                    println!("grade fields: {:?}", o.keys().collect::<Vec<_>>());
                }
            }
        }
    }

    fn shape(v: &Value, key: &str) -> String {
        const SHOW: [&str; 8] = [
            "Status", "StatusCode", "Cpunit", "ZzProcedureReason", "BookingMode", "ProgramType",
            "SessionID", "ReturnMessage",
        ];
        match v {
            Value::String(s) if SHOW.contains(&key) || s.len() <= 2 => format!("{s:?}"),
            Value::String(s) if s.starts_with("/Date(") => "date".into(),
            Value::String(s) => format!("str(len {})", s.len()),
            other => other.to_string(),
        }
    }

    /// Same env vars as live_login. Prints structure only.
    #[tokio::test]
    #[ignore]
    async fn live_grades_shape() {
        init_cert();
        let login = CampusLoginData {
            username: std::env::var("CD_USER").unwrap(),
            password: std::env::var("CD_PASS").unwrap(),
        };
        let (auth, _) = cdlogin_get_jcookie_and_meta(login).await.unwrap();
        let client = get_client_with_cd_cookie(false, auth.cookie).unwrap();
        let get = |path: String| {
            let client = client.clone();
            async move {
                client
                    .get(format!("{CD_BASE}/sap/opu/odata/sap/{path}"))
                    .header("Accept", "application/json")
                    .send()
                    .await
                    .unwrap()
                    .json::<Value>()
                    .await
                    .unwrap()
            }
        };

        let all = get("ZPIQ_COURSE_RESULTS_SRV/CourseResultSet?$skip=0&$top=100&$inlinecount=allpages".into()).await;
        let results = all["d"]["results"].as_array().unwrap();
        println!("__count={} returned={}", all["d"]["__count"], results.len());
        let mut combos = std::collections::BTreeMap::new();
        for r in results {
            *combos
                .entry(format!(
                    "Status={} StatusCode={} proc={} gradeValueEmpty={}",
                    r["Status"], r["StatusCode"], r["ZzProcedureReason"], r["GradeValue"] == ""
                ))
                .or_insert(0) += 1;
        }
        println!("status combos: {combos:#?}");

        // one record with appraisals expanded: pick the first that has a grade
        let first = results.iter().find(|r| r["GradeValue"] != "").unwrap_or(&results[0]);
        let cid = first["CourseID"].as_str().unwrap();
        let aid = first["AcadYearID"].as_str().unwrap();
        let sid = first["SessionID"].as_str().unwrap();
        let pid = first["ProgramOfStudyID"].as_str().unwrap();
        let pt = first["ProgramType"].as_str().unwrap();
        for nav in ["DetailedAppraisals", "CourseDetails"] {
            let v = get(format!(
                "ZPIQ_COURSE_RESULTS_SRV/CourseResultSet(CourseID='{cid}',AcadYearID='{aid}',SessionID='{sid}',ProgramOfStudyID='{pid}',ProgramType='{pt}')/{nav}"
            ))
            .await;
            println!("== {nav}");
            let rows = v["d"]["results"].as_array().cloned().unwrap_or_else(|| vec![v["d"].clone()]);
            for row in rows.iter().take(3) {
                if let Some(o) = row.as_object() {
                    for (k, x) in o.iter().filter(|(k, _)| *k != "__metadata") {
                        println!("  {k}: {}", shape(x, k));
                    }
                }
                println!("  --");
            }
        }
    }

    /// Runs the real service handlers against the live system. Prints counts only.
    #[tokio::test]
    #[ignore]
    async fn live_services() {
        use crate::services::{check_revive_session, get_ects, get_fachsem, get_grades, get_stundenplan};
        use axum::Extension;
        init_cert();
        let login = CampusLoginData {
            username: std::env::var("CD_USER").unwrap(),
            password: std::env::var("CD_PASS").unwrap(),
        };
        let (auth, _) = cdlogin_get_jcookie_and_meta(login).await.unwrap();

        let revive = check_revive_session(Extension(auth.clone())).await;
        println!("healthcheck: session alive (no relogin needed) = {}", matches!(revive, Ok(axum::Json(None))));

        let plan = get_stundenplan(Extension(auth.clone())).await;
        match plan {
            Ok(axum::Json(items)) => {
                let min = items.iter().map(|i| i.start).min();
                let max = items.iter().map(|i| i.start).max();
                println!("stundenplan: {} items, start range {min:?}..{max:?}", items.len());
                if let Some(i) = items.first() {
                    let dt = chrono::DateTime::from_timestamp_millis(i.start).unwrap();
                    println!("  first item start (UTC) = {dt}, color {}", i.color);
                }
            }
            Err(e) => println!("stundenplan FAILED: {} {}", e.status_code, e.message),
        }

        match get_ects(Extension(auth.clone())).await {
            Ok(v) => println!("ects: {v}"),
            Err(e) => println!("ects FAILED: {} {}", e.status_code, e.message),
        }
        match get_fachsem(Extension(auth.clone())).await {
            Ok(v) => println!("fachsem: {v}"),
            Err(e) => println!("fachsem FAILED: {} {}", e.status_code, e.message),
        }

        match get_grades(Extension(auth)).await {
            Ok(axum::Json(g)) => {
                println!("grades: {} modules, {} with subgrades", g.len(), g.iter().filter(|m| !m.subgrades.is_empty()).count());
                println!("  passed={} failed={} open={}",
                    g.iter().filter(|m| m.total_passed == Some(true)).count(),
                    g.iter().filter(|m| m.total_passed == Some(false)).count(),
                    g.iter().filter(|m| m.total_passed.is_none()).count());
                if let Some(m) = g.iter().find(|m| !m.subgrades.is_empty()) {
                    println!("  credits sum={} zero-credit modules={}", g.iter().map(|m| m.credit_points).sum::<i32>(), g.iter().filter(|m| m.credit_points == 0).count());
                    println!("  sample: credits={} period={:?} grade_len={} subgrades={}", m.credit_points, m.akad_period, m.grade.len(), m.subgrades.len());
                }
            }
            Err(e) => println!("grades FAILED: {} {}", e.status_code, e.message),
        }
    }

    /// Diagnoses paging and credit fields of CourseResultSet. Prints counts/numbers only.
    #[tokio::test]
    #[ignore]
    async fn live_paging() {
        init_cert();
        let login = CampusLoginData {
            username: std::env::var("CD_USER").unwrap(),
            password: std::env::var("CD_PASS").unwrap(),
        };
        let (auth, _) = cdlogin_get_jcookie_and_meta(login).await.unwrap();
        let client = get_client_with_cd_cookie(false, auth.cookie).unwrap();
        let get = |q: String| {
            let client = client.clone();
            async move {
                client
                    .get(format!("{CD_BASE}/sap/opu/odata/sap/ZPIQ_COURSE_RESULTS_SRV/CourseResultSet?{q}"))
                    .header("Accept", "application/json")
                    .send()
                    .await
                    .unwrap()
                    .json::<Value>()
                    .await
                    .unwrap()
            }
        };
        let key = |r: &Value| {
            format!("{}|{}|{}|{}", r["CourseID"], r["AcadYearID"], r["SessionID"], r["ProgramOfStudyID"])
        };
        for q in [
            "$skip=0&$top=100",
            "$skip=0&$top=10",
            "$skip=10&$top=10",
            "$skip=37&$top=100",
            "$skip=0&$top=100&$expand=DetailedAppraisals",
            "$skip=37&$top=100&$expand=DetailedAppraisals",
            "$skip=0&$top=100&$orderby=AcadYearID",
        ] {
            let v = get(q.to_string()).await;
            let rows = v["d"]["results"].as_array().cloned().unwrap_or_default();
            let keys: std::collections::BTreeSet<_> = rows.iter().map(&key).collect();
            let first = rows.first().map(&key).unwrap_or_default();
            println!("{q:<55} rows={:<3} unique={:<3} first_key_hash={}", rows.len(), keys.len(), first.len() + first.bytes().map(|b| b as usize).sum::<usize>());
        }
        let v = get("$skip=0&$top=100".into()).await;
        let rows = v["d"]["results"].as_array().unwrap();
        let mut by: std::collections::BTreeMap<String, usize> = Default::default();
        for r in rows {
            *by.entry(format!(
                "Credits={} Attempted={} Cpunit={} CreditUnit={} status={}",
                r["Credits"], r["AttemptedCredits"], r["Cpunit"], r["CreditUnit"], r["StatusCode"]
            ))
            .or_default() += 1;
        }
        for (k, n) in by {
            println!("{n:>3} x {k}");
        }
    }
}
