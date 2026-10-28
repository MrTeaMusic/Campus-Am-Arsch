use std::{sync::Arc, time::Duration};

use axum::{
    Router, middleware,
    response::IntoResponse,
    routing::{get, post},
};
use http::{Method, header::CONTENT_TYPE};
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};
use tower_http::cors::{Any, CorsLayer};

use crate::{
    auth, board, documents,
    constants::{
        LOGIN_RATELIMIT_QUOTA, LOGIN_RATELIMIT_RESTORE_INTERVAL_SEC, RATELIMIT_QUOTA,
        RATELIMIT_RESTORE_INTERVAL_SEC,
    },
    push,
    ratelimit_keyextractor::{GovIpOrGlobalExtractorHashed, GovJwtExtractorHashed},
    services, shared_calendar,
};

pub async fn app() -> Router {
    // Bucket rate limiting:
    // Budget of 20 requests
    // Each request consumes 1 budget
    // Increase budget by 1 every second
    let governor_conf_jwt = Arc::new(
        GovernorConfigBuilder::default()
            .burst_size(*RATELIMIT_QUOTA.get().unwrap())
            .per_second(*RATELIMIT_RESTORE_INTERVAL_SEC.get().unwrap())
            .key_extractor(GovJwtExtractorHashed)
            .finish()
            .unwrap(),
    );

    let governor_conf_signin = Arc::new(
        GovernorConfigBuilder::default()
            .burst_size(*LOGIN_RATELIMIT_QUOTA.get().unwrap())
            .per_second(*LOGIN_RATELIMIT_RESTORE_INTERVAL_SEC.get().unwrap())
            .key_extractor(GovIpOrGlobalExtractorHashed)
            .finish()
            .unwrap(),
    );

    let governor_limiter_jwt = governor_conf_jwt.limiter().clone();
    let governor_limiter_signin = governor_conf_signin.limiter().clone();

    // a separate background task to clean up
    let interval = Duration::from_secs(60);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(interval);
            governor_limiter_jwt.retain_recent();
            governor_limiter_signin.retain_recent();
        }
    });

    let cors = CorsLayer::new()
        // allow `GET` and `POST` when accessing the resource
        .allow_methods([Method::GET, Method::POST])
        // allow requests from any origin
        .allow_origin(Any)
        .allow_headers([CONTENT_TYPE]);

    Router::new()
        .route("/check_revive_session", get(services::check_revive_session))
        .route("/get_grades", get(services::get_grades))
        .route("/get_gradestats", post(services::not_supported_in_v2))
        .route("/get_examsignup", get(services::not_supported_in_v2))
        .route("/registerexam", post(services::not_supported_in_v2))
        .route("/get_examdetails", post(services::not_supported_in_v2))
        .route("/cancelexam", post(services::not_supported_in_v2))
        .route("/get_examverfahren", get(services::not_supported_in_v2))
        .route("/get_ects", get(services::get_ects))
        .route("/get_fachsem", get(services::get_fachsem))
        .route("/get_examstats", get(services::get_examstats))
        .route("/get_examreg_status", get(services::get_examreg_status))
        .route("/get_stundenplan", get(services::get_stundenplan))
        .route("/get_reminders", get(services::not_supported_in_v2))
        .route("/get_timeline", get(services::not_supported_in_v2))
        .route("/get_student_documents", get(documents::list_documents))
        .route("/get_student_document", get(documents::get_document))
        .route("/get_shared_events", get(shared_calendar::list_events))
        .route("/add_shared_event", post(shared_calendar::add_event))
        .route("/update_shared_event", post(shared_calendar::update_event))
        .route("/delete_shared_event", post(shared_calendar::delete_event))
        .route("/get_board_posts", get(board::list_posts))
        .route("/add_board_post", post(board::add_post))
        .route("/delete_board_post", post(board::delete_post))
        .route("/get_push_key", get(push::get_key))
        .route("/subscribe_push", post(push::subscribe))
        .route("/unsubscribe_push", post(push::unsubscribe))
        // apply auth and jwt rate limiting to all previous (jwt is only stored as hash)
        .layer(GovernorLayer::new(governor_conf_jwt))
        .layer(middleware::from_fn(auth::authorize))
        // sign in rate limiting (based on username, only stored as hash)
        .route(
            "/signin",
            post(auth::sign_in).layer(GovernorLayer::new(governor_conf_signin)),
        )
        .route("/", get(|| async { "API is reachable".into_response() }))
        .layer(cors)
}
