# Cache Bypass Refactor Plan

## Goal

Move all cache logic out of handlers and into services, then thread a `bypass_cache` flag from the HTTP request down to every cache decision.

## Target Architecture

```
HTTP Request
    ↓
Handler (reads headers, extracts bypass flag)
    ↓
Service (owns business logic + cache policy)
    ↓
Repository (raw SQLx, no cache awareness)
    ↓
SQLite
```

### Rules

- Handlers never touch `state.cache` directly.
- Services decide cache hits/misses.
- The `bypass_cache` flag lives in a `QueryContext` struct passed to every service method.
- `AppCache` becomes a private detail of the services layer.

## Step-by-Step Plan

### Step 1: Create `QueryContext`

Add a lightweight struct that travels with every request:

```rust
// src/services/mod.rs or new src/context.rs
#[derive(Clone, Copy, Debug, Default)]
pub struct QueryContext {
    pub bypass_cache: bool,
}
```

### Step 2: Refactor `UserRepository` + `CachedUserRepository`

Update the trait to accept `QueryContext`:

```rust
#[async_trait]
pub trait UserRepository {
    async fn get_user_by_id(&self, ctx: QueryContext, id: i64) -> Result<Option<User>, sqlx::Error>;
    // ... etc
}
```

Update `CachedUserRepository` to check `ctx.bypass_cache` before hitting `self.cache`.

### Step 3: Create a `ChatService`

Move all direct `state.cache.*` calls from `src/handlers/chat.rs` into a new `src/services/chat.rs`. It should encapsulate:

- `get_chat_messages(ctx, room_id)`
- `get_room_participants(ctx, room_id)`
- `get_accessible_rooms(ctx, user_id)`
- `get_unread_counts(ctx, user_id)`
- `get_pending_invites(ctx, user_id)`
- `get_chat_file(ctx, message_id)`
- `get_room_for_user(ctx, user_id, room_id)`

Each method uses `AppCache` internally, but skips it when `ctx.bypass_cache`.

### Step 4: Create an `AuthService` (or extend `UserService`)

Move session-token caching from `src/handlers/auth.rs` into `UserService`:

- `validate_session(ctx, token)` — currently inlined in `auth.rs` with `state.cache.session_by_token.get(...)`.

### Step 5: Update `AppState`

Change `AppState` to expose services instead of raw cache:

```rust
pub struct AppState {
    pub user_repo: Arc<dyn UserRepository + Send + Sync>,
    pub chat_service: ChatService, // or Arc<dyn ChatService>
    pub auth_service: UserService,   // now owns session cache
    pub pool: SqlitePool,
    // pub cache: AppCache, // <-- remove this from public API
    // ...
}
```

If you need `AppCache` for invalidation during writes, keep it `pub(crate)` or pass it into services internally.

### Step 6: Update all Handlers

- Read `Cache-Control` header at the top of each handler.
- Build `QueryContext { bypass_cache }`.
- Replace every `state.cache.*.get(...)` with a service call that takes `ctx`.
- Replace every `state.cache.invalidate_*()` with a service method (e.g., `chat_service.invalidate_room(room_id)`).

### Step 7: Wire up `bypass_cache` from HTTP headers

Add a small helper in `src/handlers/mod.rs`:

```rust
fn query_context(headers: &HeaderMap) -> QueryContext {
    let bypass = headers
        .get("cache-control")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("no-cache"))
        .unwrap_or(false);
    QueryContext { bypass_cache: bypass }
}
```

Every handler starts with `let ctx = query_context(&headers);` and passes `ctx` down.

## Files to Create / Modify

| File | Action |
|------|--------|
| `src/context.rs` (or `src/services/context.rs`) | **Create** — `QueryContext` struct |
| `src/models/user.rs` | **Modify** — add `QueryContext` to `UserRepository` trait methods |
| `src/cache.rs` | **Modify** — update `CachedUserRepository` impl |
| `src/services/chat.rs` | **Create** — `ChatService` with all chat caching logic |
| `src/services/users.rs` | **Modify** — add session validation + cache bypass |
| `src/handlers/chat.rs` | **Modify** — replace direct cache access with `ChatService` calls |
| `src/handlers/auth.rs` | **Modify** — replace direct cache access with `UserService` calls |
| `src/handlers/api.rs` | **Modify** — pass `QueryContext` to `UserRepository` calls |
| `src/handlers/mod.rs` | **Modify** — add `query_context()` helper, update `AppState` |
| `src/app.rs` | **Modify** — wire new services into `AppState` |
| `tests/api_integration.rs` | **Modify** — update trait mock expectations |

## Testing Strategy

1. **Compile after each step** — do not change every file at once.
2. **Add an integration test** that sends `Cache-Control: no-cache` and asserts the response does not come from cache (e.g., verify a DB write between two requests is reflected immediately).
3. **Keep existing tests green** — the refactor should not change observable behavior when `bypass_cache = false`.

## Order of Operations (Minimal Breakage)

1. Create `QueryContext` and the header helper.
2. Refactor `UserRepository` trait + `CachedUserRepository`.
3. Fix `api.rs` handlers to pass `ctx`.
4. Create `ChatService`, move one handler at a time (e.g., `list_messages` first).
5. Move session caching into `UserService`.
6. Remove `cache` from public `AppState` once no handler uses it.
7. Run full test suite.

## Outcome

This plan makes `Cache-Control: no-cache` work **end-to-end**, not just at the HTTP edge, and ensures any future feature automatically inherits the bypass behavior by using the service layer.
