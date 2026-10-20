use reqwest::StatusCode;
use scraper::ElementRef;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct CampusLoginData {
    pub username: String,
    pub password: String,
}

// JWT Claims
#[derive(Serialize, Deserialize, Clone)]
pub struct Claims {
    pub exp: usize,     // expiration time
    pub iat: usize,     // issued at
    pub nonce: String,  // AES nonce
    pub cipher: String, // AES cipher (CdAuthData)
}

// API Response type
pub struct ResponseError {
    pub message: String,
    pub status_code: StatusCode,
}

#[derive(serde::Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserBasicInfo,
}

// Inserted by the auth middleware into the request extension
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdAuthData {
    pub cookie: String,
    pub hash: String,
    pub user: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CampusDualGrade {
    pub name: String,
    pub grade: String,
    pub total_passed: Option<bool>,
    pub credit_points: i32,
    pub akad_period: String,
    pub subgrades: Vec<CampusDualSubGrade>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CampusDualSubGrade {
    pub name: String,
    pub grade: String,
    pub passed: Option<bool>,
    pub beurteilung: String,
    pub bekanntgabe: String,
    pub wiederholung: Option<String>,
    pub akad_period: String,
    pub internal_metadata: Option<SubGradeMetadata>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SubGradeMetadata {
    pub module: String,
    pub peryr: String,
    pub perid: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CampusDualSignupOption {
    pub name: String,
    pub verfahren: String,
    pub pruefart: String,
    pub status: String,
    pub signup_information: String,
    pub exam_date: Option<String>,
    pub exam_time: Option<String>,
    pub exam_room: Option<String>,
    pub warning_message: Option<String>,
    pub signup_until: Option<String>,
    pub internal_metadata: Option<ExamRegistrationMetadata>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ExamRegistrationMetadata {
    pub assessment: String,
    pub peryr: String,
    pub perid: String,
    pub offerno: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CampusDualVerfahrenOption {
    pub name: String,
    pub verfahren: String,
    pub pruefart: String,
    pub status: String,
    pub signup_information: String,
    pub exam_date: Option<String>,
    pub exam_time: Option<String>,
    pub exam_room: Option<String>,
    pub warning_message: Option<String>,
    pub signoff_until: Option<String>,
    pub internal_metadata: Option<ExamRegistrationMetadata>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct UserBasicInfo {
    pub first_name: String,
    pub last_name: String,
    pub seminar_group: String,
    pub seminar_name: String,
    pub user: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CdExamStats {
    #[serde(rename(deserialize = "EXAMS"))]
    pub total: i64,

    #[serde(rename(deserialize = "SUCCESS"))]
    pub successful: i64,

    #[serde(rename(deserialize = "FAILURE"))]
    pub unsuccessful: i64,

    #[serde(rename(deserialize = "BOOKED"))]
    pub unassessed: i64,

    #[serde(rename(deserialize = "MBOOKED"))]
    pub booked: i64,

    #[serde(rename(deserialize = "MODULES"))]
    pub finished: i64,

    #[serde(rename(deserialize = "WPCOUNT"))]
    pub ronmodus: i64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StundenplanItem {
    #[serde(rename = "allDay")]
    all_day: bool,
    pub color: String,
    pub font_color: Option<String>,
    description: String,
    editable: bool,
    pub end: i64,
    instructor: String,
    remarks: String,
    room: String,
    sinstructor: String,
    sroom: String,
    pub start: i64,
    pub title: String,
}

/// One row of ZCM_EM_STUDENT_TIMETABLE_SRV/EventListSet
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TimetableEvent {
    pub start: String,
    pub end: String,
    #[serde(default)]
    pub event_title: String,
    #[serde(default)]
    pub event_description: String,
    #[serde(default)]
    pub lecturer_name: String,
    #[serde(default)]
    pub room: String,
    #[serde(default)]
    pub cohorts: String,
    #[serde(default)]
    pub remarks: String,
    #[serde(default)]
    pub otype: String,
    #[serde(default)]
    pub exam_short: String,
}

impl StundenplanItem {
    pub fn from_event(ev: TimetableEvent, start: i64, end: i64) -> Self {
        let is_exam = !ev.exam_short.is_empty() || ev.otype != "E";
        StundenplanItem {
            all_day: false,
            color: if is_exam { "darkred".to_string() } else { String::new() },
            font_color: None,
            description: ev.event_description,
            editable: false,
            end,
            instructor: ev.lecturer_name.clone(),
            remarks: ev.remarks,
            room: ev.room.clone(),
            sinstructor: ev.lecturer_name,
            sroom: ev.room,
            start,
            title: ev.event_title,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct LatestReminder {
    #[serde(rename(deserialize = "ACAD_SESSION"))]
    acad_session: String,
    #[serde(rename(deserialize = "ACAD_YEAR"))]
    acad_year: String,
    #[serde(rename(deserialize = "AGRDATE"))]
    agrdate: String,
    #[serde(rename(deserialize = "AGRTYPE"))]
    agrtype: String,
    #[serde(rename(deserialize = "AWOBJECT"))]
    awobject: String,
    #[serde(rename(deserialize = "AWOBJECT_SHORT"))]
    awobject_short: String,
    #[serde(rename(deserialize = "AWOTYPE"))]
    awotype: String,
    #[serde(rename(deserialize = "AWSTATUS"))]
    awstatus: String,
    #[serde(rename(deserialize = "BOOKDATE"))]
    bookdate: String,
    #[serde(rename(deserialize = "BOOKREASON"))]
    bookreason: String,
    #[serde(rename(deserialize = "CPGRADED"))]
    cpgraded: String,
    #[serde(rename(deserialize = "CPUNIT"))]
    cpunit: String,
    #[serde(rename(deserialize = "GRADESYMBOL"))]
    gradesymbol: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct UpcomingReminder {
    #[serde(rename(deserialize = "BEGUZ"))]
    beguz: String,
    #[serde(rename(deserialize = "COMMENT"))]
    comment: String,
    #[serde(rename(deserialize = "ENDUZ"))]
    enduz: String,
    #[serde(rename(deserialize = "EVDAT"))]
    evdat: String,
    #[serde(rename(deserialize = "INSTRUCTOR"))]
    instructor: String,
    #[serde(rename(deserialize = "LOCATION"))]
    location: String,
    #[serde(rename(deserialize = "OBJID"))]
    objid: String,
    #[serde(rename(deserialize = "ROOM"))]
    room: String,
    #[serde(rename(deserialize = "SINSTRUCTOR"))]
    sinstructor: String,
    #[serde(rename(deserialize = "SM_SHORT"))]
    sm_short: String,
    #[serde(rename(deserialize = "SM_STEXT"))]
    sm_stext: String,
    #[serde(rename(deserialize = "SROOM"))]
    sroom: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CampusReminders {
    #[serde(rename(deserialize = "ELECTIVES"))]
    electives: i64,
    #[serde(rename(deserialize = "EXAMS"))]
    exams: i64,
    #[serde(rename(deserialize = "LATEST"))]
    latest: Vec<LatestReminder>,
    #[serde(rename(deserialize = "SEMESTER"))]
    semester: i64,
    #[serde(rename(deserialize = "UPCOMING"))]
    upcoming: Vec<UpcomingReminder>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampusTimeline {
    pub events: Vec<CampusTimelineEvent>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampusTimelineEvent {
    pub start: Option<String>,
    pub end: Option<String>,
    pub duration_event: Option<bool>,
    pub color: String,
    pub title: String,
    pub caption: String,
    pub description: String,
    pub track_num: Option<i64>,
    pub duration: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ExportTimelineEvents {
    pub fachsemester: Vec<ExportTimelineEvent>,
    pub theoriesemester: Vec<ExportTimelineEvent>,
    pub praxissemester: Vec<ExportTimelineEvent>,
    pub specials: Vec<ExportTimelineEvent>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportTimelineEvent {
    pub name: String,
    pub description: String,
    pub color: String,
    pub start: String,
    pub end: String,
}

#[derive(Serialize, Deserialize)]
pub struct CdExamDetails {
    #[serde(rename(deserialize = "EV_AGRTYPE_TEXT"))]
    pub ev_agrtype_text: String,
    #[serde(rename(deserialize = "EV_AUDTYPE_TEXT"))]
    pub ev_audtype_text: String,
    #[serde(rename(deserialize = "EV_CONTINUE_INDICATOR"))]
    pub ev_continue_indicator: String,
    #[serde(rename(deserialize = "EV_DEREG_END"))]
    pub ev_dereg_end: String,
    #[serde(rename(deserialize = "EV_DEREG_ENDTIME"))]
    pub ev_dereg_endtime: String,
    #[serde(rename(deserialize = "EV_DURATION"))]
    pub ev_duration: String,
    #[serde(rename(deserialize = "EV_DURUNIT"))]
    pub ev_durunit: String,
    #[serde(rename(deserialize = "EV_EXAMBEGTIME"))]
    pub ev_exambegtime: String,
    #[serde(rename(deserialize = "EV_EXAMDATE"))]
    pub ev_examdate: String,
    #[serde(rename(deserialize = "EV_EXAMENDTIME"))]
    pub ev_examendtime: String,
    #[serde(rename(deserialize = "EV_EXAMORG_TEXT"))]
    pub ev_examorg_text: String,
    pub ev_examorg_longtext: Option<String>,
    #[serde(rename(deserialize = "EV_INSTRUCTOR"))]
    pub ev_instructor: String,
    #[serde(rename(deserialize = "EV_LOCATION_SHORT"))]
    pub ev_location_short: String,
    #[serde(rename(deserialize = "EV_LOCATION_STEXT"))]
    pub ev_location_stext: String,
    #[serde(rename(deserialize = "EV_OBTYPE_TEXT"))]
    pub ev_obtype_text: String,
    #[serde(rename(deserialize = "EV_REASON"))]
    pub ev_reason: String,
    #[serde(rename(deserialize = "EV_REGIS_BEGIN"))]
    pub ev_regis_begin: String,
    #[serde(rename(deserialize = "EV_REGIS_BEGTIME"))]
    pub ev_regis_begtime: String,
    #[serde(rename(deserialize = "EV_REGIS_END"))]
    pub ev_regis_end: String,
    #[serde(rename(deserialize = "EV_REGIS_ENDTIME"))]
    pub ev_regis_endtime: String,
    #[serde(rename(deserialize = "EV_ROOM_SHORT"))]
    pub ev_room_short: String,
    #[serde(rename(deserialize = "EV_ROOM_STEXT"))]
    pub ev_room_stext: String,
    #[serde(rename(deserialize = "EV_SHORT"))]
    pub ev_short: String,
    #[serde(rename(deserialize = "EV_STEXT"))]
    pub ev_stext: String,
}

#[derive(Deserialize)]
pub struct CdGradeStatEntry {
    #[serde(rename(deserialize = "GRADETEXT"))]
    pub gradetext: String,
    #[serde(rename(deserialize = "COUNT"))]
    pub count: i64,
}

#[derive(Debug, Serialize, Default)]
pub struct GradeStatsAllStudents {
    pub one: i64,
    pub two: i64,
    pub three: i64,
    pub four: i64,
    pub ronmodus: i64,
}

#[derive(Debug)]
pub struct GradeResultsTableType<'a> {
    pub name_el: Option<ElementRef<'a>>,
    pub grade_el: Option<ElementRef<'a>>,
    pub passed_el: Option<ElementRef<'a>>,
    pub ects_el: Option<ElementRef<'a>>,
    pub beurteilung_el: Option<ElementRef<'a>>,
    pub bekanntgabe_el: Option<ElementRef<'a>>,
    pub wiederholung_el: Option<ElementRef<'a>>,
    pub akad_period_el: Option<ElementRef<'a>>,
}

impl<'a> From<&'a mut scraper::element_ref::Select<'a, 'a>> for GradeResultsTableType<'a> {
    fn from(iter: &'a mut scraper::element_ref::Select<'a, 'a>) -> GradeResultsTableType<'a> {
        GradeResultsTableType {
            name_el: iter.next(),
            grade_el: iter.next(),
            passed_el: iter.next(),
            ects_el: iter.next(),
            beurteilung_el: iter.next(),
            bekanntgabe_el: iter.next(),
            wiederholung_el: iter.next(),
            akad_period_el: iter.next(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ODataList<T> {
    pub d: ODataResults<T>,
}

#[derive(Debug, Deserialize)]
pub struct ODataResults<T> {
    pub results: Vec<T>,
}

/// ZPIQ_COURSE_RESULTS_SRV/CourseResultSet. A module and its exam are separate rows that share
/// the `ModuleGroup` of their expanded `CourseDetails`; exam rows are named "P <module> (<type>)"
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CourseResult {
    #[serde(default, rename = "CourseID")]
    pub course_id: String,
    #[serde(default, rename = "AcadYearID")]
    pub acad_year_id: String,
    #[serde(default, rename = "SessionID")]
    pub session_id: String,
    #[serde(default, rename = "ProgramOfStudyID")]
    pub program_of_study_id: String,
    #[serde(default)]
    pub program_type: String,
    #[serde(default)]
    pub course_name: String,
    /// Internal SAP scale value (e.g. 49000.00), not the grade
    #[serde(default)]
    pub grade_value: String,
    /// Display grade, e.g. "3,3"
    #[serde(default)]
    pub results: String,
    #[serde(default)]
    pub status_code: String,
    #[serde(default)]
    pub credits: String,
    #[serde(default)]
    pub session: String,
    #[serde(default)]
    pub zz_announcement_date: String,
    #[serde(default)]
    pub zz_assessment_date: String,
    #[serde(default)]
    pub zz_procedure_reason: String,
    /// An object when requested via $expand, otherwise a `__deferred` link
    #[serde(default)]
    pub course_details: serde_json::Value,
}

impl CourseResult {
    /// Identity of a result row, used to drop rows the service repeats
    pub fn key(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            self.course_id, self.acad_year_id, self.session_id, self.program_of_study_id, self.program_type
        )
    }

    /// Exams ("Prüfungsleistungen") are listed as "P <module> (<type>)"
    pub fn is_exam(&self) -> bool {
        self.course_name.starts_with("P ")
    }

    fn module_group(&self) -> String {
        self.course_details["ModuleGroup"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| self.course_id.clone())
    }

    fn grade(&self) -> String {
        if self.results.is_empty() { "?".to_string() } else { self.results.clone() }
    }

    fn passed(&self) -> Option<bool> {
        // 01 = booked, 02 = passed, 03 = failed
        match self.status_code.as_str() {
            "02" => Some(true),
            "03" => Some(false),
            _ => None,
        }
    }
}

/// Turns the flat result rows into modules with their exams as subgrades
pub fn build_grades(rows: Vec<CourseResult>) -> Vec<CampusDualGrade> {
    let mut seen = std::collections::HashSet::new();
    let mut groups: Vec<(String, Vec<CourseResult>)> = Vec::new();
    for row in rows.into_iter().filter(|r| seen.insert(r.key())) {
        let group = row.module_group();
        match groups.iter_mut().find(|(g, _)| *g == group) {
            Some((_, members)) => members.push(row),
            None => groups.push((group, vec![row])),
        }
    }

    groups
        .into_iter()
        .map(|(_, members)| {
            let (exams, mut modules): (Vec<_>, Vec<_>) = members.into_iter().partition(|r| r.is_exam());
            // a group without a module row (only exams) is shown as the module itself
            let (module, exams) = if modules.is_empty() {
                let mut exams = exams;
                (exams.remove(0), exams)
            } else {
                (modules.remove(0), exams)
            };

            let subgrades: Vec<CampusDualSubGrade> = exams
                .iter()
                .map(|e| CampusDualSubGrade {
                    name: e.course_name.clone(),
                    grade: e.grade(),
                    passed: e.passed(),
                    beurteilung: e.zz_assessment_date.clone(),
                    bekanntgabe: e.zz_announcement_date.clone(),
                    wiederholung: Some(e.zz_procedure_reason.clone()).filter(|r| !r.is_empty()),
                    akad_period: e.session.clone(),
                    internal_metadata: None,
                })
                .collect();

            // a module that is not graded itself shows the result of its exam
            let grade = match module.grade().as_str() {
                "?" => exams.iter().map(|e| e.grade()).find(|g| g != "?").unwrap_or_else(|| "?".to_string()),
                g => g.to_string(),
            };

            CampusDualGrade {
                name: module.course_name.clone(),
                grade,
                total_passed: module.passed().or_else(|| exams.iter().find_map(|e| e.passed())),
                credit_points: module.credits.trim().replace(',', ".").parse::<f64>().map(|f| f as i32).unwrap_or_default(),
                akad_period: module.session.clone(),
                subgrades,
            }
        })
        .collect()
}

/// ZBA_EXAM_REGISTRATION_SRV/PeriodSet
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ExamPeriod {
    pub peryr: String,
    pub perid: String,
}

/// ZBA_EXAM_REGISTRATION_SRV/ExamSet. "B" = "Angebot gebucht" (an exam offer is booked)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ExamRegistration {
    #[serde(default)]
    pub registration_state: String,
}
