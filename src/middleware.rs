use axum::{
    extract::{MatchedPath, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use crate::handlers::AppState;
use std::time::Instant;
use tracing::info;

pub async fn log_request_latency(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let path = request
        .extensions()
        .get::<MatchedPath>()
        .map(|matched_path| matched_path.as_str().to_owned())
        .unwrap_or_else(|| request.uri().path().to_owned());
    let started_at = Instant::now();

    let response = next.run(request).await;
    let status = response.status();
    let elapsed_ms = started_at.elapsed().as_millis() as u64;

    state.request_metrics.record(path.clone(), elapsed_ms);
    log_latency(
        method.as_str(),
        &path,
        status,
        elapsed_ms as f64,
    );

    response
}

fn log_latency(method: &str, path: &str, status: StatusCode, elapsed_ms: f64) {
    info!(
        method,
        path,
        status = status.as_u16(),
        elapsed_ms,
        "request completed"
    );
}
