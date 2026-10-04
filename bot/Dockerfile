# ==========================================
# Stage 1: Build stage
# ==========================================
FROM rust:1-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy dependency manifests
COPY Cargo.toml Cargo.lock ./

# Create dummy source file to pre-build dependencies and cache Docker layer
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release --locked
RUN rm -rf src target/release/deps/bot* target/release/bot*

# Copy the real source code
COPY src ./src
RUN touch src/main.rs

# Build the release binary
RUN cargo build --release --locked

# ==========================================
# Stage 2: Runtime stage
# ==========================================
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies (TLS certificates, tzdata, ffmpeg, python3, curl)
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    tzdata \
    libssl3 \
    curl \
    ffmpeg \
    python3 \
    && rm -rf /var/lib/apt/lists/*

# Install yt-dlp binary
RUN curl -fsSL https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp -o /usr/local/bin/yt-dlp \
    && chmod a+rx /usr/local/bin/yt-dlp

# Create a non-root user for running the bot securely
RUN groupadd -g 10001 botuser && \
    useradd -u 10001 -g botuser -s /bin/sh -d /home/botuser -m botuser

WORKDIR /home/botuser

# Copy the compiled binary from the builder stage
COPY --from=builder /app/target/release/bot /usr/local/bin/discord_bot

# Set ownership and switch to non-root user
USER botuser:botuser

ENV RUST_LOG=info

ENTRYPOINT ["/usr/local/bin/discord_bot"]
