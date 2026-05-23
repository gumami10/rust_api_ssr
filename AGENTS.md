# Agent Context: rust_api_ssr

## Project Type
Rust Axum web app with SSR (Askama), REST API, SQLite (SQLx), and real-time WebSocket chat.

## Architecture (handlers → services → models)
- **Handlers** (`src/handlers/`): thin HTTP layer (api, views, auth, chat, health). Returns JSON or Askama templates.
- **Services** (`src/services/`): business logic. `UserService` handles auth, password hashing (Argon2), duplicate email checks.
- **Models** (`src/models/`): data types and repository traits. `UserRepository` is async-trait based; `SqliteUserRepository` is the impl.
- **State** (`AppState`): cloneable struct with `Arc<dyn UserRepository>`, `SqlitePool`, `broadcast::Sender<BroadcastEvent>`, and `RequestMetrics`.

## Key Files
| Purpose | Path |
|---------|------|
| Router | `src/app.rs` |
| Errors | `src/error.rs` (AppError → IntoResponse) |
| Config | `src/config.rs` (BIND_ADDRESS, DATABASE_URL from env) |
| Middleware | `src/middleware.rs` (latency logging) |
| Chat logic | `src/handlers/chat.rs` (rooms, invites, WS, files) |
| Auth | `src/handlers/auth.rs` (session cookies, login/logout) |
| Tests | `tests/api_integration.rs`, `tests/ssr_integration.rs` |

## Database (SQLite, SQLx migrations)
Tables: `users`, `sessions`, `chat_rooms`, `chat_room_members`, `chat_room_invites`, `chat_messages`, `chat_room_read_positions`.
General room ID = 1.

## Rules
- Keep handlers thin; put business logic in `services`.
- Use `AppError` for errors; it maps to JSON and HTTP statuses automatically.
- Askama templates are compile-time checked; they live in `templates/` and extend `base.html`.
- Tests use in-memory SQLite + `tower::ServiceExt::oneshot`.
- When adding migrations, ensure queries still compile with SQLx.
- Do not run `git commit` or `git push` unless explicitly asked.
