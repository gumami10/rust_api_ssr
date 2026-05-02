use crate::error::AppError;
use crate::handlers::AppState;
use crate::models::user::User;
use askama::Template;
use axum::{extract::State, response::IntoResponse};

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    users: Vec<User>,
}

pub async fn render_index(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let users = state.user_repo.list_users().await?;
    let template = IndexTemplate { users };
    Ok(template)
}
