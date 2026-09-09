# 프런트엔드와 Rust 를 함께 빌드해 실행 파일과 정적 파일만 담은 이미지를 만든다.
#
# 스테이지가 셋인 이유:
#   1. frontend  — Node 는 빌드에만 필요하고 최종 이미지에는 넣지 않는다.
#   2. backend   — Rust 툴체인과 target/ 도 마찬가지다.
#   3. runtime   — 실행 파일 + dist + migrations 만 남긴다.

# ─── 1. 프런트엔드 빌드 ───
FROM node:20-slim AS frontend

WORKDIR /app/frontend

# 의존성 설치를 소스 변경과 분리해 캐시가 유지되게 한다.
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci

COPY frontend/ ./
RUN npm run build


# ─── 2. Rust 빌드 ───
# rust-toolchain.toml 이 버전을 고정하므로 베이스 태그와 무관하게 같은 컴파일러를 쓴다.
FROM rust:1-slim AS backend

WORKDIR /app

RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libssl-dev \
 && rm -rf /var/lib/apt/lists/*

COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
# 툴체인을 먼저 내려받아 소스가 바뀌어도 다시 받지 않게 한다.
RUN rustup show active-toolchain

COPY src ./src
COPY migrations ./migrations
COPY tests ./tests

RUN cargo build --release --locked --bin style-engine


# ─── 3. 런타임 ───
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 실행에 필요한 것만 복사한다. Node, cargo, target/ 은 넘어오지 않는다.
COPY --from=backend  /app/target/release/style-engine  /usr/local/bin/style-engine
COPY --from=frontend /app/frontend/dist                /app/frontend/dist
# 마이그레이션은 sqlx::migrate! 가 빌드 시점에 바이너리로 포함하지만,
# 사람이 확인할 수 있도록 이미지에도 남겨 둔다.
COPY migrations /app/migrations

# 업로드된 룩북 이미지가 쓰이는 경로. 볼륨을 붙이지 않으면 컨테이너와 함께 사라진다.
RUN mkdir -p /app/static/images

ENV FRONTEND_DIST=/app/frontend/dist \
    RUST_LOG=style_engine=info

EXPOSE 3003

# 기존 health endpoint 를 그대로 쓴다.
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
  CMD curl -fsS http://localhost:3003/api/health || exit 1

CMD ["style-engine"]
