use std::path::Path;

use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    routing::{get, post, put},
};
use serde_json::json;
use tower_http::{services::ServeDir, trace::TraceLayer};

use crate::{chunks, events, files, messages, state::AppState};

pub(crate) fn router(state: AppState, web_root: &Path) -> Router {
    Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/config", get(|| async { Json(json!({ "chunk_size": files::CHUNK_SIZE, "upload_concurrency": chunks::UPLOAD_CONCURRENCY, "version": env!("CARGO_PKG_VERSION") })) }))
        .route("/api/messages", get(messages::list).post(messages::create))
        .route("/api/messages/{id}", get(messages::get).patch(messages::edit).delete(messages::delete))
        .route("/api/events", get(events::subscribe))
        .route("/api/uploads", post(files::create))
        .route("/api/uploads/{id}", get(files::status).patch(files::append).delete(files::cancel))
        .route("/api/uploads/{id}/chunks/{offset}", put(chunks::put))
        .route("/api/uploads/{id}/complete", post(files::complete))
        .route("/api/files/{id}/content", get(files::content))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .fallback_service(ServeDir::new(web_root))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
