use crate::error::AppError;
use crate::handlers::{AppState, RequestMetric};
use crate::models::user::User;
use askama::Template;
use axum::{
    extract::{Form, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use uuid::Uuid;

const SESSION_COOKIE_NAME: &str = "chat_session";

#[derive(Template)]
#[template(path = "auth/login.html")]
struct LoginTemplate {
    viewer: Option<User>,
    request_metrics: Vec<RequestMetric>,
    email: String,
    email_error: Option<String>,
    password_error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    email: String,
    password: String,
}

pub async fn create_session_and_redirect(
    state: &AppState,
    user_id: i64,
    location: &str,
) -> Result<Response, AppError> {
    let token = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO sessions (token, user_id) VALUES (?, ?)")
        .bind(&token)
        .bind(user_id)
        .execute(&state.pool)
        .await
        .map_err(AppError::Database)?;

    Ok(redirect_with_cookie(location, &token))
}

pub async fn render_login(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let viewer = current_user(&state, &headers).await?;
    let request_metrics = state.request_metrics.recent();
    render_template(
        LoginTemplate {
            viewer,
            request_metrics,
            email: String::new(),
            email_error: None,
            password_error: None,
        },
        StatusCode::OK,
    )
}

pub async fn login(
    State(state): State<AppState>,
    Form(form): Form<LoginForm>,
) -> Result<Response, AppError> {
    let email = form.email.trim().to_string();
    let request_metrics = state.request_metrics.recent();
    let mut template = LoginTemplate {
        viewer: None,
        request_metrics,
        email: email.clone(),
        email_error: None,
        password_error: None,
    };

    if email.is_empty() {
        template.email_error = Some("Email is required.".to_string());
    } else if !email.contains('@') {
        template.email_error = Some("Email must contain @.".to_string());
    }

    if form.password.trim().is_empty() {
        template.password_error = Some("Password is required.".to_string());
    }

    if template.email_error.is_some() || template.password_error.is_some() {
        return render_template(template, StatusCode::UNPROCESSABLE_ENTITY);
    }

    let user = match state
        .user_service()
        .authenticate_user(&email, &form.password)
        .await
    {
        Ok(user) => user,
        Err(crate::services::users::UserServiceError::InvalidCredentials) => {
            template.password_error = Some("Email or password is incorrect.".to_string());
            return render_template(template, StatusCode::UNAUTHORIZED);
        }
        Err(crate::services::users::UserServiceError::Database(err)) => {
            return Err(AppError::Database(err));
        }
        Err(crate::services::users::UserServiceError::DuplicateEmail)
        | Err(crate::services::users::UserServiceError::PasswordHash(_)) => {
            return Err(AppError::Internal);
        }
    };

    create_session_and_redirect(&state, user.id, "/chat").await
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, AppError> {
    if let Some(token) = session_token(&headers) {
        sqlx::query("DELETE FROM sessions WHERE token = ?")
            .bind(token)
            .execute(&state.pool)
            .await
            .map_err(AppError::Database)?;
    }

    Ok(redirect_with_cookie("/users", ""))
}

pub async fn current_user(state: &AppState, headers: &HeaderMap) -> Result<Option<User>, AppError> {
    let Some(token) = session_token(headers) else {
        return Ok(None);
    };

    let user = sqlx::query_as::<_, User>(
        r#"
        SELECT users.id, users.name, users.email
        FROM sessions
        INNER JOIN users ON users.id = sessions.user_id
        WHERE sessions.token = ?
        "#,
    )
    .bind(token)
    .fetch_optional(&state.pool)
    .await
    .map_err(AppError::Database)?;

    Ok(user)
}

fn session_token(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;

    cookie_header.split(';').find_map(|cookie| {
        let (name, value) = cookie.trim().split_once('=')?;
        (name == SESSION_COOKIE_NAME).then(|| value.to_string())
    })
}

fn redirect_with_cookie(location: &str, token: &str) -> Response {
    let mut response = Redirect::to(location).into_response();
    let cookie = if token.is_empty() {
        format!(
            "{}=; HttpOnly; Path=/; Max-Age=0; SameSite=Lax",
            SESSION_COOKIE_NAME
        )
    } else {
        format!(
            "{}={}; HttpOnly; Path=/; SameSite=Lax",
            SESSION_COOKIE_NAME, token
        )
    };

    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).expect("valid cookie header"),
    );
    response
}

fn render_template<T: Template>(template: T, status: StatusCode) -> Result<Response, AppError> {
    let html = template.render().map_err(|_| AppError::Internal)?;
    Ok((status, Html(html)).into_response())
}
