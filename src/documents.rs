//! Student documents (Leistungsbescheinigung, Studienbescheinigung, ...) from
//! ZBA_CM_STUDENT_DOCUMENTS_SRV, as the "Studentendokumente" app of CampusDual offers them

use axum::{
    Extension, Json,
    body::Body,
    extract::Query,
    response::{IntoResponse, Response},
};
use http::{StatusCode, header};
use serde::{Deserialize, Serialize};

use crate::{
    campus_backend::{login::CD_BASE, req_client_funcs::get_client_with_cd_cookie},
    types::{CdAuthData, ODataList, ResponseError},
};

const SRV: &str = "ZBA_CM_STUDENT_DOCUMENTS_SRV";
/// Shown first, it is the document the app was asked for
const LEISTUNGSBESCHEINIGUNG: &str = "LEISTUNGS_BESCH_BA";

/// ZBA_CM_STUDENT_DOCUMENTS_SRV/FormSet: one row per printable document
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Form {
    formname: String,
    form_title: String,
    student_id: String,
    /// `/Date(ms)/`, the day the document is created on (today)
    creation_date: String,
}

/// ZBA_CM_STUDENT_DOCUMENTS_SRV/SemesterSet: the periods the documents can be printed for
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Semester {
    perid: String,
    peryr: String,
    /// 1 is the current period
    sort_key: i64,
}

#[derive(Serialize)]
pub struct StudentDocument {
    pub id: String,
    pub title: String,
}

#[derive(Deserialize)]
pub struct DocumentQuery {
    pub id: String,
}

async fn odata_list<T: serde::de::DeserializeOwned>(
    client: &reqwest_middleware::ClientWithMiddleware,
    query: &str,
) -> Result<Vec<T>, ResponseError> {
    let resp = client
        .get(format!("{CD_BASE}/sap/opu/odata/sap/{SRV}/{query}"))
        .header("Accept", "application/json")
        .send()
        .await?;
    if resp.status().is_redirection() {
        return Err(session_expired());
    }
    let list: ODataList<T> = resp.error_for_status()?.json().await?;
    Ok(list.d.results)
}

fn session_expired() -> ResponseError {
    ResponseError {
        message: "CampusDual session expired".to_string(),
        status_code: StatusCode::UNAUTHORIZED,
    }
}

async fn get_forms(
    client: &reqwest_middleware::ClientWithMiddleware,
) -> Result<Vec<Form>, ResponseError> {
    let mut forms: Vec<Form> =
        odata_list(client, "FormSet?sap-client=100&$skip=0&$top=100").await?;
    forms.sort_by_key(|f| f.formname != LEISTUNGSBESCHEINIGUNG);
    Ok(forms)
}

pub async fn list_documents(
    Extension(cd_authdata): Extension<CdAuthData>,
) -> Result<Json<Vec<StudentDocument>>, ResponseError> {
    let client = get_client_with_cd_cookie(true, cd_authdata.cookie)?;
    let forms = get_forms(&client).await?;
    Ok(Json(
        forms
            .into_iter()
            .map(|f| StudentDocument {
                id: f.formname,
                title: f.form_title,
            })
            .collect(),
    ))
}

/// Passes on the PDF CampusDual generates, for the current period like the CampusDual app does
pub async fn get_document(
    Extension(cd_authdata): Extension<CdAuthData>,
    Query(query): Query<DocumentQuery>,
) -> Result<Response, ResponseError> {
    let client = get_client_with_cd_cookie(true, cd_authdata.cookie)?;

    // only names CampusDual lists end up in the request path
    let form = get_forms(&client)
        .await?
        .into_iter()
        .find(|f| f.formname == query.id)
        .ok_or_else(|| ResponseError {
            message: "Dokument nicht gefunden".to_string(),
            status_code: StatusCode::NOT_FOUND,
        })?;

    let semester = odata_list::<Semester>(&client, "SemesterSet?sap-client=100&$skip=0&$top=100")
        .await?
        .into_iter()
        .min_by_key(|s| s.sort_key)
        .ok_or_else(|| ResponseError {
            message: "Kein Semester für die Studentendokumente gefunden".to_string(),
            status_code: StatusCode::NOT_FOUND,
        })?;

    let creation_date = odata_date_to_key(&form.creation_date).ok_or_else(|| ResponseError {
        message: format!("unexpected CreationDate {:?}", form.creation_date),
        status_code: StatusCode::BAD_GATEWAY,
    })?;

    let url = format!(
        "{CD_BASE}/sap/opu/odata/sap/{SRV}/PrintDocumentSet(Formname='{}',StudentId='{}',CreationDate=datetime'{}',Peryr='{}',Perid='{}')/$value",
        form.formname, form.student_id, creation_date, semester.peryr, semester.perid
    );
    let resp = client.get(url).send().await?;
    if resp.status().is_redirection() {
        return Err(session_expired());
    }
    let resp = resp.error_for_status()?;

    let disposition = resp
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| {
            format!(
                "attachment; filename=\"{}.pdf\"",
                ascii_filename(&form.form_title)
            )
        });

    Ok((
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (header::CONTENT_DISPOSITION, disposition),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        Body::from(resp.bytes().await?),
    )
        .into_response())
}

/// `/Date(1793145600000)/` -> `2026-10-28T00%3A00%3A00`, the key format of PrintDocumentSet
fn odata_date_to_key(raw: &str) -> Option<String> {
    let ms: i64 = raw
        .strip_prefix("/Date(")?
        .strip_suffix(")/")?
        .parse()
        .ok()?;
    let dt = chrono::DateTime::from_timestamp_millis(ms)?;
    Some(dt.format("%Y-%m-%dT%H%%3A%M%%3A%S").to_string())
}

fn ascii_filename(title: &str) -> String {
    title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_date_becomes_key() {
        assert_eq!(
            odata_date_to_key("/Date(1793145600000)/").as_deref(),
            Some("2026-10-28T00%3A00%3A00")
        );
        assert_eq!(odata_date_to_key("2026-10-28"), None);
    }

    #[test]
    fn parses_form_and_semester_rows() {
        let forms: ODataList<Form> = serde_json::from_str(
            r#"{"d":{"results":[{"__metadata":{},"Formname":"LEISTUNGS_BESCH_BA","FormTitle":"Leistungsbescheinigung","StudentId":"1","CreationDate":"/Date(1793145600000)/","Peryr":"0000","Perid":"000"}]}}"#,
        )
        .unwrap();
        assert_eq!(forms.d.results[0].formname, LEISTUNGSBESCHEINIGUNG);
        let sems: ODataList<Semester> = serde_json::from_str(
            r#"{"d":{"results":[{"Perid":"002","Peryr":"2026","SortKey":2,"PeriodTitle":"x"},{"Perid":"001","Peryr":"2027","SortKey":1,"PeriodTitle":"y"}]}}"#,
        )
        .unwrap();
        let current = sems
            .d
            .results
            .into_iter()
            .min_by_key(|s| s.sort_key)
            .unwrap();
        assert_eq!(
            (current.peryr.as_str(), current.perid.as_str()),
            ("2027", "001")
        );
    }
}
