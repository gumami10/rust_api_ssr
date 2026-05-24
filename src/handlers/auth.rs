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
const CSRF_COOKIE_NAME: &str = "csrf_token";
const SESSION_MAX_AGE_SECS: i64 = 30 * 24 * 60 * 60; // 30 days

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

#[derive(Debug, Deserialize)]
pub struct LogoutForm {
    csrf_token: String,
}

pub async fn create_session_and_redirect(
    state: &AppState,
    user_id: i64,
    location: &str,
) -> Result<Response, AppError> {
    let token = Uuid::new_v4().to_string();
    let csrf_token = Uuid::new_v4().to_string();

    sqlx::query("INSERT INTO sessions (token, user_id) VALUES (?, ?)")
        .bind(&token)
        .bind(user_id)
        .execute(&state.pool)
        .await
        .map_err(AppError::Database)?;

    let mut response = Redirect::to(location).into_response();

    let secure_flag = if state.cookie_secure { "; Secure" } else { "" };

    let session_cookie = format!(
        "{}={}; HttpOnly; Path=/; Max-Age={}; SameSite=Lax{}",
        SESSION_COOKIE_NAME, token, SESSION_MAX_AGE_SECS, secure_flag
    );
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&session_cookie).expect("valid session cookie header"),
    );

    let csrf_cookie = format!(
        "{}={}; Path=/; Max-Age={}; SameSite=Lax{}",
        CSRF_COOKIE_NAME, csrf_token, SESSION_MAX_AGE_SECS, secure_flag
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&csrf_cookie).expect("valid csrf cookie header"),
    );

    Ok(response)
}

pub async fn render_login(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let ctx = crate::handlers::query_context(&headers);
    let viewer = current_user(&state, &headers, ctx).await?;
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

    if !state.login_rate_limiter.is_allowed(&email) {
        template.password_error =
            Some("Too many login attempts. Please try again later.".to_string());
        return render_template(template, StatusCode::TOO_MANY_REQUESTS);
    }

    let user = match state
        .user_service
        .authenticate_user(crate::context::QueryContext::default(), &email, &form.password)
        .await
    {
        Ok(user) => user,
        Err(crate::services::users::UserServiceError::InvalidCredentials) => {
            state.login_rate_limiter.record_attempt(&email);
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

    state.login_rate_limiter.clear(&email);
    create_session_and_redirect(&state, user.id, "/chat").await
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LogoutForm>,
) -> Result<Response, AppError> {
    let csrf_cookie = csrf_token_from_headers(&headers);
    if csrf_cookie.as_deref() != Some(&form.csrf_token) {
        return Err(AppError::Forbidden);
    }

    if let Some(token) = session_token(&headers) {
        sqlx::query("DELETE FROM sessions WHERE token = ?")
            .bind(&token)
            .execute(&state.pool)
            .await
            .map_err(AppError::Database)?;
        state.user_service.invalidate_session(&token).await;
    }

    let mut response = Redirect::to("/users").into_response();
    let secure_flag = if state.cookie_secure { "; Secure" } else { "" };

    let session_cookie = format!(
        "{}=; HttpOnly; Path=/; Max-Age=0; SameSite=Lax{}",
        SESSION_COOKIE_NAME, secure_flag
    );
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&session_cookie).expect("valid session cookie header"),
    );

    let csrf_cookie = format!(
        "{}=; Path=/; Max-Age=0; SameSite=Lax{}",
        CSRF_COOKIE_NAME, secure_flag
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&csrf_cookie).expect("valid csrf cookie header"),
    );

    Ok(response)
}

pub async fn current_user(
    state: &AppState,
    headers: &HeaderMap,
    ctx: crate::context::QueryContext,
) -> Result<Option<User>, AppError> {
    let Some(token) = session_token(headers) else {
        return Ok(None);
    };

    state
        .user_service
        .validate_session(ctx, &token, &state.pool)
        .await
        .map_err(|e| match e {
            crate::services::users::UserServiceError::Database(err) => AppError::Database(err),
            _ => AppError::Internal,
        })
}

fn session_token(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;

    cookie_header.split(';').find_map(|cookie| {
        let (name, value) = cookie.trim().split_once('=')?;
        (name == SESSION_COOKIE_NAME).then(|| value.to_string())
    })
}

fn csrf_token_from_headers(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;

    cookie_header.split(';').find_map(|cookie| {
        let (name, value) = cookie.trim().split_once('=')?;
        (name == CSRF_COOKIE_NAME).then(|| value.to_string())
    })
}

fn render_template<T: Template>(template: T, status: StatusCode) -> Result<Response, AppError> {
    let html = template.render().map_err(|_| AppError::Internal)?;
    Ok((status, Html(html)).into_response())
}
