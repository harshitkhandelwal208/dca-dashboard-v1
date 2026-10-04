# DCA bot + dashboard, one image. Build context is the repository root:
#   docker build -f bot/Dockerfile -t dca-bot .
#
# Stage 1: the React dashboard (static files served by the bot)
FROM node:20-bookworm-slim AS dashboard
WORKDIR /dashboard
COPY dashboard/package.json dashboard/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY dashboard/ ./
RUN npm run build

# Stage 2: PaddleOCR models (PP-OCRv5 ONNX, ~100MB; also fetched on demand if the folder is empty)
FROM debian:bookworm-slim AS models
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
COPY bot/scripts/download-models.sh /download-models.sh
RUN sh /download-models.sh /models

# Stage 3: the Rust binary (ONNX Runtime is linked statically and needs glibc >= 2.38, hence trixie)
FROM rust:trixie AS builder
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config cmake && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY bot/native/ native/
RUN cd native && cargo build --release -p dca-bot \
 && cp target/release/dca-bot /build/dca-bot

# Stage 4: runtime
FROM debian:trixie-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libstdc++6 \
    fontconfig \
    fonts-noto-core \
    fonts-noto-cjk \
 && rm -rf /var/lib/apt/lists/*

# MALLOC_ARENA_MAX keeps the allocator from hoarding memory on a 512 MB host.
ENV PORT=3000 \
    RUST_LOG=info \
    MALLOC_ARENA_MAX=1

WORKDIR /app
COPY --from=builder /build/dca-bot /app/dca-bot
COPY --from=models /models /app/models
COPY --from=dashboard /dashboard/dist /app/dashboard
COPY bot/fonts /app/fonts
COPY bot/assets /app/assets

RUN mkdir -p /app/data
EXPOSE 3000
CMD ["/app/dca-bot"]
