# rust_api_ssr

A Rust API and server-side rendered (SSR) web application built for fast response times and low operational overhead. It serves a JSON REST API, HTML pages, and a real-time WebSocket chat from a single service.

## Goals

- Keep request handling lightweight and predictable.
- Serve JSON API responses and server-rendered HTML from one Rust service.
- Use compile-time checked templates instead of runtime template parsing.
- Keep shared application state cheap to clone through `Arc`.
- Log per-request latency for quick feedback while tuning performance.

## Tech Stack

- **Rust 2021**
- **Tokio** async runtime
- **Axum** HTTP framework (with WebSocket support)
- **Askama / askama_axum** for compile-time checked HTML templates
- **SQLx** with SQLite (compile-time checked queries, migrations)
- **Serde / serde_json** for serialization
- **Argon2** for password hashing
- **Tracing** with JSON-formatted logs
- **thiserror** for error handling

## Architecture

The codebase follows a layered architecture:

```
┌─────────────────────────────────────────────────────────────┐
│  Handlers (src/handlers/)                                   │
│  - api.rs       → JSON REST API endpoints                   │
│  - views.rs     → SSR HTML page endpoints (users CRUD)      │
│  - auth.rs      → Session-based login/logout                │
│  - chat.rs      → Chat rooms, invites, WebSocket handler    │
│  - health.rs    → Health (/healthz) and readiness (/readyz) │
├─────────────────────────────────────────────────────────────┤
│  Services (src/services/)                                   │
│  - users.rs     → Business logic, password hashing, auth    │
├─────────────────────────────────────────────────────────────┤
│  Models / Repositories (src/models/)                        │
│  - user.rs      → User struct, UserRepository trait,        │
│                   SqliteUserRepository implementation         │
├─────────────────────────────────────────────────────────────┤
│  Infrastructure                                             │
│  - app.rs       → Router construction                       │
│  - error.rs     → AppError enum and IntoResponse mapping    │
│  - middleware.rs→ Request latency logging                   │
│  - config.rs    → Environment-based configuration           │
└─────────────────────────────────────────────────────────────┘
```

### Key Design Decisions

- **AppState is cloneable**: All shared state lives behind `Arc` or cheap clone types so Axum can clone it per request without performance issues.
- **Repository trait**: `UserRepository` is a trait with an async implementation (`SqliteUserRepository`). This makes tests easy to inject and keeps handlers decoupled from SQLx directly.
- **Service layer**: `UserService` contains business rules (duplicate email checks, password hashing, authentication). Handlers delegate to services, not repositories directly.
- **Error handling**: A single `AppError` enum covers database, not-found, conflict, and internal errors. It implements `IntoResponse` so handlers can use `?` propagation.
- **Templates**: Askama compiles HTML templates at build time. Runtime rendering is fast and many template errors are caught by `cargo check`.
- **Latency middleware**: Every request is timed and logged via tracing (JSON). The last 8 request metrics are stored in a small in-memory ring buffer and displayed in the UI footer.

## Features

### Users
- `GET /users` — SSR users list
- `GET /users/:id` — SSR user profile
- `GET /users/new` — SSR new user form
- `POST /users` — create user (validates name, email, password ≥8 chars)
- `GET /users/:id/edit` — SSR edit form
- `POST /users/:id` — update user
- `POST /users/:id/delete` — delete user
- `GET /api/users` — JSON list all users
- `GET /api/users/:id` — JSON get one user
- `DELETE /api/users/:id` — delete user (API)

### Authentication
- `GET /login` — login form
- `POST /login` — authenticate with email + password (Argon2 verified)
- `POST /logout` — invalidate session cookie
- Session cookies: `chat_session` token stored in SQLite `sessions` table, HttpOnly, SameSite=Lax
- Creating a user automatically logs them in

### Chat
- `GET /chat` — general chat room (requires login)
- `GET /chat/rooms/:id` — private room (members only)
- `POST /chat/rooms` — create a private room with at least one other participant
- `POST /chat/rooms/:id/invites` — invite a user to a private room
- `POST /chat/invites/:id/accept` — accept a pending invitation
- `GET /chat/ws?room_id=...` — WebSocket endpoint for real-time messages
- `GET /chat/files/:message_id` — serve file attachments inline

**Chat capabilities:**
- Real-time messaging via WebSocket + `tokio::sync::broadcast` channel
- Typing indicators (2-second debounce)
- File sharing (base64 over WebSocket, stored as BLOB in SQLite)
- Unread message badges per room (tracked via `chat_room_read_positions`)
- System notification messages (e.g., "Alice created this room", "Bob joined")

### Health
- `GET /healthz` — liveness probe (always returns 204)
- `GET /readyz` — readiness probe (queries database, returns 204)

## Project Structure

```text
src/
  main.rs              # Entry point: logging, DB pool, migrations, server bind
  lib.rs               # Module re-exports
  app.rs               # Router construction with all routes
  config.rs            # Config from environment variables
  error.rs             # AppError and HTTP response mapping
  middleware.rs        # Request latency logging
  handlers/
    mod.rs             # AppState, RequestMetrics
    api.rs             # JSON API handlers
    views.rs           # SSR page handlers (users CRUD)
    auth.rs            # Session auth handlers
    chat.rs            # Chat rooms, invites, WebSocket
    health.rs          # Health and readiness endpoints
  models/
    mod.rs
    user.rs            # User, NewUser, UpdateUser, UserRepository trait, SqliteUserRepository
  services/
    mod.rs
    users.rs           # UserService, password hashing, auth logic
templates/
  base.html            # Base layout with nav, viewer context, metrics footer
  users/
    index.html         # Users list
    show.html          # User profile
    new.html           # New user form (with faker.js mock data buttons)
    edit.html          # Edit user form
    _form.html         # Shared form partial
  auth/
    login.html         # Login form
  chat/
    index.html         # Chat room page (messages, sidebar, WS client)
migrations/
  0001_create_users.sql
  0002_auth_and_chat.sql
  0003_chat_rooms.sql
  0004_notifications_files_unread.sql
tests/
  api_integration.rs   # API route tests
  ssr_integration.rs   # SSR + auth + chat flow tests
scripts/
  manual_api_test.py   # Python manual HTTP tester
  k6-load-test.js      # k6 load/performance test
Dockerfile
docker-compose.yml
```

## Database Schema

Managed via SQLx migrations in `./migrations`:

1. **users** — `id`, `name`, `email` (unique), `password_hash`
2. **sessions** — `token` (PK), `user_id`, `created_at`
3. **chat_rooms** — `id`, `name`, `kind` (`general`/`private`), `created_by_user_id`, `created_at`
4. **chat_room_members** — `room_id`, `user_id`, `joined_at`
5. **chat_room_invites** — `id`, `room_id`, `invited_user_id`, `invited_by_user_id`, `status`, `created_at`, `accepted_at`
6. **chat_messages** — `id`, `room_id`, `user_id`, `body`, `created_at`, `kind` (`user`/`notification`), `file_name`, `file_data`, `file_content_type`
7. **chat_room_read_positions** — `room_id`, `user_id`, `last_read_message_id`, `updated_at`

General room (`id = 1`) is seeded automatically by migrations.

## Environment Variables

| Variable       | Default                          | Description              |
|----------------|----------------------------------|--------------------------|
| `BIND_ADDRESS` | `0.0.0.0:3000`                   | Server bind address      |
| `DATABASE_URL` | `sqlite://data.db`               | SQLite database URL      |
| `RUST_LOG`     | `rust_api_ssr=info,axum=warn`    | Tracing log filter       |

## Run

```bash
cargo run
```

Server listens on `http://127.0.0.1:3000` by default.

### Docker

```bash
docker build -t rust_api_ssr .
docker run --rm -p 3000:3000 rust_api_ssr
```

Or with Docker Compose:

```bash
docker compose up --build
```

The container mounts `./data.db` so demo data persists across restarts.

## Test

Run the Rust integration tests:

```bash
cargo test
```

Run the manual route checker:

```bash
python3 scripts/manual_api_test.py --start-server check-all
```

Run the k6 load test:

```bash
k6 run scripts/k6-load-test.js
```

Tune k6 with environment variables:

```bash
BASE_URL=http://127.0.0.1:3000 API_VUS=20 SSR_VUS=10 k6 run scripts/k6-load-test.js
```

## API Reference

### `GET /api/users`

Returns:

```json
[
  { "id": 1, "name": "Alice", "email": "alice@example.com" },
  { "id": 2, "name": "Bob", "email": "bob@example.com" }
]
```

### `GET /api/users/:id`

Returns one user or `404`:

```json
{ "error": "User with id 999 not found" }
```

### `DELETE /api/users/:id`

Returns `204` on success or `404` if missing.

## Notes for Agents / Contributors

- All SQL queries use SQLx and are checked at compile time against the migrations.
- When adding a new migration, run `cargo sqlx prepare` if you use `SQLX_OFFLINE` builds.
- Handlers should stay thin: validate input, call a service, return a response. Business logic belongs in `src/services/`.
- The `AppState` owns the broadcast sender for chat. Any code that needs to push real-time events can clone `state.chat_tx`.
- Templates extend `base.html` and receive `viewer` (Option<User>) and `request_metrics` for consistent navigation.
- Tests use an in-memory SQLite database and `tower::ServiceExt::oneshot` to exercise routes without starting a real server.
