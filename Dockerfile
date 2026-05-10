# Web build stage
FROM oven/bun:1 AS web-builder

WORKDIR /app

COPY web/package.json web/bun.lock* ./web/
RUN cd web && bun install --frozen-lockfile

COPY web ./web
RUN cd web && bun run build

# Rust build stage
FROM rust:1.88-slim-bookworm AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y \
    build-essential \
    make \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

COPY src ./src
COPY block_types.json ./block_types.json
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

ENV HYDRO_DATA_DIR=/app/data
RUN mkdir -p /app/data

COPY --from=builder /app/target/release/Hydro ./Hydro
COPY --from=web-builder /app/dist ./dist
COPY block_types.json ./block_types.json

EXPOSE 3000

CMD ["./Hydro"]
