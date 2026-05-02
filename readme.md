# rust_api_ssr

A small Rust API + server-side rendered web app built around fast response times and low operational overhead.

The project uses Axum on Tokio for async HTTP handling, Askama for compile-time checked HTML templates, and SQLx with SQLite for the demo data layer. It currently serves a minimal users API and an SSR users page from the same router.

## Goals

- Keep request handling lightweight and predictable.
- Serve JSON API responses and server-rendered HTML from one Rust service.
- Use compile-time checked templates instead of runtime template parsing.
- Keep shared application state cheap to clone through `Arc`.
- Log per-request latency for quick feedback while tuning performance.

## Current Features

- `GET /` renders the users list as HTML with Askama.
- `GET /api/users` returns all users as JSON.
- `GET /api/users/:id` returns one user as JSON.
- Missing users return a structured JSON `404` response.
- Request latency is logged by middleware.
- Integration tests cover the API routes and SSR page.
- A Python manual test script can start the server and exercise the routes.

## Tech Stack

- Rust 2021
- Tokio async runtime
- Axum HTTP framework
- Askama / Askama Axum for SSR templates
- SQLx with SQLite
- Serde / serde_json
- Tracing with JSON-formatted logs

## Project Structure

```text
src/
  main.rs              # binary entry point, logging, database setup, server bind
  lib.rs               # library exports for app, handlers, models, middleware, errors
  app.rs               # router construction
  error.rs             # application error type and HTTP response mapping
  middleware.rs        # request latency logging middleware
  handlers/
    api.rs             # JSON API handlers
    views.rs           # SSR page handler
    mod.rs             # shared AppState
  models/
    user.rs            # User model, repository trait, SQLite implementation
templates/
  index.html           # Askama SSR template
tests/
  api_integration.rs   # route integration tests
scripts/
  manual_api_test.py   # manual HTTP tester
```

## Requirements

- Rust toolchain with Cargo
- Python 3, only if you want to use `scripts/manual_api_test.py`

## Run

```bash
cargo run
```

The server listens on:

```text
http://127.0.0.1:3000
```

The binary currently binds to `0.0.0.0:3000`, so it is also reachable from other interfaces where allowed by your environment.

## Test

Run the Rust integration tests:

```bash
cargo test
```

Run manual route checks with the helper script:

```bash
python3 scripts/manual_api_test.py --start-server check-all
```

Other useful manual commands:

```bash
python3 scripts/manual_api_test.py --start-server list-users
python3 scripts/manual_api_test.py --start-server get-user 1
python3 scripts/manual_api_test.py --start-server index
python3 scripts/manual_api_test.py --start-server interactive
```

## API

### `GET /api/users`

Returns:

```json
[
  {
    "id": 1,
    "name": "Alice",
    "email": "alice@example.com"
  },
  {
    "id": 2,
    "name": "Bob",
    "email": "bob@example.com"
  }
]
```

### `GET /api/users/:id`

Returns one user:

```json
{
  "id": 1,
  "name": "Alice",
  "email": "alice@example.com"
}
```

If the user does not exist:

```json
{
  "error": "User with id 999 not found"
}
```

## SSR Page

`GET /` renders `templates/index.html` with the same users loaded through the repository layer. Askama compiles the template at build time, which keeps rendering fast and catches many template mistakes before runtime.

## Latency-Oriented Design Notes

This project is intentionally simple and direct:

- Axum and Tokio provide async request handling without a large framework layer.
- Handlers do only the route-specific work and delegate data access to a repository trait.
- `AppState` is cloneable and stores shared services behind `Arc`.
- Askama avoids runtime template lookup and parsing.
- The latency middleware records elapsed milliseconds for every request.
- JSON logs make latency data easy to consume in local tools or log pipelines.

The demo uses an in-memory SQLite database and inserts sample users at startup. That keeps local startup and tests fast, but the data is not persistent.

## Production Notes

Before using this as a production service, consider:

- Move the database URL, bind address, and pool size into configuration.
- Use a persistent database instead of `sqlite::memory:`.
- Add migrations for schema management.
- Add health and readiness endpoints.
- Add benchmarking for your target latency budget, for example with `wrk`, `oha`, or `bombardier`.
- Tune database indexes and connection pool settings based on real traffic.
- Add graceful shutdown.
- Add stricter observability around p95/p99 latency, error rates, and saturation.

## Environment

`RUST_LOG` controls logging. If unset, the app defaults to:

```text
rust_api_ssr=debug,axum=debug
```

Example:

```bash
RUST_LOG=rust_api_ssr=info,axum=warn cargo run
```
