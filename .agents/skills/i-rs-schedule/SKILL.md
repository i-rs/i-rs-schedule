---
name: i-rs-schedule
description: Operate the i-rs-schedule task scheduler through its CLI and REST API — create, update, run, and delete cron or one-shot tasks (HTTP or shell), inspect execution history and stats, preview cron schedules, test notifications, manage auth tokens, and read audit logs. Use whenever the user mentions scheduled tasks, cron jobs, timed/定时任务, task scheduling, triggering or running tasks, checking execution records/logs, or wants anything to happen automatically on a schedule in this project — even if they don't say "i-rs-schedule".
---

# i-rs-schedule CLI

Self-hosted task scheduler: cron or one-shot tasks that execute HTTP requests or shell commands. Three surfaces — a Rust server (REST API on port 3000), this CLI (`crates/cli`), and a React web UI. The CLI is the preferred automation surface and covers every API endpoint.

## Build & run

```bash
cargo build                                    # workspace: crates/schedule (server) + crates/cli
cargo run -p i-rs-schedule                     # start server on 127.0.0.1:3000
CLI=./target/debug/i-rs-schedule-cli           # CLI binary (or cargo run -p i-rs-schedule-cli -- <cmd>)
```

The CLI talks to the server over HTTP:
- `--server <url>` flag or `SCHEDULE_SERVER` env (default `http://localhost:3000`)
- `--token <tok>` flag or `SCHEDULE_TOKEN` env for auth

## Auth

Three modes, resolved server-side:
1. **Open** — no credentials configured: everything works without auth (local dev default).
2. **Static token** — server started with `SCHEDULE_TOKEN=<x>` (or config.toml `token = "x"`): send `Authorization: Bearer <x>`.
3. **Admin login** — server started with `ADMIN_USER`/`ADMIN_PASSWORD` (or config.toml): mint a 30-day session token, then use it as the bearer:

```bash
SCHEDULE_TOKEN=$($CLI auth login --username admin --password pw123 | python3 -c "import json,sys;print(json.load(sys.stdin)['data']['token'])")
export SCHEDULE_TOKEN
```

`/healthz` and `/metrics` never require auth. If a command prints `error (401): unauthorized` and exits 1, credentials are missing or wrong — never retry blindly; check the mode.

## Command reference

All list-style output is a JSON envelope `{code, message, data}`; `code != 0` prints `error (code): msg` to stderr and exits 1 — scripts can rely on exit codes.

### Tasks

```bash
# Create (cron: 6 fields — sec min hour dom mon dow; timezone is an IANA name)
$CLI task add --name nightly-sync --type http \
  --cron "0 0 2 * * *" --timezone "Asia/Shanghai" \
  --url "https://api.example.com/sync" --method POST \
  --headers '{"X-Token":"abc"}' --body '{"full":true}'

$CLI task add --name cleanup --type shell --delay-secs 60 \
  --timeout-secs 30 --max-retries 2 --cmd "rm -rf /tmp/cache"   # --delay-secs implies a one-shot task

# Update: fetch-merge-PUT. Unspecified fields KEEP their current values,
# specified fields replace them. This is the safe way to tweak one thing.
$CLI task update --id <id> --enabled true
$CLI task update --id <id> --timezone "Asia/Shanghai" --timeout-secs 60

$CLI task list [--enabled true|false]     # JSON envelope, data = task array
$CLI task show --id <id>                  # single task incl. next_run_at
$CLI task enable --id <id>                # also re-schedules it
$CLI task disable --id <id>
$CLI task rm --id <id>                    # deletes the task AND its execution history
$CLI task run --id <id>                   # triggers now; synchronous — waits for the result
$CLI task stats --id <id>                 # {total, success, failure, avg_duration_ms}
$CLI task notify-test --id <id>           # sends a test message through the task's channel
```

`task add`/`task update` shared fields: `--type http|shell`, `--cron`/`--delay-secs` (once), `--url/--method/--headers/--body` (HTTP), `--cmd` (shell), `--timezone`, `--timeout-secs` (1–3600, default 30), `--max-retries` (0–10, exponential backoff 30s→…→8m), `--notify-type webhook|feishu|dingtalk --notify-url <url>`, `--enabled`, `--trigger-on-success <id[,id2,…]> --trigger-on success|failure|always`.

### Trigger chains (light orchestration)

A task whose final status matches its `--trigger-on` policy automatically runs the downstream tasks in `--trigger-on-success` (each downstream independently retries/notifies, chain depth capped at 10; cycles are rejected with 400 at save time).

Downstream shell commands can interpolate the upstream result:
```bash
$CLI task add --name notify --type shell --trigger-on failure \
  --cmd 'curl -X POST https://hooks.example.com -d "task said: {{trigger.output}}"'
```
`{{trigger.output}}` (truncated to 10k chars) and `{{trigger.status}}` are substituted before execution — quote them yourself if the output may contain shell metacharacters.

### Executions

```bash
$CLI exec list [--task-id <id>] [--limit 50]     # newest first
$CLI exec show --id <exec_id>                    # full record incl. output
```

### Scheduling & ops utilities

```bash
$CLI cron preview --expr "0 0 9 * * *" --timezone "Asia/Shanghai"   # next 5 fire times (UTC ISO)
$CLI task export                       # prints {version, tasks} JSON — redirect to a file
$CLI task import --file tasks.json     # skips ids that already exist; reports {imported, skipped}
$CLI auth login --username admin --password pw123    # mint a session token
$CLI auth audit --limit 50             # audit trail (logins, task CRUD, runs)
$CLI token list / token create --name ci / token revoke --id <id>   # API tokens (plaintext shown once)
```

## Gotchas

- Cron expressions have **6 fields** (seconds first): `0 */5 * * * *` = every 5 minutes. Invalid expressions are rejected at create/update with 400.
- `--once` tasks use `--delay-secs` counted from creation; expired ones are skipped at startup (logged, not run).
- Task `update` requires the task to exist; `add` creates a new one. Don't use `add` to mutate.
- HTTP tasks have a per-task timeout (`--timeout-secs`, default 30s); shell tasks share the same limit. On timeout the execution is marked `failure` with a clear message.
- Global notification fallback: server-side `NOTIFY_TYPE`/`NOTIFY_URL` apply to tasks that have no own channel configured.

## When the CLI is not enough

Everything above is also a REST endpoint (`/api/tasks`, `/api/executions`, `/api/stats/daily`, `/api/audit`, …). For bulk operations not covered by the CLI (e.g., listing every execution of the last 14 days for charting), call the API directly with `curl` + `SCHEDULE_TOKEN`. The API envelope pattern: success = `{code: 0, data: …}`, failure = `{code: 4xx/5xx, message}`.
