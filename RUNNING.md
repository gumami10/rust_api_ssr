# How To Run

This app can be run locally with Cargo or in Docker.

## Prerequisites

- Rust toolchain with Cargo
- Optional: Docker and Docker Compose
- Optional: k6 for load testing

## Run Locally

Start the app from the project root:

```bash
cargo run
```

The server listens on:

```text
http://127.0.0.1:3000
```

Useful environment variables:

- `BIND_ADDRESS` sets the host and port, default `0.0.0.0:3000`
- `DATABASE_URL` sets the SQLite database URL, default `sqlite://data.db`
- `RUST_LOG` sets logging, default `rust_api_ssr=info,axum=warn`

Example:

```bash
BIND_ADDRESS=127.0.0.1:3000 DATABASE_URL=sqlite://data.db cargo run
```

## Run With Docker

Build and run the container directly:

```bash
docker build -t rust_api_ssr .
docker run --rm -p 3000:3000 rust_api_ssr
```

Or use Docker Compose:

```bash
docker compose up --build
```

The container uses the checked-in `data.db` file so the demo data is available on startup.

## Verify

Run the integration tests:

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

You can point k6 at another instance with `BASE_URL`:

```bash
BASE_URL=http://127.0.0.1:3000 k6 run scripts/k6-load-test.js
```
