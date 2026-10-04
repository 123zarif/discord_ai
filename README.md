# Discord AI Assistant (Rust Backend Foundation)

A production-grade Rust Discord bot backend foundation engineered to eventually learn and imitate a specific user's conversational style by processing Discord chat history, generating vector embeddings, and orchestrating semantic retrieval with local LLM inference.

This phase establishes a robust, extensible foundation focused on modular architecture, asynchronous PostgreSQL with `pgvector`, structured logging, configuration validation, slash command orchestration with Poise/Serenity, and containerized deployment with Docker Compose.

---

## Architecture Overview

```text
Discord Gateway
       │
       ▼
  [Rust Bot] (Serenity + Poise)
       │
       ├── Configuration & Permissions (`OWNER_ID`, `TARGET_USER_ID`)
       │
       ├── Slash Commands (`/ping`, `/status`)
       │
       ├── Database Layer (SQLx Connection Pool)
       │         │
       │         ▼
       │   [PostgreSQL 16 + pgvector]
       │
       ▼ (Future Phases)
  Message Ingestion Pipeline ──► Semantic Chunking ──► Embedding Model
                                                            │
                                                            ▼
  Discord Response ◄── LLM Service (llama-server) ◄── pgvector Hybrid Retrieval
```

---

## Current Capabilities

- **Strict Separation of Concerns**: Discord handlers contain zero raw SQL or business logic; all interactions route through dedicated database and permission services.
- **Typed Configuration**: Validates environment variables at boot, redacting sensitive tokens in logs and application errors.
- **PostgreSQL Connection Pool**: Async pooling with SQLx with connection timeouts and pool health tracking.
- **pgvector Readiness**: Automatic initialization of the `vector` extension with schema capability for semantic vector search.
- **Nekotina-Style Anime Actions & Tracking**:
  - Full suite of anime roleplay actions (Kiss, Hug, Pat, Slap, Cuddle, Bite, Dance, Cry, etc.) powered by `nekos.best`.
  - Atomic interaction counters stored in PostgreSQL (`action_counts`), tracking how many times users have interacted.
  - Quick-kiss shortcut commands (`/kc` and `/km`) targeting specified user IDs.
- **Anime Hub & Watchlists (AniList Integration)**:
  - Real-time title search and autocomplete powered by the AniList GraphQL API.
  - `/anime <title>`: Rich anime cards with cover art, synopsis, rating, episode count, genres, and an interactive `[Add to Watchlist]` button.
  - `/watchlist`: Full watchlist tracking (`add`, `view`, `update`, `remove`) with statuses (*Watching*, *Plan to Watch*, *Completed*, *On Hold*, *Dropped*), episode progress, and 1–10 rating scores.
  - `/recommend`: Peer-to-peer recommendation system (`send`, `list`, `sent`) allowing users to suggest anime to each other with personal notes and interactive one-click `[Add to Watchlist]` buttons for the recipient.
- **Discord Slash Commands**:
  - `/ping`: Measures Gateway WebSocket heartbeat and round-trip interaction latency.
  - `/status`: Actively queries PostgreSQL and inspects `pg_extension` for `vector`, displaying live infrastructure health and owner diagnostics.
  - `/anime`: Search and view anime details from AniList.
  - `/watchlist`: Manage and view personal anime watchlists and progress.
  - `/recommend`: Send and receive anime recommendations with custom notes.
  - `/action` & individual action commands: 20 interactive commands (`/kiss`, `/slap`, `/hug`, etc.) and 14 solo emotes.
  - `/kc` & `/km`: Instant kiss shortcuts.
- **Extensible Event Pipeline**: Event dispatching ready for non-blocking message ingestion without requiring client refactoring.
- **Reusable Permissions**: Administrative checks protecting sensitive commands via `OWNER_ID`.
- **Multi-Stage Docker Build**: Minimal Debian slim container with non-root security execution.

---

## Project Structure

```text
discord_ai/
├── docker-compose.yml          # Postgres + pgvector and bot service orchestration
├── .env.example                # Template for environment configuration
├── .gitignore                  # Git exclusions for build artifacts and secrets
├── README.md                   # System documentation
│
├── bot/
│   ├── Cargo.toml              # Rust crate manifest & dependencies
│   ├── Cargo.lock              # Deterministic dependency lockfile
│   ├── Dockerfile              # Multi-stage container build
│   └── src/
│       ├── main.rs             # Application bootstrapper and lifecycle
│       ├── config/
│       │   └── mod.rs          # Typed environment parser and validator
│       ├── discord/
│       │   ├── mod.rs          # Discord module exports and shared AppData
│       │   ├── actions.rs      # nekos.best API client, ActionType enum, and formatting
│       │   ├── client.rs       # Serenity & Poise framework setup and builder
│       │   ├── events.rs       # Event listener with ingestion hooks
│       │   ├── permissions.rs  # Reusable command permission checks
│       │   └── commands/
│       │       ├── mod.rs      # Command registry
│       │       ├── ping.rs     # /ping command
│       │       ├── status.rs   # /status command (PostgreSQL & pgvector checks)
│       │       ├── action.rs   # /action anime roleplay command
│       │       ├── kc.rs       # /kc shortcut kiss command
│       │       └── km.rs       # /km shortcut kiss command
│       ├── db/
│       │   ├── mod.rs          # Database client and connection pool management
│       │   ├── health.rs       # PostgreSQL live query and pgvector verifier
│       │   └── actions.rs      # PostgreSQL atomic interaction count persistence
│       └── error.rs            # Strongly-typed error definitions via thiserror
│
└── database/
    └── init/
        ├── 001_extensions.sql  # Database initialization enabling pgvector
        └── 002_actions.sql     # Action counts table and index migration
```

---

## Prerequisites

- **Docker** (version 24.0+)
- **Docker Compose** (version 2.20+)
- *(Optional for local host development)*:
  - **Rust** 1.82+ (2021 edition)
  - **PostgreSQL 16** with `pgvector`

---

## Environment Configuration

Create a `.env` file in the project root:

```bash
cp .env.example .env
```

Edit `.env` with your credentials:

```env
# Discord Bot Authentication Token (from Discord Developer Portal)
DISCORD_TOKEN=your_bot_token_here

# PostgreSQL Connection String
# Inside Docker, the host is 'postgres'. If running bot locally outside Docker, use 'localhost'.
DATABASE_URL=postgresql://postgres:postgres@postgres:5432/discord_ai

# Discord Snowflake ID of the bot administrator/owner (for privileged commands)
OWNER_ID=123456789012345678

# Discord Snowflake ID of the user whose style will eventually be learned and imitated
TARGET_USER_ID=123456789012345678

# Optional: Dedicated development Discord Server/Guild ID for instant guild command testing
# DEV_GUILD_ID=123456789012345678

# Tracing log filter level (trace, debug, info, warn, error)
RUST_LOG=info
```

> [!TIP]
> **Server vs. Global Commands**: Commands are automatically registered directly to every joined server (and `DEV_GUILD_ID` if specified) for instant availability without Discord propagation delays, as well as globally for direct messages (DMs). When the bot is invited to a new server, commands are dynamically registered on join.

> [!IMPORTANT]
> The bot will fail fast during startup with an actionable error message if any required variable is missing or malformed.

---

## Discord Bot Setup & Intents

### 1. Create Bot Application
1. Go to the [Discord Developer Portal](https://discord.com/developers/applications).
2. Click **New Application** and provide a name.
3. Navigate to the **Bot** tab:
   - Click **Reset Token** to copy your token into `DISCORD_TOKEN` in `.env`.
   - Under **Privileged Gateway Intents**, enable:
     - **Message Content Intent** (Required to collect and read messages for style training)
     - **Server Members Intent** (Optional, recommended for member resolution)

### 2. Generate Invite URL
1. Navigate to **OAuth2** -> **URL Generator**.
2. Select scopes:
   - `bot`
   - `applications.commands` (Required to register `/ping` and `/status` slash commands)
3. Select bot permissions:
   - `Read Messages/View Channels`
   - `Send Messages`
   - `Read Message History`
   - `Embed Links`
4. Copy the generated URL into your browser to invite the bot to your server.

---

## Fast Development Mode (No Rebuilding)

During active development, rebuilding the entire multi-stage release Docker image (`docker compose up --build`) takes 40–50+ seconds. We provide two fast workflows with **incremental builds and hot-reloading**:

### Option A: Local Host Dev (`./dev.sh`) — *Fastest (1–2s Incremental Builds)*

Runs the bot natively on your host machine while PostgreSQL runs in Docker:

```bash
./dev.sh
```

- Automatically ensures the `postgres` Docker container is running.
- Safely stops the background `discord_ai_bot` container to prevent duplicate gateway connections.
- Automatically connects to `localhost:5432` (or configured `POSTGRES_PORT`) even if `.env` specifies `@postgres:` — no manual `.env` editing required!
- **Auto-Reload**: If `cargo-watch` is installed (`cargo install cargo-watch`), the bot automatically hot-reloads on every file save. Otherwise, it runs `cargo run`.

---

### Option B: Docker Dev Mode (`docker-compose.dev.yml`) — *Containerized Hot-Reload*

If you prefer keeping everything inside Docker without local Rust toolchain:

```bash
docker compose -f docker-compose.yml -f docker-compose.dev.yml up
```

- Live-mounts `./bot` directly into the container.
- Persists Cargo build caches in Docker volumes (`cargo_registry`, `bot_target`) so compilation is incremental.
- Uses `cargo watch` inside the container to recompile on file save without rebuilding images!

---

## Production Deployment with Docker Compose

### Start the Application
To build and start both the PostgreSQL database (with `pgvector`) and the Rust bot:

```bash
docker compose up --build -d
```

### View Live Logs
View the bot's structured startup logs:

```bash
docker compose logs -f bot
```

Expected startup sequence:
```text
Starting bot...
Loading configuration...
Connecting to PostgreSQL...
PostgreSQL connected
pgvector available
Connecting to Discord...
Discord connected
Bot ready. Logged in as <BotName> (ID: <BotID>)
```

To view database logs:

```bash
docker compose logs -f postgres
```

### Stop the Application
To gracefully stop the running containers:

```bash
docker compose down
```

---

## Database Volume Management

PostgreSQL data persists in the Docker named volume `postgres_data`.

> [!NOTE]
> Database initialization scripts located in `database/init/` (such as `001_extensions.sql`) execute **only on a fresh database volume** when PostgreSQL is initialized for the first time.

### Reset Development Database
If you modify the initial schema or need to wipe the database and re-run initialization scripts:

```bash
# Stop containers and remove the persistent volume
docker compose down -v

# Rebuild and start fresh
docker compose up --build -d
```

---

## Slash Commands Reference

| Command | Access | Description |
|---|---|---|
| `/ping` | Public | Proves the bot is responsive and displays gateway heartbeat and round-trip latency. |
| `/status` | Public (with Owner Diagnostics) | Live health check querying PostgreSQL and verifying that the `pgvector` extension is active. |
| `/action <action> [target]` | Public | Performs an anime roleplay action (kiss, hug, pat, slap, etc.) with dynamic GIFs and interaction counting. |
| `/kc` | Public | Shortcut alias to send a sweet kiss to user `759834538448388206`. |
| `/km` | Public | Shortcut alias to send a sweet kiss to user `790510722047410208`. |

---

## Planned AI Architecture

The foundation is built to scale into the following discrete stages without modifying core bot architecture:

1. **Discord Ingestion (`ingestion/`)**: Historical message backfilling and streaming collection filtered by `TARGET_USER_ID`.
2. **Conversation Detection (`conversations/`)**: Temporal and contextual message grouping into coherent multi-turn exchanges.
3. **Semantic Chunking & Embedding (`memory/`)**: Vector generation and storage in PostgreSQL using pgvector (`vector` column types and HNSW indexes).
4. **Dataset Builder (`dataset/`)**: Exporting cleaned dialogue pairs for fine-tuning / LoRA.
5. **Decoupled Inference (`ai/`)**: Async HTTP client communicating with an independent `llama.cpp` / `llama-server` instance running a quantized Qwen model, keeping inference isolated from Discord connectivity.
