#!/usr/bin/env bash
set -e

# Change directory to project root
cd "$(dirname "$0")"

echo "=========================================="
echo " Starting Discord Bot in Development Mode"
echo "=========================================="

# 1. Ensure PostgreSQL container is running
echo "Ensuring PostgreSQL is running in Docker..."
docker compose up postgres -d

# 2. Stop production bot container if running to prevent conflicting Discord gateway sessions
if docker ps --format '{{.Names}}' | grep -q "^discord_ai_bot$"; then
    echo "Stopping background discord_ai_bot container to avoid duplicate bot sessions..."
    docker stop discord_ai_bot >/dev/null
fi

# 3. Launch the bot locally with fast incremental compilation
if command -v cargo-watch >/dev/null 2>&1; then
    echo "⚡ cargo-watch detected: Hot-reloading active on file changes in bot/src!"
    cargo watch -w bot/src -x "run --manifest-path bot/Cargo.toml"
else
    echo "⚡ Launching bot via cargo run (fast incremental build)..."
    echo "💡 Tip: run 'cargo install cargo-watch' for automatic hot-reloading on file save!"
    cargo run --manifest-path bot/Cargo.toml
fi
