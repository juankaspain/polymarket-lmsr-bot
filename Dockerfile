# ─────────────────────────────────────────────────────────────
# Polymarket LMSR Bot — Multi-stage Docker Build
#
# Stage 1 (builder): Compiles with jemalloc on Linux, release profile
#   - lto=true, codegen-units=1, strip=true, opt-level=3
# Stage 2 (runtime): Minimal Debian slim image for production
#
# Build:  docker build -t polymarket-lmsr-bot .
# Run:    docker run --env-file .env -v ./config.toml:/app/config.toml polymarket-lmsr-bot
# ─────────────────────────────────────────────────────────────

# ── Builder stage ────────────────────────────────────────────
FROM rust:1.82-bookworm AS builder

WORKDIR /build

# Install jemalloc build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libjemalloc-dev \
    make \
    && rm -rf /var/lib/apt/lists/*

# Cache dependencies: copy manifests first, build a dummy to cache deps
COPY Cargo.toml Cargo.lock* ./
RUN mkdir -p src && \
    echo 'fn main() { println!("dummy"); }' > src/main.rs && \
    echo '' > src/lib.rs && \
    cargo build --release 2>/dev/null || true && \
    rm -rf src

# Copy full source and build for real
COPY src/ src/
COPY benches/ benches/

# Release build with profile from Cargo.toml (lto, codegen-units=1, strip)
RUN cargo build --release --bin polymarket-lmsr-bot

# ── Runtime stage ────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

# Install minimal runtime deps (TLS certs for HTTPS, jemalloc shared lib)
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libjemalloc2 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user for security
RUN groupadd -r bot && useradd -r -g bot -d /app -s /sbin/nologin bot

WORKDIR /app

# Copy binary from builder
COPY --from=builder /build/target/release/polymarket-lmsr-bot /app/polymarket-lmsr-bot

# Create data directories for persistence (state.json + trades JSONL)
RUN mkdir -p /app/data/trades && chown -R bot:bot /app

# Switch to non-root user
USER bot

# Health check against /live endpoint on :9090
HEALTHCHECK --interval=15s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:9090/live || exit 1

# Expose health/metrics port
EXPOSE 9090

# Entrypoint
ENTRYPOINT ["/app/polymarket-lmsr-bot"]
