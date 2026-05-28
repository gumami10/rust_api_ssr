# Agent Context: rust_api_ssr

## Project Type
Rust Axum web app with SSR (Askama), REST API, SQLite (SQLx), real-time WebSocket chat, and end-to-end encryption for 1-to-1 rooms.

## Architecture (handlers → services → models)
- **Handlers** (`src/handlers/`): thin HTTP layer (api, views, auth, chat, health). Returns JSON or Askama templates.
- **Services** (`src/services/`): business logic. `UserService` handles auth, password hashing (Argon2), duplicate email checks. `ChatService` handles rooms, messages, encryption keys, unread counts.
- **Models** (`src/models/`): data types and repository traits. `UserRepository` is async-trait based; `SqliteUserRepository` is the impl.
- **State** (`AppState`): cloneable struct with `Arc<dyn UserRepository>`, `SqlitePool`, `broadcast::Sender<BroadcastEvent>`, and `RequestMetrics`.

## Key Files
| Purpose | Path |
|---------|------|
| Router | `src/app.rs` |
| Errors | `src/error.rs` (AppError → IntoResponse) |
| Config | `src/config.rs` (BIND_ADDRESS, DATABASE_URL, COOKIE_SECURE from env) |
| Middleware | `src/middleware.rs` (latency logging, request metrics) |
| Cache | `src/cache.rs` (Moka-based AppCache + CachedUserRepository decorator) |
| Context | `src/context.rs` (QueryContext with bypass_cache flag) |
| Chat logic | `src/handlers/chat.rs` (rooms, invites, WS, files, E2E API) |
| Auth | `src/handlers/auth.rs` (session cookies, login/logout, CSRF, rate limiting) |
| User views | `src/handlers/views.rs` (user CRUD pages with validation) |
| API | `src/handlers/api.rs` (JSON REST endpoints for users) |
| Health | `src/handlers/health.rs` (healthz, readyz probes) |
| Tests | `tests/api_integration.rs`, `tests/ssr_integration.rs` |

## Dependencies
- `tokio` (full), `axum` (with ws), `sqlx` (sqlite, tokio-rustls)
- `askama` + `askama_axum` (SSR templates)
- `argon2` + `password-hash` (Argon2id hashing)
- `serde` + `serde_json` (serialization)
- `uuid` (session tokens), `base64` (file uploads)
- `moka` (future-enabled caching)
- `futures-util`, `tokio::sync::broadcast` (WebSocket + pub/sub)
- `tracing` + `tracing-subscriber` (JSON structured logs)
- `tower` (dev, for test oneshot)

## Database (SQLite, SQLx migrations)
Migrations live in `migrations/` and are applied at startup by `main.rs` (`sqlx::migrate!("./migrations").run(pool)`).

### Tables
| Table | Purpose |
|-------|---------|
| `users` | id, name, email, password_hash |
| `sessions` | token (PK), user_id, created_at (30-day expiry) |
| `chat_rooms` | id, name, kind (`general`/`private`), created_by_user_id |
| `chat_room_members` | (room_id, user_id) PK, joined_at |
| `chat_room_invites` | id, room_id, invited_user_id, invited_by_user_id, status (`pending`/`accepted`), created_at, accepted_at |
| `chat_messages` | id, room_id, user_id, body, created_at, kind (`user`/`notification`), is_encrypted, file_name, file_data (BLOB), file_content_type |
| `chat_room_read_positions` | (room_id, user_id) PK, last_read_message_id, updated_at |
| `user_public_keys` | user_id (PK), public_key (for E2E wrapping) |
| `chat_room_keys` | (room_id, user_id) PK, encrypted_key (server never sees plaintext) |

### Migrations
1. `0001_create_users.sql` — Base users table.
2. `0002_auth_and_chat.sql` — Adds password_hash to users, sessions, and legacy chat_messages.
3. `0003_chat_rooms.sql` — Rooms, members, invites; migrates messages to per-room structure.
4. `0004_notifications_files_unread.sql` — Adds kind, file columns, and read_positions.
5. `0005_e2e_encryption.sql` — Adds is_encrypted, user_public_keys, chat_room_keys.

### Important
- General room ID = 1 (seeded in migration 0003).
- E2E encryption applies only to 1-to-1 rooms (exactly 2 participants). Multi-person rooms are not encrypted.
- Tests use in-memory SQLite and manually recreate schema without running migrations (to avoid migration tooling in tests).

## Caching Strategy
`AppCache` (Moka) caches:
- Users by ID/email/password_hash, all-users list
- Sessions by token
- Chat messages per room (30s), participants per room (2m)
- Accessible rooms per user (1m), room-for-user (1m)
- Unread counts per user (15s)
- Pending invites per user (1m)
- Chat files per message (24h)

`QueryContext::bypass_cache` is set when `Cache-Control: no-cache` header is present. Tests verify this behavior.

Cache invalidation is manual/granular via `AppCache` methods and `ChatService` helpers.

## Auth & Sessions
- **Session cookies**: `chat_session` (HttpOnly, SameSite=Lax, Max-Age=30 days). Secure flag controlled by `COOKIE_SECURE` env.
- **CSRF cookies**: `csrf_token` (not HttpOnly, used by logout form JS injection).
- **Session validation**: DB lookup via `UserService::validate_session` with 30-day expiry. Cached in `AppCache`.
- **Password hashing**: Argon2id via `argon2` crate.
- **Login rate limiting**: Per-email, 5 attempts per 15 minutes (`LoginRateLimiter`).
- **Current user extraction**: `auth::current_user(state, headers, ctx)` checks session cookie and returns `Option<User>`.

## Chat Features
### WebSocket
- Endpoint: `GET /chat/ws?room_id=N`
- Broadcast channel (`tokio::sync::broadcast`, capacity 100) pushes `BroadcastEvent::Message` and `BroadcastEvent::Typing` to all clients in the same room.
- Incoming WS payload: JSON with optional `body`, `typing` (bool), `file_data` (base64), `file_name`, `file_content_type`.

### Rooms
- **General** (`/chat`): open to all authenticated users.
- **Private** (`/chat/rooms/:id`): created with at least 1 other participant. Creator auto-joined. Invitation-only for additional members.
- **Unread counts**: computed from `chat_room_read_positions` vs latest message ID per room.
- **Read positions**: updated when visiting a room page.

### Invitations
- `POST /chat/rooms/:id/invites` — invite a non-member to a private room.
- `POST /chat/invites/:id/accept` — accept pending invite and join room.
- Validation prevents self-invites, duplicate invites, invites to general room, invites to existing members.

### File Attachments
- Files sent via WS as base64; stored in `chat_messages.file_data` BLOB.
- Served via `GET /chat/files/:message_id` with content-type and inline disposition.
- Cached aggressively (1 day TTL).

### E2E Encryption (1-to-1 rooms only)
- Browser generates RSA-OAEP 2048 keypair; public key uploaded to server (`POST /api/crypto/public-key`).
- Room key: AES-GCM 256 generated in browser.
- Key sharing: room key is wrapped with each participant's RSA public key and stored server-side (`POST /api/crypto/room-key/:room_id`).
- Message encryption: AES-GCM in browser before sending via WS.
- Server stores ciphertext (`is_encrypted=1`) and forwards it unchanged.
- Decryption happens in browser on receive/render.
- Fallback UI shows `[Encrypted]` when key unavailable.
- Poll loop (3s) re-syncs keys and re-decrypts messages if key changes.

## API Endpoints (JSON)
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/users` | List all users |
| GET | `/api/users/:id` | Get user by ID |
| DELETE | `/api/users/:id` | Delete user |
| GET | `/api/crypto/public-key/:user_id` | Get user's public key |
| POST | `/api/crypto/public-key` | Store own public key |
| GET | `/api/crypto/room-key/:room_id` | Get encrypted room key for self |
| POST | `/api/crypto/room-key/:room_id` | Store encrypted room key for a user |
| GET | `/api/crypto/room-key/:room_id/members` | List user IDs with room keys stored |

## SSR Endpoints (Askama Templates)
| Method | Path | Description |
|--------|------|-------------|
| GET | `/` | Users list (alias of `/users`) |
| GET | `/users` | Users list |
| GET | `/users/new` | Create user form |
| POST | `/users` | Create user (redirects to profile) |
| GET | `/users/:id` | User profile |
| GET | `/users/:id/edit` | Edit user form |
| POST | `/users/:id` | Update user |
| POST | `/users/:id/delete` | Delete user |
| GET | `/login` | Login form |
| POST | `/login` | Authenticate and redirect to `/chat` |
| POST | `/logout` | Clear session, redirect to `/users` |
| GET | `/chat` | General chat room |
| POST | `/chat/rooms` | Create private room |
| GET | `/chat/rooms/:id` | Private chat room |
| POST | `/chat/rooms/:id/invites` | Invite user to room |
| POST | `/chat/invites/:id/accept` | Accept invite |
| GET | `/chat/ws` | WebSocket upgrade |
| GET | `/chat/files/:id` | Serve file attachment |
| GET | `/perf` | Request latency graph/table |
| GET | `/healthz` | Health probe (204) |
| GET | `/readyz` | Readiness probe (DB check, 204) |

## Frontend
- **Tailwind CSS** via CDN (`https://cdn.tailwindcss.com`).
- **Custom theme config** in `base.html` (Inter font, glow shadow).
- **Faker.js integration** on new-user form for demo data (`@faker-js/faker@9` from esm.sh).
- **Chart.js** on `/perf` page for latency bar chart.
- **Dark mode UI** with slate/cyan/emerald palette, glassmorphism cards, backdrop blur.
- **Activity widget** fixed bottom-right showing recent request latencies on all pages.

## Error Handling
`AppError` (thiserror enum):
- `Database(sqlx::Error)` → 500
- `Conflict(String)` → 409
- `NotFound(String)` → 404
- `Forbidden` → 403
- `Internal` → 500

All variants render JSON `{ "error": "..." }` via `IntoResponse`.

`UserServiceError` converts into `AppError`:
- `DuplicateEmail` → `Conflict`
- `InvalidCredentials` / `PasswordHash` → `Internal`

## Middleware
- `log_request_latency`: records request path + elapsed_ms into `RequestMetrics` (max 200 entries). Logs JSON via tracing.

## Testing
- Uses `tower::ServiceExt::oneshot` against in-memory SQLite.
- `tests/api_integration.rs`: JSON API + cache bypass + basic SSR smoke test.
- `tests/ssr_integration.rs`: Full SSR flows including auth, rooms, invites, files, E2E encryption, validation errors.
- Both test suites manually set up tables to avoid SQLx migration runtime dependency.

## Environment Variables
| Variable | Default | Description |
|----------|---------|-------------|
| `BIND_ADDRESS` | `0.0.0.0:3000` | Server listen address |
| `DATABASE_URL` | `sqlite://data.db` | SQLite DB path |
| `COOKIE_SECURE` | `true` | Set `Secure` flag on cookies |
| `RUST_LOG` | `rust_api_ssr=info,axum=warn` | Tracing filter |

## Load Testing
`scripts/k6-load-test.js` — k6 script with three scenarios:
- `health_checks` (1 VU, 30s)
- `api_reads` (10 VUs, 1m)
- `ssr_reads` (5 VUs, 1m)

Thresholds: <1% errors, p95<500ms, >99% checks pass.

## Rules
- Keep handlers thin; put business logic in `services`.
- Use `AppError` for errors; it maps to JSON and HTTP statuses automatically.
- Askama templates are compile-time checked; they live in `templates/` and extend `base.html`.
- Tests use in-memory SQLite + `tower::ServiceExt::oneshot`.
- When adding migrations, ensure queries still compile with SQLx (`cargo sqlx prepare` if using compile-time checks).
- Do not run `git commit` or `git push` unless explicitly asked.

## File Tree Summary
```
rust_api_ssr/
├── Cargo.toml
├── AGENTS.md
├── .agents/
│   └── AGENTS.md (this file)
├── migrations/
│   ├── 0001_create_users.sql
│   ├── 0002_auth_and_chat.sql
│   ├── 0003_chat_rooms.sql
│   ├── 0004_notifications_files_unread.sql
│   └── 0005_e2e_encryption.sql
├── scripts/
│   └── k6-load-test.js
├── src/
│   ├── lib.rs
│   ├── main.rs
│   ├── app.rs
│   ├── config.rs
│   ├── error.rs
│   ├── context.rs
│   ├── middleware.rs
│   ├── cache.rs
│   ├── handlers/
│   │   ├── mod.rs
│   │   ├── api.rs
│   │   ├── auth.rs
│   │   ├── chat.rs
│   │   ├── health.rs
│   │   └── views.rs
│   ├── services/
│   │   ├── mod.rs
│   │   ├── users.rs
│   │   └── chat.rs
│   └── models/
│       ├── mod.rs
│       └── user.rs
├── templates/
│   ├── base.html
│   ├── auth/login.html
│   ├── chat/index.html
│   ├── perf.html
│   └── users/
│       ├── index.html
│       ├── show.html
│       ├── new.html
│       ├── edit.html
│       └── _form.html
└── tests/
    ├── api_integration.rs
    └── ssr_integration.rs
```
