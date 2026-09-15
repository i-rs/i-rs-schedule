# i-rs-schedule

Self-hosted task scheduler and job runner. Create cron or one-shot tasks that execute HTTP requests or shell commands. REST API, CLI, and web UI included.

## Architecture

```
┌────────────────────────────────────────────────────┐
│                   i-rs-schedule                     │
│  ┌─────────────┐  ┌──────────┐  ┌──────────────┐  │
│  │  Scheduler   │  │ Executor │  │   REST API   │  │
│  │ (DelayQueue) │──│ (HTTP/   │  │  (desirable) │  │
│  │              │  │  Shell)  │  │              │  │
│  └──────┬───────┘  └────┬─────┘  └──────┬───────┘  │
│         │               │               │          │
│         └───────┬───────┘               │          │
│                 ▼                       ▼          │
│           ┌──────────┐          ┌────────────┐     │
│           │  SQLite   │          │  CLI / Web │     │
│           └──────────┘          └────────────┘     │
└────────────────────────────────────────────────────┘
```

Rust workspace: `crates/schedule` (server) + `crates/cli` (CLI). React frontend in `frontend/`.

## Quick Start

```bash
# Terminal 1 — Backend
cargo run -p i-rs-schedule

# Terminal 2 — Frontend (optional)
cd frontend && pnpm install && pnpm dev
# Opens http://localhost:5173, proxies /api → backend

# CLI (without frontend)
cargo run -p i-rs-schedule-cli -- task add \
  --name healthcheck --type http --cron "0 */5 * * * *" \
  --url "https://example.com/health"
```

## Task Types

| Type | Config | Example |
|---|---|---|
| **HTTP** | Method, URL, Headers, Body | `GET https://api.example.com/status` |
| **Shell** | Command | `rm -rf /tmp/cache/*` |

## Scheduling

| Type | Config | Example |
|---|---|---|
| **Cron** | 6-field expression `sec min hour dom month dow` | `0 */5 * * * *` — every 5 minutes |
| **Once** | Delay in seconds | `3600` — fire after 1 hour |

### Cron Examples (6 fields: `sec min hour dom month dow`)

| Expression | Meaning |
|---|---|
| `0 */5 * * * *` | Every 5 minutes |
| `0 0 * * * *` | Every hour at :00 |
| `0 0 0 * * *` | Daily at midnight |
| `0 0 9 * * 1-5` | 9:00 AM Mon–Fri |
| `0 30 2 * * 0` | 2:30 AM every Sunday |

## API

All responses use envelope `{ code: 0, message: "ok", data: ... }`.

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/tasks` | Create task |
| `GET` | `/api/tasks` | List all tasks |
| `GET` | `/api/tasks/:id` | Get task |
| `PUT` | `/api/tasks/:id` | Update task |
| `DELETE` | `/api/tasks/:id` | Delete task |
| `POST` | `/api/tasks/:id/enable` | Enable task |
| `POST` | `/api/tasks/:id/disable` | Disable task |
| `POST` | `/api/tasks/:id/run` | Run task immediately |
| `GET` | `/api/executions?task_id=&limit=` | List executions |
| `GET` | `/api/executions/:id` | Get execution |

## CLI

```bash
i-rs-cli --server http://localhost:3000 task add \
  --name cleanup --type shell --cron "0 0 2 * * *" --cmd "rm -rf /tmp/*"

i-rs-cli task list
i-rs-cli task rm --id <uuid>
```

## Docker

```bash
docker compose up -d --build
# API on http://localhost:3000, data persisted in the scheduler-data volume
```

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `SCHEDULE_DB` | `./data/schedule.db` | SQLite database path |
| `SCHEDULE_PORT` | `3000` | API server port |
| `SCHEDULE_SERVER` | `http://localhost:3000` | CLI server address |
| `SCHEDULE_TOKEN` | *(unset)* | API token; when set, all requests require `Authorization: Bearer <token>` |
| `RETENTION_DAYS` | `30` | Execution history retention; `0` keeps records forever |
| `RUST_LOG` | `info` | Log level (e.g. `i_rs_schedule=debug`) |

## Commands

```bash
# Rust
cargo build                   # Build all crates
cargo run -p i-rs-schedule    # Run scheduler server
cargo clippy --workspace      # Lint
cargo fmt                     # Format

# Frontend
cd frontend
pnpm dev                      # Dev server (proxies /api → :3000)
pnpm build                    # Production build
pnpm lint                     # Lint
```

## Tech Stack

**Backend:** Rust, tokio, desirable, rusqlite, cron, reqwest

**Frontend:** React 19, Vite 8, Tailwind CSS 4, Base UI, React Router, pnpm
