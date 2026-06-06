FROM rust:1-bookworm AS builder

WORKDIR /app
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/src ./src
COPY backend/migrations ./migrations
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates wget \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/user-admin-backend /usr/local/bin/user-admin-backend
COPY backend/migrations ./migrations

ENV APP_HOST=0.0.0.0
ENV APP_PORT=8080
EXPOSE 8080

CMD ["user-admin-backend"]
