use crate::error::AppError;
use crate::handlers::auth;
use crate::handlers::{AppState, RequestMetric};
use crate::models::user::{NewUser, UpdateUser, User};
use crate::services::users::UserServiceError;
use askama::Template;
use axum::{
    extract::{Form, Path, State},
    http::HeaderMap,
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;

#[derive(Template)]
#[template(path = "users/index.html")]
struct IndexTemplate {
    viewer: Option<User>,
    request_metrics: Vec<RequestMetric>,
    users: Vec<User>,
}

#[derive(Template)]
#[template(path = "users/show.html")]
struct ShowTemplate {
    viewer: Option<User>,
    request_metrics: Vec<RequestMetric>,
    user: User,
}

#[derive(Template)]
#[template(path = "users/new.html")]
struct NewTemplate {
    viewer: Option<User>,
    request_metrics: Vec<RequestMetric>,
    form: UserFormView,
}

#[derive(Template)]
#[template(path = "users/edit.html")]
struct EditTemplate {
    viewer: Option<User>,
    request_metrics: Vec<RequestMetric>,
    form: UserFormView,
}

#[derive(Debug, Deserialize)]
pub struct UserForm {
    name: String,
    email: String,
    #[serde(default)]
    password: String,
}

#[derive(Debug, Clone)]
struct UserFormView {
    action: String,
    submit_label: String,
    name: String,
    email: String,
    password: String,
    name_error: Option<String>,
    email_error: Option<String>,
    password_error: Option<String>,
    show_password: bool,
}

impl UserFormView {
    fn new(action: impl Into<String>, submit_label: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            submit_label: submit_label.into(),
            name: String::new(),
            email: String::new(),
            password: String::new(),
            name_error: None,
            email_error: None,
            password_error: None,
            show_password: true,
        }
    }

    fn from_user(user: &User) -> Self {
        Self {
            action: format!("/users/{}", user.id),
            submit_label: "Save user".to_string(),
            name: user.name.clone(),
            email: user.email.clone(),
            password: String::new(),
            name_error: None,
            email_error: None,
            password_error: None,
            show_password: false,
        }
    }

    fn validate(
        action: impl Into<String>,
        submit_label: impl Into<String>,
        form: UserForm,
        show_password: bool,
    ) -> Result<Self, Self> {
        let name = form.name.trim().to_string();
        let email = form.email.trim().to_string();
        let mut view = Self {
            action: action.into(),
            submit_label: submit_label.into(),
            name,
            email,
            password: form.password.trim().to_string(),
            name_error: None,
            email_error: None,
            password_error: None,
            show_password,
        };

        if view.name.is_empty() {
            view.name_error = Some("Name is required.".to_string());
        }

        if view.email.is_empty() {
            view.email_error = Some("Email is required.".to_string());
        } else if !view.email.contains('@') {
            view.email_error = Some("Email must contain @.".to_string());
        }

        if show_password {
            if view.password.is_empty() {
                view.password_error = Some("Password is required.".to_string());
            } else if view.password.len() < 8 {
                view.password_error = Some("Password must be at least 8 characters.".to_string());
            }
        }

        if view.name_error.is_some()
            || view.email_error.is_some()
            || view.password_error.is_some()
        {
            Err(view)
        } else {
            Ok(view)
        }
    }
}

pub async fn render_index(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let viewer = auth::current_user(&state, &headers).await?;
    let users = state.user_service().list_users().await?;
    let request_metrics = state.request_metrics.recent();
    let template = IndexTemplate {
        viewer,
        request_metrics,
        users,
    };
    Ok(template)
}

pub async fn render_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let viewer = auth::current_user(&state, &headers).await?;
    let request_metrics = state.request_metrics.recent();
    let user = state
        .user_service()
        .get_user(id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("User with id {} not found", id)))?;

    render_template(
        ShowTemplate {
            viewer,
            request_metrics,
            user,
        },
        StatusCode::OK,
    )
}

pub async fn render_new_user(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let viewer = auth::current_user(&state, &headers).await?;
    let request_metrics = state.request_metrics.recent();
    render_template(
        NewTemplate {
            viewer,
            request_metrics,
            form: UserFormView::new("/users", "Create user"),
        },
        StatusCode::OK,
    )
}

pub async fn create_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<UserForm>,
) -> Result<Response, AppError> {
    let viewer = auth::current_user(&state, &headers).await?;
    let request_metrics = state.request_metrics.recent();
    let mut form = match UserFormView::validate("/users", "Create user", form, true) {
        Ok(form) => form,
        Err(form) => {
            return render_template(
                NewTemplate {
                    viewer,
                    request_metrics,
                    form,
                },
                StatusCode::UNPROCESSABLE_ENTITY,
            )
        }
    };

    let user = match state
        .user_service()
        .create_user(NewUser {
            name: form.name.clone(),
            email: form.email.clone(),
            password: form.password.clone(),
        })
        .await
    {
        Ok(user) => user,
        Err(UserServiceError::DuplicateEmail) => {
            form.email_error = Some("Email already exists.".to_string());
            return render_template(
                NewTemplate {
                    viewer,
                    request_metrics,
                    form,
                },
                StatusCode::CONFLICT,
            );
        }
        Err(UserServiceError::InvalidCredentials) | Err(UserServiceError::PasswordHash(_)) => {
            return Err(AppError::Internal);
        }
        Err(UserServiceError::Database(err)) => return Err(AppError::Database(err)),
    };

    auth::create_session_and_redirect(&state, user.id, &format!("/users/{}", user.id)).await
}

pub async fn render_edit_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let viewer = auth::current_user(&state, &headers).await?;
    let request_metrics = state.request_metrics.recent();
    let user = state
        .user_service()
        .get_user(id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("User with id {} not found", id)))?;

    render_template(
        EditTemplate {
            viewer,
            request_metrics,
            form: UserFormView::from_user(&user),
        },
        StatusCode::OK,
    )
}

pub async fn update_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<UserForm>,
) -> Result<Response, AppError> {
    let viewer = auth::current_user(&state, &headers).await?;
    let request_metrics = state.request_metrics.recent();
    let mut form = match UserFormView::validate(format!("/users/{}", id), "Save user", form, false)
    {
        Ok(form) => form,
        Err(form) => {
            return render_template(
                EditTemplate {
                    viewer,
                    request_metrics,
                    form,
                },
                StatusCode::UNPROCESSABLE_ENTITY,
            )
        }
    };

    let user = match state
        .user_service()
        .update_user(
            id,
            UpdateUser {
                name: form.name.clone(),
                email: form.email.clone(),
            },
        )
        .await
    {
        Ok(Some(user)) => user,
        Ok(None) => return Err(AppError::NotFound(format!("User with id {} not found", id))),
        Err(UserServiceError::DuplicateEmail) => {
            form.email_error = Some("Email already exists.".to_string());
            return render_template(
                EditTemplate {
                    viewer,
                    request_metrics,
                    form,
                },
                StatusCode::CONFLICT,
            );
        }
        Err(UserServiceError::InvalidCredentials) | Err(UserServiceError::PasswordHash(_)) => {
            return Err(AppError::Internal);
        }
        Err(UserServiceError::Database(err)) => return Err(AppError::Database(err)),
    };

    Ok(Redirect::to(&format!("/users/{}", user.id)).into_response())
}

pub async fn delete_user(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let deleted = state.user_service().delete_user(id).await?;

    if deleted {
        Ok(Redirect::to("/users").into_response())
    } else {
        Err(AppError::NotFound(format!("User with id {} not found", id)))
    }
}

fn render_template<T: Template>(template: T, status: StatusCode) -> Result<Response, AppError> {
    let html = template.render().map_err(|_| AppError::Internal)?;
    Ok((status, Html(html)).into_response())
}
