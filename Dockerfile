# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS backend-build

WORKDIR /workspace

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --locked

FROM node:24-bookworm-slim AS frontend-build

WORKDIR /workspace/frontend

COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend ./
RUN npm run build

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates wget \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --home-dir /app --shell /usr/sbin/nologin guglerag \
    && mkdir -p /app/data /app/logs /app/frontend/dist \
    && chown -R guglerag:guglerag /app

WORKDIR /app

COPY --from=backend-build --chown=guglerag:guglerag /workspace/target/release/GugleRAG ./GugleRAG
COPY --from=frontend-build --chown=guglerag:guglerag /workspace/frontend/dist ./frontend/dist
COPY --chown=guglerag:guglerag .env.example ./.env.example

ENV SERVER_HOST=0.0.0.0 \
    SERVER_PORT=8080

EXPOSE 8080
VOLUME ["/app/data", "/app/logs"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 CMD ["wget", "--no-verbose", "--tries=1", "--spider", "http://127.0.0.1:8080/health"]

USER guglerag

ENTRYPOINT ["./GugleRAG"]
