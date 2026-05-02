use axum::{
    extract::{MatchedPath, Request},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::time::Instant;
use tracing::info;

pub async fn log_request_latency(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request
        .extensions()
        .get::<MatchedPath>()
        .map(|matched_path| matched_path.as_str().to_owned())
        .unwrap_or_else(|| request.uri().path().to_owned());
    let started_at = Instant::now();

    let response = next.run(request).await;
    let status = response.status();
    log_latency(
        method.as_str(),
        &path,
        status,
        started_at.elapsed().as_secs_f64() * 1000.0,
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
