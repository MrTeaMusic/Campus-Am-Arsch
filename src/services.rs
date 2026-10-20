use axum::{Extension, Json};
use chrono::DateTime;
use fnv::FnvHasher;
use http::StatusCode;
use std::hash::{Hash, Hasher};

use crate::{
    auth::sign_in,
    campus_backend::login::CD_BASE,
    campus_backend::req_client_funcs::{
        extract_exam_signup_options, extract_exam_verfahren_options,
        get_client_default, get_client_with_cd_cookie,
    },
    color_stuff::hex_to_luminance,
    types::{
        build_grades, CampusDualGrade, CampusDualSignupOption, CampusDualVerfahrenOption, CampusLoginData,
        CampusReminders, CourseResult, CampusTimeline, CampusTimelineEvent, CdAuthData, CdExamDetails,
        CdExamStats, CdGradeStatEntry, ExamRegistrationMetadata, ExportTimelineEvent,
        ExportTimelineEvents, GradeStatsAllStudents, LoginResponse, ExamPeriod, ExamRegistration, ODataList, ResponseError,
        StundenplanItem, SubGradeMetadata, TimetableEvent,
    },
};

pub async fn get_grades(
    Extension(cd_auth_data): Extension<CdAuthData>,
) -> Result<Json<Vec<CampusDualGrade>>, ResponseError> {
    let client = get_client_with_cd_cookie(true, cd_auth_data.cookie)?;

    // the service ignores $skip/$top and always returns the same (up to 37) rows
    let page: ODataList<CourseResult> = odata_get(
        &client,
        "ZPIQ_COURSE_RESULTS_SRV/CourseResultSet?$expand=CourseDetails",
    )
    .await?;

    let grades = build_grades(page.d.results);

    Ok(Json(grades))
}

pub async fn get_gradestats(
    Extension(cd_auth_data): Extension<CdAuthData>,
    Json(subgrade_meta): Json<SubGradeMetadata>,
) -> Result<Json<GradeStatsAllStudents>, ResponseError> {
    let client = get_client_with_cd_cookie(true, cd_auth_data.cookie)?;

    let grade_stats: Vec<CdGradeStatEntry> = client
        .get(format!(
            "https://selfservice.campus-dual.de/acwork/mscoredist?module={}&peryr={}&perid={}",
            subgrade_meta.module, subgrade_meta.peryr, subgrade_meta.perid
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut all_stats = GradeStatsAllStudents::default();

    for stat in grade_stats {
        match stat.gradetext.as_str() {
            "sehr gut" => all_stats.one = stat.count,
            "gut" => all_stats.two = stat.count,
            "befriedigend" => all_stats.three = stat.count,
            "ausreichend" => all_stats.four = stat.count,
            "nicht ausreichend" => all_stats.ronmodus = stat.count,
            _ => {
                log::warn!("unknown grade stat key: {}", stat.gradetext)
            }
        }
    }

    Ok(Json(all_stats))
}

pub async fn check_revive_session(
    Extension(cd_auth_data): Extension<CdAuthData>,
) -> Result<Json<Option<LoginResponse>>, ResponseError> {
    let client = get_client_with_cd_cookie(false, cd_auth_data.cookie)?;

    let resp = client
        .get(format!(
            "{CD_BASE}/sap/opu/odata/sap/ZCM_EM_STUDENT_TIMETABLE_SRV/EventListSet/$count?sap-client=100"
        ))
        .send()
        .await?;

    let status = resp.status();
    if status.is_success() {
        return Ok(Json(None)); // session still alive
    }

    // An expired session redirects to the IdP (3xx) or is rejected (401/403)
    if status.is_redirection() || matches!(status.as_u16(), 401 | 403) {
        let new_login_response = sign_in(Json(CampusLoginData {
            username: cd_auth_data.user,
            password: cd_auth_data.password,
        }))
        .await;

        log::info!("CaDu auth revive ok?={}", new_login_response.is_ok());

        return match new_login_response {
            Ok(Json(login_response)) => Ok(Json(Some(login_response))),
            Err(_) => Err(ResponseError {
                message: "Failed to log in to CaDu - did the password change?".to_string(),
                status_code: StatusCode::UNAUTHORIZED,
            }),
        };
    }

    Err(ResponseError {
        message: "CD healthcheck failed".to_string(),
        status_code: status,
    })
}

pub async fn get_examsignup(
    Extension(cd_auth_data): Extension<CdAuthData>,
) -> Result<Json<Vec<CampusDualSignupOption>>, ResponseError> {
    let client = get_client_with_cd_cookie(true, cd_auth_data.cookie)?;
    let exam_signup_html = client
        .get("https://selfservice.campus-dual.de/acwork/expproc")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let signup_options = extract_exam_signup_options(exam_signup_html).await?;

    Ok(Json(signup_options))
}

pub async fn post_registerexam(
    Extension(cd_auth_data): Extension<CdAuthData>,
    Json(examregist_meta): Json<ExamRegistrationMetadata>,
) -> Result<String, ResponseError> {
    let client = get_client_default(true)?;
    let exam_regist_resp = client
        .get(format!(
            "https://selfservice.campus-dual.de/acwork/registerexam?userid={}&assessment={}&peryr={}&perid={}&offerno={}&hash={}",
            cd_auth_data.user,
            examregist_meta.assessment,
            examregist_meta.peryr,
            examregist_meta.perid,
            examregist_meta.offerno,
            cd_auth_data.hash,
        ))
        .send()
        .await?
        .error_for_status()?;

    Ok(exam_regist_resp.text().await?)
}

pub async fn get_examdetails(
    Extension(cd_auth_data): Extension<CdAuthData>,
    Json(examregist_meta): Json<ExamRegistrationMetadata>,
) -> Result<Json<CdExamDetails>, ResponseError> {
    let client = get_client_default(true)?;
    let mut exam_details: CdExamDetails = client
        .get(format!(
            "https://selfservice.campus-dual.de/acwork/offerdetail?user={}&objidexm=undefined&evob_objid={}&peryr={}&perid={}&offerno={}",
            cd_auth_data.user,
            examregist_meta.assessment,
            examregist_meta.peryr,
            examregist_meta.perid,
            examregist_meta.offerno,
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let examorg_long = {
        let resp = client
            .get(format!(
                "https://selfservice.campus-dual.de/acwork/examorg?examorg={}",
                exam_details.ev_examorg_text
            ))
            .send()
            .await?
            .error_for_status();
        match resp {
            Ok(resp) => serde_json::from_str(&resp.text().await?)?,
            Err(_) => exam_details.ev_examorg_text.clone(),
        }
    };

    exam_details.ev_examorg_longtext = Some(examorg_long);
    Ok(Json(exam_details))
}

pub async fn post_cancelexam(
    Extension(cd_auth_data): Extension<CdAuthData>,
    Json(examregist_meta): Json<ExamRegistrationMetadata>,
) -> Result<String, ResponseError> {
    let client = get_client_default(true)?;
    let exam_regist_resp = client
        .get(format!(
            "https://selfservice.campus-dual.de/acwork/cancelexam?userid={}&objid={}&hash={}",
            cd_auth_data.user, examregist_meta.assessment, cd_auth_data.hash
        ))
        .send()
        .await?
        .error_for_status()?;

    Ok(exam_regist_resp.text().await?)
}

pub async fn get_examverfahren(
    Extension(cd_auth_data): Extension<CdAuthData>,
) -> Result<Json<Vec<CampusDualVerfahrenOption>>, ResponseError> {
    let client = get_client_with_cd_cookie(true, cd_auth_data.cookie)?;
    let exam_verfahren_html = client
        .get("https://selfservice.campus-dual.de/acwork/cancelproc")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let signup_verfahren = extract_exam_verfahren_options(exam_verfahren_html).await?;

    Ok(Json(signup_verfahren))
}

/// Achieved ECTS: sum of the credits of all successfully completed modules
pub async fn get_ects(
    Extension(cd_authdata): Extension<CdAuthData>,
) -> Result<String, ResponseError> {
    let client = get_client_with_cd_cookie(true, cd_authdata.cookie)?;

    let results: ODataList<CourseResult> = odata_get(
        &client,
        "ZPIQ_COURSE_RESULTS_SRV/CourseResultSet",
    )
    .await?;

    let mut seen = std::collections::HashSet::new();
    let ects: f64 = results
        .d
        .results
        .iter()
        .filter(|r| !r.is_exam() && r.status_code == "02" && seen.insert(r.key()))
        .map(|r| r.credits.trim().replace(',', ".").parse::<f64>().unwrap_or_default())
        .sum();

    Ok((ects as i64).to_string())
}

pub async fn get_fachsem(
    Extension(cd_authdata): Extension<CdAuthData>,
) -> Result<String, ResponseError> {
    let client = get_client_with_cd_cookie(true, cd_authdata.cookie)?;

    let audit: serde_json::Value = odata_get(
        &client,
        "ZPIQ_DEGREE_AUDIT_SRV/AuditHeaderSet(ProgramOfStudyId='')",
    )
    .await?;

    // "3.00" -> 3
    let number = audit["d"]["CurrentSemester"]
        .as_str()
        .and_then(|s| s.trim().replace(',', ".").parse::<f64>().ok())
        .map(|f| f as u32)
        .unwrap_or(1);

    Ok(number.to_string())
}

/// GET a JSON OData resource below /sap/opu/odata/sap/ with the stored session
async fn odata_get<T: serde::de::DeserializeOwned>(
    client: &reqwest_middleware::ClientWithMiddleware,
    path: &str,
) -> Result<T, ResponseError> {
    let resp = client
        .get(format!("{CD_BASE}/sap/opu/odata/sap/{path}"))
        .header("Accept", "application/json")
        .send()
        .await?;

    if resp.status().is_redirection() {
        return Err(ResponseError {
            message: "CampusDual session expired".to_string(),
            status_code: StatusCode::UNAUTHORIZED,
        });
    }

    Ok(resp.error_for_status()?.json().await?)
}

/// Derived from the module results (V2 has no separate statistics endpoint). "Pending" are the
/// exams with a booked offer in the exam registration, which keeps listing past periods
pub async fn get_examstats(
    Extension(cd_authdata): Extension<CdAuthData>,
) -> Result<Json<CdExamStats>, ResponseError> {
    let client = get_client_with_cd_cookie(true, cd_authdata.cookie)?;

    let results: ODataList<CourseResult> =
        odata_get(&client, "ZPIQ_COURSE_RESULTS_SRV/CourseResultSet").await?;

    // same module/exam grouping as the grades, so both always agree
    let grades = build_grades(results.d.results);
    let successful = grades.iter().filter(|g| g.total_passed == Some(true)).count() as i64;
    let unsuccessful = grades.iter().filter(|g| g.total_passed == Some(false)).count() as i64;
    let booked_modules = grades.iter().filter(|g| g.total_passed.is_none()).count() as i64;

    // exams the student has an offer booked for; falls back to the open modules if unavailable
    let booked = match count_booked_exam_offers(&client).await {
        Ok(n) => n,
        Err(e) => {
            log::warn!("exam registration unavailable: {}", e.message);
            booked_modules
        }
    };

    Ok(Json(CdExamStats {
        total: successful + unsuccessful + booked,
        successful,
        unsuccessful,
        unassessed: booked,
        booked,
        finished: successful + unsuccessful,
        ronmodus: 0,
    }))
}

async fn count_booked_exam_offers(
    client: &reqwest_middleware::ClientWithMiddleware,
) -> Result<i64, ResponseError> {
    let periods: ODataList<ExamPeriod> =
        odata_get(client, "ZBA_EXAM_REGISTRATION_SRV/PeriodSet?sap-client=100&$skip=0&$top=100")
            .await?;

    let mut booked = 0;
    for p in periods.d.results {
        let exams: ODataList<ExamRegistration> = odata_get(
            client,
            &format!(
                "ZBA_EXAM_REGISTRATION_SRV/ExamSet?sap-client=100&$skip=0&$top=100&$filter=Perid%20eq%20%27{}%27%20and%20Peryr%20eq%20%27{}%27",
                p.perid, p.peryr
            ),
        )
        .await?;
        booked += exams.d.results.iter().filter(|e| e.registration_state == "B").count() as i64;
    }
    Ok(booked)
}

pub async fn get_stundenplan(
    Extension(cd_authdata): Extension<CdAuthData>,
) -> Result<Json<Vec<StundenplanItem>>, ResponseError> {
    let client = get_client_with_cd_cookie(true, cd_authdata.cookie)?;

    let resp = client
        .get(format!(
            "{CD_BASE}/sap/opu/odata/sap/ZCM_EM_STUDENT_TIMETABLE_SRV/EventListSet?sap-client=100&$skip=0&$top=5000"
        ))
        .header("Accept", "application/json")
        .send()
        .await?;

    if resp.status().is_redirection() {
        return Err(ResponseError {
            message: "CampusDual session expired".to_string(),
            status_code: StatusCode::UNAUTHORIZED,
        });
    }

    let events: ODataList<TimetableEvent> = resp.error_for_status()?.json().await?;

    let mut stundenplan: Vec<StundenplanItem> = events
        .d
        .results
        .into_iter()
        .filter_map(|ev| {
            let start = odata_date_to_epoch_ms(&ev.start)?;
            let end = odata_date_to_epoch_ms(&ev.end)?;
            Some(StundenplanItem::from_event(ev, start, end))
        })
        .collect();

    for item in &mut stundenplan {
        item.color = match item.color.as_str() {
            "darkred" => "#D41610".to_string(),
            _ => string_to_rgb(&format!("0{}0", item.title)),
        };
        item.font_color = Some(
            if hex_to_luminance(&item.color) < 128.0 {
                "#FFFFFF"
            } else {
                "#000000"
            }
            .to_string(),
        );
    }

    Ok(Json(stundenplan))
}

/// SAP sends `/Date(ms)/` with ms being a real UTC epoch
fn odata_date_to_epoch_ms(raw: &str) -> Option<i64> {
    raw.strip_prefix("/Date(")?.strip_suffix(")/")?.parse().ok()
}

fn string_to_rgb(input: &str) -> String {
    // Create a hasher
    let mut hasher = FnvHasher::default();

    // Hash the input string
    input.hash(&mut hasher);
    let hash = hasher.finish();

    // Extract RGB components from the hash
    let r = (hash & 0xFF) as u8;
    let g = ((hash >> 8) & 0xFF) as u8;
    let b = ((hash >> 16) & 0xFF) as u8;

    format!("#{:02X}{:02X}{:02X}", r, g, b)
}

pub async fn get_reminders(
    Extension(cd_authdata): Extension<CdAuthData>,
) -> Result<Json<CampusReminders>, ResponseError> {
    let client = get_client_default(true)?;

    let user = cd_authdata.user;
    let hash = cd_authdata.hash;

    let resp = client
        .get(format!(
            "https://selfservice.campus-dual.de/dash/getreminders?user={user}&hash={hash}"
        ))
        .send()
        .await?
        .error_for_status()?
        .json::<CampusReminders>()
        .await?;

    Ok(Json(resp))
}

pub async fn get_timeline(
    Extension(cd_authdata): Extension<CdAuthData>,
) -> Result<Json<ExportTimelineEvents>, ResponseError> {
    let client = get_client_default(true)?;
    let resp = client
        .get(format!(
            "https://selfservice.campus-dual.de/dash/gettimeline?user={}",
            cd_authdata.user
        ))
        .send()
        .await?
        .error_for_status()?
        .json::<CampusTimeline>()
        .await;

    if let Ok(timeline) = resp {
        let events = timeline.events;

        let fachsemester: Vec<ExportTimelineEvent> = events_by_color("#fcbe04", &events);
        let theoriesemester: Vec<ExportTimelineEvent> = events_by_color("#0070a3", &events);
        let praxissemester: Vec<ExportTimelineEvent> = events_by_color("#119911", &events);
        let specials: Vec<ExportTimelineEvent> = events_by_color("#880000", &events);

        let export_events = ExportTimelineEvents {
            fachsemester,
            theoriesemester,
            praxissemester,
            specials,
        };

        Ok(Json(export_events))
    } else {
        Ok(Json(ExportTimelineEvents::default()))
    }
}

fn events_by_color(color: &str, events: &[CampusTimelineEvent]) -> Vec<ExportTimelineEvent> {
    events
        .iter()
        .filter(|event| event.color == color)
        .map(|event| ExportTimelineEvent {
            name: event.title.clone(),
            description: event
                .description
                .replace("<br>", " ")
                .replace("<strong>", "")
                .replace("</strong>", ""),
            color: event.color.clone(),
            start: campusdate_to_iso8601(event.start.as_ref()),
            end: campusdate_to_iso8601(event.end.as_ref()),
        })
        .collect()
}

fn campusdate_to_iso8601(input: Option<&String>) -> String {
    let format = "%a, %d %b %Y %H:%M:%S %z";

    input
        .and_then(|input| DateTime::parse_from_str(input, format).ok())
        .map(|parsed| parsed.to_rfc3339())
        .unwrap_or("o. D.".to_string())
}

/// Endpoints that still depend on the pre-V2 CampusDual pages
pub async fn not_supported_in_v2() -> ResponseError {
    ResponseError {
        message: "Diese Funktion ist mit CampusDual V2 noch nicht verfügbar".to_string(),
        status_code: StatusCode::NOT_IMPLEMENTED,
    }
}
