# Scheduled Task System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a tokio-based scheduled task system with SQLite persistence, desirable web API, and CLI client.

**Architecture:** Workspace with two crates. `schedule` crate runs a DelayQueue-based scheduler that fires cron and one-shot tasks (HTTP callbacks or shell commands), exposes REST API via desirable. `cli` crate is a standalone HTTP client that talks to the schedule API.

**Tech Stack:** Rust edition 2024, tokio + tokio-util (DelayQueue), desirable (hyper-based web framework), rusqlite (bundled), reqwest, cron, clap, serde/serde_json, uuid, chrono, tracing

---

## File Structure

```
i-rs-schedule/
├── Cargo.toml                    # workspace root
├── AGENTS.md                     # (exists, may update)
├── .gitignore                    # (exists: /target)
├── src/                          # (DELETE: old hello world)
├── Cargo.lock                    # (auto-generated)
└── crates/
    ├── schedule/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── main.rs           # startup wiring
    │       ├── db.rs             # Sqlite init, task CRUD, execution CRUD
    │       ├── executor.rs       # HTTP callback + shell command execution
    │       ├── scheduler.rs      # DelayQueue loop, control channel
    │       └── api.rs            # desirable router, all REST endpoints
    └── cli/
        ├── Cargo.toml
        └── src/
            └── main.rs           # clap CLI, HTTP requests to schedule
```

---

### Task 1: Workspace Restructure

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/schedule/src/main.rs`
- Delete: `src/main.rs` (old hello world)

- [ ] **Step 1: Convert root Cargo.toml to workspace**

Replace `Cargo.toml`:

```toml
[workspace]
members = ["crates/schedule", "crates/cli"]
resolver = "2"
```

- [ ] **Step 2: Move existing main.rs to schedule crate**

```bash
mkdir -p crates/schedule/src
mv src/main.rs crates/schedule/src/main.rs
rmdir src
```

- [ ] **Step 3: Verify workspace compiles (will fail — no Cargo.toml for schedule yet, expected)**

Run: `cargo build 2>&1`
Expected: error about missing `crates/schedule/Cargo.toml`

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "chore: restructure to workspace"
```

---

### Task 2: Schedule Crate Dependencies

**Files:**
- Create: `crates/schedule/Cargo.toml`

- [ ] **Step 1: Create Cargo.toml with all dependencies**

Write `crates/schedule/Cargo.toml`:

```toml
[package]
name = "schedule"
version = "0.1.0"
edition = "2024"

[dependencies]
desirable = "1.1"
tokio = { version = "1", features = ["full"] }
tokio-util = { version = "0.7", features = ["time"] }
rusqlite = { version = "0.35", features = ["bundled"] }
reqwest = { version = "0.12", features = ["json"] }
cron = "0.15"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
chrono = "0.4"
tracing = "0.1"
tracing-subscriber = "0.3"
anyhow = "1"
```

- [ ] **Step 2: Verify fetch compiles**

Run: `cargo build -p schedule`
Expected: SUCCESS (compiles hello world main.rs)

- [ ] **Step 3: Commit**

```bash
git add crates/schedule/Cargo.toml crates/schedule/src/main.rs Cargo.lock && git commit -m "feat(schedule): add dependencies"
```

---

### Task 3: Database Layer (db.rs)

**Files:**
- Create: `crates/schedule/src/db.rs`

- [ ] **Step 1: Write db.rs with schema init and Task/Execution types**

Write `crates/schedule/src/db.rs`:

```rust
use anyhow::Context;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub task_type: TaskType,
    pub enabled: bool,
    pub schedule: ScheduleConfig,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskType {
    Http {
        method: String,
        url: String,
        headers: Option<serde_json::Value>,
        body: Option<String>,
    },
    Shell {
        cmd: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScheduleConfig {
    Cron { expr: String },
    Once { delay_secs: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecution {
    pub id: String,
    pub task_id: String,
    pub status: String,
    pub output: Option<String>,
    pub http_status: Option<i64>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

impl Task {
    pub fn new(name: String, task_type: TaskType, schedule: ScheduleConfig) -> Self {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            task_type,
            enabled: true,
            schedule,
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn new(db_path: &str) -> anyhow::Result<Self> {
        if let Some(parent) = Path::new(db_path).parent() {
            std::fs::create_dir_all(parent).context("create db parent dir")?;
        }
        let conn = Connection::open(db_path).context("open sqlite db")?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .context("set sqlite pragmas")?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS tasks (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                task_type   TEXT NOT NULL DEFAULT 'http',
                enabled     INTEGER NOT NULL DEFAULT 1,
                schedule_type TEXT NOT NULL DEFAULT 'cron',
                cron_expr   TEXT,
                delay_secs  INTEGER,
                http_method TEXT DEFAULT 'GET',
                http_url    TEXT,
                http_headers TEXT,
                http_body   TEXT,
                shell_cmd   TEXT,
                created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS task_executions (
                id          TEXT PRIMARY KEY,
                task_id     TEXT NOT NULL,
                status      TEXT NOT NULL,
                output      TEXT,
                http_status INTEGER,
                started_at  TEXT NOT NULL DEFAULT (datetime('now')),
                finished_at TEXT,
                FOREIGN KEY (task_id) REFERENCES tasks(id)
            );
            ",
        )
        .context("init sqlite schema")?;
        Ok(())
    }

    fn row_to_task(row: &rusqlite::Row) -> rusqlite::Result<Task> {
        let id: String = row.get("id")?;
        let name: String = row.get("name")?;
        let task_type_str: String = row.get("task_type")?;
        let enabled: bool = row.get::<_, i64>("enabled")? != 0;
        let schedule_type: String = row.get("schedule_type")?;
        let cron_expr: Option<String> = row.get("cron_expr")?;
        let delay_secs: Option<i64> = row.get("delay_secs")?;
        let http_method: Option<String> = row.get("http_method")?;
        let http_url: Option<String> = row.get("http_url")?;
        let http_headers: Option<String> = row.get("http_headers")?;
        let http_body: Option<String> = row.get("http_body")?;
        let shell_cmd: Option<String> = row.get("shell_cmd")?;
        let created_at: String = row.get("created_at")?;
        let updated_at: String = row.get("updated_at")?;

        let task_type = match task_type_str.as_str() {
            "shell" => TaskType::Shell {
                cmd: shell_cmd.unwrap_or_default(),
            },
            _ => TaskType::Http {
                method: http_method.unwrap_or_else(|| "GET".into()),
                url: http_url.unwrap_or_default(),
                headers: http_headers
                    .and_then(|h| serde_json::from_str(&h).ok())
                    .unwrap_or(serde_json::Value::Null),
                body: http_body,
            },
        };

        let schedule = match schedule_type.as_str() {
            "once" => ScheduleConfig::Once {
                delay_secs: delay_secs.unwrap_or(0) as u64,
            },
            _ => ScheduleConfig::Cron {
                expr: cron_expr.unwrap_or_default(),
            },
        };

        Ok(Task {
            id,
            name,
            task_type,
            enabled,
            schedule,
            created_at,
            updated_at,
        })
    }

    fn row_to_execution(row: &rusqlite::Row) -> rusqlite::Result<TaskExecution> {
        Ok(TaskExecution {
            id: row.get("id")?,
            task_id: row.get("task_id")?,
            status: row.get("status")?,
            output: row.get("output")?,
            http_status: row.get("http_status")?,
            started_at: row.get("started_at")?,
            finished_at: row.get("finished_at")?,
        })
    }

    pub async fn create_task(&self, task: &Task) -> anyhow::Result<()> {
        let conn = self.conn.clone();
        let task = task.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let conn = conn.lock().unwrap();
            let (schedule_type, cron_expr, delay_secs, http_method, http_url, http_headers, http_body, shell_cmd) =
                match &task.schedule {
                    ScheduleConfig::Cron { expr } => ("cron", Some(expr.as_str()), None, None, None, None, None, None),
                    ScheduleConfig::Once { delay_secs } => ("once", None, Some(*delay_secs as i64), None, None, None, None, None),
                };
            let (task_type_str, method, url, headers, body, cmd) = match &task.task_type {
                TaskType::Http { method, url, headers, body } => {
                    let h = headers.as_ref().and_then(|v| serde_json::to_string(v).ok());
                    ("http", Some(method.as_str()), Some(url.as_str()), h, body.as_deref(), None)
                }
                TaskType::Shell { cmd } => ("shell", None, None, None, None, Some(cmd.as_str())),
            };
            let actual_method = method.unwrap_or("GET");
            let actual_url = url.unwrap_or("");
            let actual_cmd = cmd.unwrap_or("");

            conn.execute(
                "INSERT INTO tasks (id, name, task_type, enabled, schedule_type, cron_expr, delay_secs,
                 http_method, http_url, http_headers, http_body, shell_cmd, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    task.id, task.name, task_type_str, task.enabled as i64, schedule_type,
                    cron_expr, delay_secs,
                    actual_method, actual_url, headers, body,
                    actual_cmd,
                    task.created_at, task.updated_at,
                ],
            )?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    pub async fn list_enabled_tasks(&self) -> anyhow::Result<Vec<Task>> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<Task>> {
            let conn = conn.lock().unwrap();
            let mut stmt = conn.prepare("SELECT * FROM tasks WHERE enabled = 1")?;
            let tasks = stmt
                .query_map([], |row| Self::row_to_task(row))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(tasks)
        })
        .await??
    }

    pub async fn list_all_tasks(&self) -> anyhow::Result<Vec<Task>> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<Task>> {
            let conn = conn.lock().unwrap();
            let mut stmt = conn.prepare("SELECT * FROM tasks ORDER BY created_at DESC")?;
            let tasks = stmt
                .query_map([], |row| Self::row_to_task(row))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(tasks)
        })
        .await??
    }

    pub async fn get_task(&self, id: &str) -> anyhow::Result<Option<Task>> {
        let conn = self.conn.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Option<Task>> {
            let conn = conn.lock().unwrap();
            let mut stmt = conn.prepare("SELECT * FROM tasks WHERE id = ?1")?;
            let mut rows = stmt.query_map(params![&id], |row| Self::row_to_task(row))?;
            match rows.next() {
                Some(Ok(task)) => Ok(Some(task)),
                Some(Err(e)) => Err(e.into()),
                None => Ok(None),
            }
        })
        .await??
    }

    pub async fn update_task(&self, task: &Task) -> anyhow::Result<()> {
        let conn = self.conn.clone();
        let task = task.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let conn = conn.lock().unwrap();
            let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
            let (schedule_type, cron_expr, delay_secs) = match &task.schedule {
                ScheduleConfig::Cron { expr } => ("cron", Some(expr.as_str()), None),
                ScheduleConfig::Once { delay_secs } => ("once", None, Some(*delay_secs as i64)),
            };
            let (task_type_str, method, url, headers, body, cmd) = match &task.task_type {
                TaskType::Http { method, url, headers, body } => {
                    let h = headers.as_ref().and_then(|v| serde_json::to_string(v).ok());
                    ("http", Some(method.as_str()), Some(url.as_str()), h, body.as_deref(), None)
                }
                TaskType::Shell { cmd } => ("shell", None, None, None, None, Some(cmd.as_str())),
            };
            conn.execute(
                "UPDATE tasks SET name=?1, task_type=?2, enabled=?3, schedule_type=?4, cron_expr=?5,
                 delay_secs=?6, http_method=?7, http_url=?8, http_headers=?9, http_body=?10,
                 shell_cmd=?11, updated_at=?12 WHERE id=?13",
                params![
                    task.name, task_type_str, task.enabled as i64, schedule_type, cron_expr, delay_secs,
                    method.unwrap_or("GET"), url.unwrap_or(""), headers, body,
                    cmd.unwrap_or(""),
                    now, task.id,
                ],
            )?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    pub async fn delete_task(&self, id: &str) -> anyhow::Result<bool> {
        let conn = self.conn.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> anyhow::Result<bool> {
            let conn = conn.lock().unwrap();
            let affected = conn.execute("DELETE FROM tasks WHERE id = ?1", params![&id])?;
            Ok(affected > 0)
        })
        .await??
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> anyhow::Result<bool> {
        let conn = self.conn.clone();
        let id = id.to_string();
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        tokio::task::spawn_blocking(move || -> anyhow::Result<bool> {
            let conn = conn.lock().unwrap();
            let affected = conn.execute(
                "UPDATE tasks SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
                params![enabled as i64, now, &id],
            )?;
            Ok(affected > 0)
        })
        .await??
    }

    pub async fn create_execution(&self, exec: &TaskExecution) -> anyhow::Result<()> {
        let conn = self.conn.clone();
        let exec = exec.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let conn = conn.lock().unwrap();
            conn.execute(
                "INSERT INTO task_executions (id, task_id, status, output, http_status, started_at, finished_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    exec.id, exec.task_id, exec.status, exec.output, exec.http_status,
                    exec.started_at, exec.finished_at,
                ],
            )?;
            Ok(())
        })
        .await??
    }

    pub async fn update_execution(
        &self,
        exec_id: &str,
        status: &str,
        output: &str,
        http_status: Option<i64>,
    ) -> anyhow::Result<()> {
        let conn = self.conn.clone();
        let exec_id = exec_id.to_string();
        let status = status.to_string();
        let output = output.to_string();
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let conn = conn.lock().unwrap();
            conn.execute(
                "UPDATE task_executions SET status=?1, output=?2, http_status=?3, finished_at=?4 WHERE id=?5",
                params![status, output, http_status, now, exec_id],
            )?;
            Ok(())
        })
        .await??
    }

    pub async fn list_executions(&self, task_id: Option<&str>, limit: Option<u32>) -> anyhow::Result<Vec<TaskExecution>> {
        let conn = self.conn.clone();
        let task_id = task_id.map(String::from);
        let limit = limit.unwrap_or(50) as i64;
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<TaskExecution>> {
            let conn = conn.lock().unwrap();
            let rows = if let Some(ref tid) = task_id {
                let mut stmt = conn.prepare(
                    "SELECT * FROM task_executions WHERE task_id = ?1 ORDER BY started_at DESC LIMIT ?2"
                )?;
                stmt.query_map(params![tid, limit], |row| Self::row_to_execution(row))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            } else {
                let mut stmt = conn.prepare(
                    "SELECT * FROM task_executions ORDER BY started_at DESC LIMIT ?1"
                )?;
                stmt.query_map(params![limit], |row| Self::row_to_execution(row))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            Ok(rows)
        })
        .await??
    }

    pub async fn get_execution(&self, id: &str) -> anyhow::Result<Option<TaskExecution>> {
        let conn = self.conn.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Option<TaskExecution>> {
            let conn = conn.lock().unwrap();
            let mut stmt = conn.prepare("SELECT * FROM task_executions WHERE id = ?1")?;
            let mut rows = stmt.query_map(params![&id], |row| Self::row_to_execution(row))?;
            match rows.next() {
                Some(Ok(exec)) => Ok(Some(exec)),
                Some(Err(e)) => Err(e.into()),
                None => Ok(None),
            }
        })
        .await??
    }
}
```

- [ ] **Step 2: Verify compiles**

Run: `cargo build -p schedule`
Expected: SUCCESS (with dead code warnings)

- [ ] **Step 3: Commit**

```bash
git add crates/schedule/src/db.rs && git commit -m "feat(schedule): add db layer with SQLite schema and CRUD"
```

---

### Task 4: Executor (executor.rs)

**Files:**
- Create: `crates/schedule/src/executor.rs`

- [ ] **Step 1: Write executor.rs**

Write `crates/schedule/src/executor.rs`:

```rust
use crate::db::{Task, TaskType};

pub struct ExecutionResult {
    pub status: String,
    pub output: String,
    pub http_status: Option<i64>,
}

pub struct Executor;

impl Executor {
    pub async fn execute(&self, task: &Task) -> ExecutionResult {
        match &task.task_type {
            TaskType::Http {
                method,
                url,
                headers,
                body,
            } => Self::execute_http(method, url, headers, body.as_deref()).await,
            TaskType::Shell { cmd } => Self::execute_shell(cmd).await,
        }
    }

    async fn execute_http(
        method: &str,
        url: &str,
        headers: &Option<serde_json::Value>,
        body: Option<&str>,
    ) -> ExecutionResult {
        let client = reqwest::Client::new();
        let mut req = match method.to_uppercase().as_str() {
            "GET" => client.get(url),
            "POST" => client.post(url),
            "PUT" => client.put(url),
            "DELETE" => client.delete(url),
            _ => client.get(url),
        };

        if let Some(serde_json::Value::Object(map)) = headers {
            for (k, v) in map {
                if let Some(val) = v.as_str() {
                    req = req.header(k.as_str(), val);
                }
            }
        }

        if let Some(b) = body {
            req = req.body(b.to_string());
        }

        match req.send().await {
            Ok(resp) => {
                let http_status = resp.status().as_u16() as i64;
                let output = resp.text().await.unwrap_or_default();
                let status = if resp.status().is_success() {
                    "success"
                } else {
                    "failure"
                };
                ExecutionResult {
                    status: status.to_string(),
                    output,
                    http_status: Some(http_status),
                }
            }
            Err(e) => ExecutionResult {
                status: "failure".to_string(),
                output: e.to_string(),
                http_status: None,
            },
        }
    }

    async fn execute_shell(cmd: &str) -> ExecutionResult {
        match tokio::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .output()
            .await
        {
            Ok(out) => {
                let status = if out.status.success() {
                    "success"
                } else {
                    "failure"
                };
                let output = if out.status.success() {
                    String::from_utf8_lossy(&out.stdout).to_string()
                } else {
                    String::from_utf8_lossy(&out.stderr).to_string()
                };
                ExecutionResult {
                    status: status.to_string(),
                    output,
                    http_status: None,
                }
            }
            Err(e) => ExecutionResult {
                status: "failure".to_string(),
                output: e.to_string(),
                http_status: None,
            },
        }
    }
}
```

- [ ] **Step 2: Verify compiles**

Run: `cargo build -p schedule`
Expected: SUCCESS

- [ ] **Step 3: Commit**

```bash
git add crates/schedule/src/executor.rs && git commit -m "feat(schedule): add executor for HTTP callback and shell command tasks"
```

---

### Task 5: Scheduler (scheduler.rs)

**Files:**
- Create: `crates/schedule/src/scheduler.rs`

- [ ] **Step 1: Write scheduler.rs**

Write `crates/schedule/src/scheduler.rs`:

```rust
use crate::db::{Db, ScheduleConfig, Task, TaskExecution};
use crate::executor::{ExecutionResult, Executor};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::time::delay_queue::{self, DelayQueue};
use uuid::Uuid;

pub enum ControlCmd {
    Add(Task),
    Remove(String),
    Update(Task),
}

pub struct Scheduler {
    queue: DelayQueue<Task>,
    keys: HashMap<String, delay_queue::Key>,
}

impl ScheduleConfig {
    pub fn next_delay(&self) -> Duration {
        match self {
            ScheduleConfig::Cron { expr } => {
                match expr.parse::<cron::Schedule>() {
                    Ok(schedule) => {
                        let now = chrono::Utc::now();
                        match schedule.upcoming(chrono::Utc).next() {
                            Some(next) => {
                                let delta = (next - now).num_milliseconds().max(0);
                                Duration::from_millis(delta as u64)
                            }
                            None => Duration::from_secs(3600),
                        }
                    }
                    Err(_) => Duration::from_secs(3600),
                }
            }
            ScheduleConfig::Once { delay_secs } => Duration::from_secs(*delay_secs),
        }
    }

    pub fn remaining_delay(&self, created_at: &str) -> Option<Duration> {
        match self {
            ScheduleConfig::Once { delay_secs } => {
                if let Ok(created) = chrono::NaiveDateTime::parse_from_str(
                    created_at,
                    "%Y-%m-%dT%H:%M:%S%.3fZ",
                )
                .or_else(|_| {
                    chrono::NaiveDateTime::parse_from_str(created_at, "%Y-%m-%dT%H:%M:%S%.fZ")
                }) {
                    let created_utc = created.and_utc();
                    let now = chrono::Utc::now();
                    let elapsed = (now - created_utc).num_seconds().max(0) as u64;
                    if elapsed >= *delay_secs {
                        None // already fired
                    } else {
                        Some(Duration::from_secs(*delay_secs - elapsed))
                    }
                } else {
                    Some(Duration::from_secs(*delay_secs))
                }
            }
            ScheduleConfig::Cron { .. } => Some(self.next_delay()),
        }
    }
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            queue: DelayQueue::new(),
            keys: HashMap::new(),
        }
    }

    pub fn load_tasks(&mut self, tasks: Vec<Task>) {
        for task in tasks {
            if let Some(delay) = task.schedule.remaining_delay(&task.created_at) {
                let key = self.queue.insert(task.clone(), delay);
                self.keys.insert(task.id.clone(), key);
            }
        }
    }

    pub fn insert(&mut self, task: Task) {
        let delay = task.schedule.next_delay();
        let key = self.queue.insert(task.clone(), delay);
        self.keys.insert(task.id.clone(), key);
    }

    pub fn remove(&mut self, id: &str) {
        if let Some(key) = self.keys.remove(id) {
            self.queue.remove(&key);
        }
    }

    pub fn update(&mut self, task: Task) {
        self.remove(&task.id);
        if task.enabled {
            self.insert(task);
        }
    }

    pub async fn run(
        mut self,
        mut cmd_rx: mpsc::UnboundedReceiver<ControlCmd>,
        executor: Arc<Executor>,
        db: Db,
    ) {
        loop {
            tokio::select! {
                Some(expired) = self.queue.next() => {
                    let task = expired.into_inner();
                    // Once tasks are removed after firing
                    if matches!(task.schedule, ScheduleConfig::Once { .. }) {
                        self.keys.remove(&task.id);
                    }

                    let exec = executor.clone();
                    let db = db.clone();
                    let task_clone = task.clone();

                    tokio::spawn(async move {
                        let exec_id = Uuid::new_v4().to_string();
                        let started_at = chrono::Utc::now()
                            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                            .to_string();

                        let _ = db.create_execution(&TaskExecution {
                            id: exec_id.clone(),
                            task_id: task_clone.id.clone(),
                            status: "running".to_string(),
                            output: None,
                            http_status: None,
                            started_at,
                            finished_at: None,
                        }).await;

                        let result = exec.execute(&task_clone).await;

                        let _ = db.update_execution(
                            &exec_id,
                            &result.status,
                            &result.output,
                            result.http_status,
                        ).await;
                    });
                }

                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        ControlCmd::Add(task) => self.insert(task),
                        ControlCmd::Remove(id) => self.remove(&id),
                        ControlCmd::Update(task) => self.update(task),
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 2: Verify compiles**

Run: `cargo build -p schedule`
Expected: SUCCESS

- [ ] **Step 3: Commit**

```bash
git add crates/schedule/src/scheduler.rs && git commit -m "feat(schedule): add DelayQueue-based scheduler with control channel"
```

---

### Task 6: API Router (api.rs)

**Files:**
- Create: `crates/schedule/src/api.rs`

- [ ] **Step 1: Write api.rs**

Write `crates/schedule/src/api.rs`:

```rust
use crate::db::{Db, ScheduleConfig, Task, TaskExecution, TaskType};
use crate::scheduler::ControlCmd;
use desirable::{IntoResponse, Request, Response, Router};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::mpsc;

fn json_response<T: serde::Serialize>(data: T) -> Response {
    Response::builder()
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&data).unwrap_or_default().into())
        .unwrap()
}

fn error_response(status: u16, message: &str) -> Response {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(
            serde_json::json!({ "error": message })
                .to_string()
                .into(),
        )
        .unwrap()
}

#[derive(Deserialize)]
struct CreateTaskRequest {
    name: String,
    task_type: Option<String>,
    schedule_type: Option<String>,
    cron_expr: Option<String>,
    delay_secs: Option<u64>,
    http_method: Option<String>,
    http_url: Option<String>,
    http_headers: Option<serde_json::Value>,
    http_body: Option<String>,
    shell_cmd: Option<String>,
}

pub fn build_router(db: Db, cmd_tx: mpsc::UnboundedSender<ControlCmd>) -> Router {
    let db = Arc::new(db);
    let cmd_tx = Arc::new(cmd_tx);

    let mut router = Router::new();

    let db_post = db.clone();
    let tx_post = cmd_tx.clone();
    router.post("/api/tasks", move |mut req: Request| {
        let db = db_post.clone();
        let tx = tx_post.clone();
        async move {
            let body: CreateTaskRequest = match req.body_json().await {
                Ok(b) => b,
                Err(e) => return error_response(400, &format!("invalid body: {e}")),
            };

            let task_type = match body.task_type.as_deref() {
                Some("shell") => TaskType::Shell {
                    cmd: body.shell_cmd.unwrap_or_default(),
                },
                _ => TaskType::Http {
                    method: body.http_method.unwrap_or_else(|| "GET".into()),
                    url: body.http_url.unwrap_or_default(),
                    headers: body.http_headers,
                    body: body.http_body,
                },
            };

            let schedule = match body.schedule_type.as_deref() {
                Some("once") => ScheduleConfig::Once {
                    delay_secs: body.delay_secs.unwrap_or(0),
                },
                _ => ScheduleConfig::Cron {
                    expr: body.cron_expr.unwrap_or_default(),
                },
            };

            let task = Task::new(body.name, task_type, schedule);
            if let Err(e) = db.create_task(&task).await {
                return error_response(500, &format!("db error: {e}"));
            }
            let _ = tx.send(ControlCmd::Add(task.clone()));
            json_response(&task)
        }
    });

    let db_get_all = db.clone();
    router.get("/api/tasks", move |_req: Request| {
        let db = db_get_all.clone();
        async move {
            match db.list_all_tasks().await {
                Ok(tasks) => json_response(&tasks),
                Err(e) => error_response(500, &format!("db error: {e}")),
            }
        }
    });

    let db_get_one = db.clone();
    router.get("/api/tasks/:id", move |req: Request| {
        let db = db_get_one.clone();
        async move {
            let id = match req.param("id") {
                Some(id) => id,
                None => return error_response(400, "missing id"),
            };
            match db.get_task(id).await {
                Ok(Some(task)) => json_response(&task),
                Ok(None) => error_response(404, "not found"),
                Err(e) => error_response(500, &format!("db error: {e}")),
            }
        }
    });

    let db_update = db.clone();
    let tx_update = cmd_tx.clone();
    router.put("/api/tasks/:id", move |mut req: Request| {
        let db = db_update.clone();
        let tx = tx_update.clone();
        async move {
            let id = match req.param("id") {
                Some(id) => id.to_string(),
                None => return error_response(400, "missing id"),
            };
            let body: CreateTaskRequest = match req.body_json().await {
                Ok(b) => b,
                Err(e) => return error_response(400, &format!("invalid body: {e}")),
            };

            let task_type = match body.task_type.as_deref() {
                Some("shell") => TaskType::Shell {
                    cmd: body.shell_cmd.unwrap_or_default(),
                },
                _ => TaskType::Http {
                    method: body.http_method.unwrap_or_else(|| "GET".into()),
                    url: body.http_url.unwrap_or_default(),
                    headers: body.http_headers,
                    body: body.http_body,
                },
            };

            let schedule = match body.schedule_type.as_deref() {
                Some("once") => ScheduleConfig::Once {
                    delay_secs: body.delay_secs.unwrap_or(0),
                },
                _ => ScheduleConfig::Cron {
                    expr: body.cron_expr.unwrap_or_default(),
                },
            };

            let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
            let task = Task {
                id,
                name: body.name,
                task_type,
                enabled: true,
                schedule,
                created_at: now.clone(),
                updated_at: now,
            };

            if let Err(e) = db.update_task(&task).await {
                return error_response(500, &format!("db error: {e}"));
            }
            let _ = tx.send(ControlCmd::Update(task.clone()));
            json_response(&task)
        }
    });

    let db_delete = db.clone();
    let tx_delete = cmd_tx.clone();
    router.delete("/api/tasks/:id", move |req: Request| {
        let db = db_delete.clone();
        let tx = tx_delete.clone();
        async move {
            let id = match req.param("id") {
                Some(id) => id.to_string(),
                None => return error_response(400, "missing id"),
            };
            match db.delete_task(&id).await {
                Ok(true) => {
                    let _ = tx.send(ControlCmd::Remove(id));
                    json_response(&serde_json::json!({ "deleted": true }))
                }
                Ok(false) => error_response(404, "not found"),
                Err(e) => error_response(500, &format!("db error: {e}")),
            }
        }
    });

    let db_enable = db.clone();
    let tx_enable = cmd_tx.clone();
    router.post("/api/tasks/:id/enable", move |req: Request| {
        let db = db_enable.clone();
        let tx = tx_enable.clone();
        async move {
            let id = match req.param("id") {
                Some(id) => id.to_string(),
                None => return error_response(400, "missing id"),
            };
            match db.set_enabled(&id, true).await {
                Ok(true) => {
                    if let Ok(Some(task)) = db.get_task(&id).await {
                        let _ = tx.send(ControlCmd::Add(task));
                    }
                    json_response(&serde_json::json!({ "enabled": true }))
                }
                Ok(false) => error_response(404, "not found"),
                Err(e) => error_response(500, &format!("db error: {e}")),
            }
        }
    });

    let db_disable = db.clone();
    let tx_disable = cmd_tx.clone();
    router.post("/api/tasks/:id/disable", move |req: Request| {
        let db = db_disable.clone();
        let tx = tx_disable.clone();
        async move {
            let id = match req.param("id") {
                Some(id) => id.to_string(),
                None => return error_response(400, "missing id"),
            };
            match db.set_enabled(&id, false).await {
                Ok(true) => {
                    let _ = tx.send(ControlCmd::Remove(id));
                    json_response(&serde_json::json!({ "enabled": false }))
                }
                Ok(false) => error_response(404, "not found"),
                Err(e) => error_response(500, &format!("db error: {e}")),
            }
        }
    });

    let db_exec_list = db.clone();
    router.get("/api/executions", move |req: Request| {
        let db = db_exec_list.clone();
        async move {
            let task_id = req.query("task_id");
            let limit = req
                .query("limit")
                .and_then(|s: &str| s.parse::<u32>().ok());
            match db.list_executions(task_id, limit).await {
                Ok(execs) => json_response(&execs),
                Err(e) => error_response(500, &format!("db error: {e}")),
            }
        }
    });

    let db_exec_one = db.clone();
    router.get("/api/executions/:id", move |req: Request| {
        let db = db_exec_one.clone();
        async move {
            let id = match req.param("id") {
                Some(id) => id,
                None => return error_response(400, "missing id"),
            };
            match db.get_execution(id).await {
                Ok(Some(exec)) => json_response(&exec),
                Ok(None) => error_response(404, "not found"),
                Err(e) => error_response(500, &format!("db error: {e}")),
            }
        }
    });

    router
}
```

- [ ] **Step 2: Verify compiles**

Run: `cargo build -p schedule`
Expected: SUCCESS

- [ ] **Step 3: Commit**

```bash
git add crates/schedule/src/api.rs && git commit -m "feat(schedule): add REST API routes for task and execution management"
```

---

### Task 7: Main Entry Point (main.rs)

**Files:**
- Modify: `crates/schedule/src/main.rs`

- [ ] **Step 1: Write main.rs with full wiring**

Write `crates/schedule/src/main.rs`:

```rust
mod api;
mod db;
mod executor;
mod scheduler;

use db::Db;
use executor::Executor;
use scheduler::Scheduler;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let db_path =
        std::env::var("SCHEDULE_DB").unwrap_or_else(|_| "./data/schedule.db".to_string());
    let port = std::env::var("SCHEDULE_PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("127.0.0.1:{port}");

    let db = Db::new(&db_path)?;

    let tasks = db.list_enabled_tasks().await?;
    tracing::info!("loaded {} enabled tasks from db", tasks.len());

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();

    let mut scheduler = Scheduler::new();
    scheduler.load_tasks(tasks);

    let executor = Arc::new(Executor);
    let scheduler_db = db.clone();

    tokio::spawn(async move {
        scheduler.run(cmd_rx, executor, scheduler_db).await;
    });

    let router = api::build_router(db, cmd_tx);

    tracing::info!("starting server on http://{addr}");
    desirable::new(&addr).run(router).await?;

    Ok(())
}
```

- [ ] **Step 2: Build and verify**

Run: `cargo build -p schedule`
Expected: SUCCESS

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -p schedule 2>&1`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add crates/schedule/src/main.rs && git commit -m "feat(schedule): wire up main entry point"
```

---

### Task 8: CLI Crate Setup

**Files:**
- Create: `crates/cli/Cargo.toml`

- [ ] **Step 1: Create CLI Cargo.toml**

Write `crates/cli/Cargo.toml`:

```toml
[package]
name = "i-rs-cli"
version = "0.1.0"
edition = "2024"

[dependencies]
clap = { version = "4", features = ["derive"] }
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = "1"
anyhow = "1"
```

- [ ] **Step 2: Verify compiles (will fail — no main.rs yet, expected)**

Run: `cargo build -p i-rs-cli 2>&1`
Expected: error about missing main.rs

- [ ] **Step 3: Commit**

```bash
git add crates/cli/Cargo.toml Cargo.lock && git commit -m "feat(cli): add crate setup with dependencies"
```

---

### Task 9: CLI Implementation (cli/src/main.rs)

**Files:**
- Create: `crates/cli/src/main.rs`

- [ ] **Step 1: Write CLI main.rs**

Write `crates/cli/src/main.rs`:

```rust
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, Args};
use serde::{Deserialize, Serialize};

#[derive(Parser)]
#[command(name = "i-rs-cli", version, about = "CLI for i-rs-schedule task management")]
struct Cli {
    #[arg(long, default_value = "http://localhost:3000", env = "SCHEDULE_SERVER")]
    server: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(subcommand)]
    Task(TaskCmd),

    #[command(subcommand)]
    Exec(ExecCmd),
}

#[derive(Subcommand)]
enum TaskCmd {
    Add(AddArgs),
    List(ListArgs),
    Show(ShowArgs),
    Rm(RmArgs),
    Enable(IdArgs),
    Disable(IdArgs),
}

#[derive(Subcommand)]
enum ExecCmd {
    List(ExecListArgs),
    Show(IdArgs),
}

#[derive(Args)]
struct AddArgs {
    #[arg(long)]
    name: String,

    #[arg(long, value_parser = ["http", "shell"])]
    r#type: String,

    #[arg(long)]
    cron: Option<String>,

    #[arg(long)]
    delay_secs: Option<u64>,

    #[arg(long)]
    url: Option<String>,

    #[arg(long, default_value = "GET")]
    method: Option<String>,

    #[arg(long)]
    headers: Option<String>,

    #[arg(long)]
    body: Option<String>,

    #[arg(long)]
    cmd: Option<String>,
}

#[derive(Args)]
struct ListArgs {
    #[arg(long)]
    enabled: Option<bool>,
}

#[derive(Args)]
struct ShowArgs {
    #[arg(long)]
    id: String,
}

#[derive(Args)]
struct RmArgs {
    #[arg(long)]
    id: String,
}

#[derive(Args)]
struct IdArgs {
    #[arg(long)]
    id: String,
}

#[derive(Args)]
struct ExecListArgs {
    #[arg(long)]
    task_id: Option<String>,

    #[arg(long, default_value = "50")]
    limit: u32,
}

#[derive(Serialize)]
struct CreateTaskBody {
    name: String,
    task_type: String,
    schedule_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cron_expr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    delay_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_headers: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shell_cmd: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TaskResponse {
    id: String,
    name: String,
    task_type: serde_json::Value,
    enabled: bool,
    schedule: serde_json::Value,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct ExecResponse {
    id: String,
    task_id: String,
    status: String,
    output: Option<String>,
    http_status: Option<i64>,
    started_at: String,
    finished_at: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = reqwest::Client::new();
    let base = &cli.server;

    match cli.command {
        Command::Task(cmd) => match cmd {
            TaskCmd::Add(args) => {
                let schedule_type = if args.delay_secs.is_some() { "once" } else { "cron" };
                let headers_json: Option<serde_json::Value> = args
                    .headers
                    .as_ref()
                    .and_then(|h| serde_json::from_str(h).ok());

                let body = CreateTaskBody {
                    name: args.name,
                    task_type: args.r#type,
                    schedule_type: schedule_type.to_string(),
                    cron_expr: args.cron,
                    delay_secs: args.delay_secs,
                    http_method: args.method,
                    http_url: args.url,
                    http_headers: headers_json,
                    http_body: args.body,
                    shell_cmd: args.cmd,
                };

                let resp = client
                    .post(format!("{base}/api/tasks"))
                    .json(&body)
                    .send()
                    .await
                    .context("failed to create task")?;

                let task: serde_json::Value = resp.json().await?;
                println!("{}", serde_json::to_string_pretty(&task)?);
            }
            TaskCmd::List(args) => {
                let mut url = format!("{base}/api/tasks");
                let resp = client.get(&url).send().await?;
                let tasks: Vec<serde_json::Value> = resp.json().await?;

                let filtered: Vec<&serde_json::Value> = if let Some(enabled) = args.enabled {
                    tasks.iter().filter(|t| t["enabled"].as_bool() == Some(enabled)).collect()
                } else {
                    tasks.iter().collect()
                };

                println!("{}", serde_json::to_string_pretty(&filtered)?);
            }
            TaskCmd::Show(args) => {
                let resp = client
                    .get(format!("{base}/api/tasks/{}", args.id))
                    .send()
                    .await?;
                let task: serde_json::Value = resp.json().await?;
                println!("{}", serde_json::to_string_pretty(&task)?);
            }
            TaskCmd::Rm(args) => {
                let resp = client
                    .delete(format!("{base}/api/tasks/{}", args.id))
                    .send()
                    .await?;
                let result: serde_json::Value = resp.json().await?;
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            TaskCmd::Enable(args) => {
                let resp = client
                    .post(format!("{base}/api/tasks/{}/enable", args.id))
                    .send()
                    .await?;
                let result: serde_json::Value = resp.json().await?;
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            TaskCmd::Disable(args) => {
                let resp = client
                    .post(format!("{base}/api/tasks/{}/disable", args.id))
                    .send()
                    .await?;
                let result: serde_json::Value = resp.json().await?;
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
        },
        Command::Exec(cmd) => match cmd {
            ExecCmd::List(args) => {
                let mut url = format!("{base}/api/executions?limit={}", args.limit);
                if let Some(ref tid) = args.task_id {
                    url.push_str(&format!("&task_id={tid}"));
                }
                let resp = client.get(&url).send().await?;
                let execs: Vec<serde_json::Value> = resp.json().await?;
                println!("{}", serde_json::to_string_pretty(&execs)?);
            }
            ExecCmd::Show(args) => {
                let resp = client
                    .get(format!("{base}/api/executions/{}", args.id))
                    .send()
                    .await?;
                let exec: serde_json::Value = resp.json().await?;
                println!("{}", serde_json::to_string_pretty(&exec)?);
            }
        },
    }

    Ok(())
}
```

- [ ] **Step 2: Verify compiles**

Run: `cargo build -p i-rs-cli`
Expected: SUCCESS

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -p i-rs-cli 2>&1`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add crates/cli/src/main.rs && git commit -m "feat(cli): add full CLI with task and execution management commands"
```

---

### Task 10: Full Build and Verification

**Files:** None (verification only)

- [ ] **Step 1: Build entire workspace**

Run: `cargo build`
Expected: SUCCESS, all crates compile

- [ ] **Step 2: Run clippy on entire workspace**

Run: `cargo clippy --workspace 2>&1`
Expected: No errors, no warnings

- [ ] **Step 3: Run cargo fmt check**

Run: `cargo fmt --check 2>&1`
Expected: No formatting violations (or fix with `cargo fmt`)

- [ ] **Step 4: Run cargo test**

Run: `cargo test --workspace`
Expected: 0 tests run (no test code yet, but verifies compilation)

- [ ] **Step 5: Verify CLI help output**

Run: `cargo run -p i-rs-cli -- --help`
Expected: Full help text with subcommands

- [ ] **Step 6: Commit (if any fmt changes)**

```bash
git add -A && git commit -m "chore: final verification, formatting, and cleanup"
```
