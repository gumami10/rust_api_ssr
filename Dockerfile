FROM rust:1.78-bookworm AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        libsqlite3-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY templates ./templates
COPY migrations ./migrations
COPY data.db ./data.db

RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

ENV BIND_ADDRESS=0.0.0.0:3000
ENV DATABASE_URL=sqlite:///app/data.db
ENV RUST_LOG=rust_api_ssr=info,axum=warn

COPY --from=builder /app/target/release/rust_api_ssr /usr/local/bin/rust_api_ssr
COPY --from=builder /app/data.db /app/data.db

EXPOSE 3000

CMD ["rust_api_ssr"]
