# i-rs-schedule 优化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 对 `crates/schedule` 与 `crates/cli` 做性能、可维护性、健壮性优化,不改 API 路由签名、schema、frontend。

**Architecture:** DB 层从 `Arc<Mutex<Connection>>` 迁移到 `r2d2` 连接池;Executor 复用单个 `reqwest::Client` 并加超时;全代码库消除重复(时间格式串、`build_task`、CLI 样板);统一错误处理与结构化日志;PUT 支持 `enabled` 字段。

**Tech Stack:** Rust 2024, tokio, rusqlite 0.40 + r2d2 0.8 + r2d2_sqlite 0.35(新增), reqwest, desirable, cron, tracing。

**验证约定:** AGENTS.md 声明 "No tests",本项目无测试套件。每个 task 的验证用 `cargo build && cargo clippy && cargo fmt`,在 plan 中记为 VERIFY。每个 task 结尾单独 commit。

---

## File Structure

| 文件 | 改动类型 | 责任 |
|------|----------|------|
| `crates/schedule/Cargo.toml` | 修改 | 新增 `r2d2`, `r2d2_sqlite` 依赖 |
| `crates/schedule/src/db.rs` | 重构 | pool 模型;移除 `private` 模块;新增 `TIMESTAMP_FMT`/`now_iso`/`parse_iso`/`spawn_db`;`CreateTaskRequest` 加 `enabled` 字段相关代码不在此文件 |
| `crates/schedule/src/executor.rs` | 修改 | `Executor` 持有 `reqwest::Client`;新增 `Executor::new()`;HTTP 超时 30s |
| `crates/schedule/src/scheduler.rs` | 修改 | 用 `now_iso`/`TIMESTAMP_FMT`;过期 Once 日志;execution 写库不再静默;结构化日志 |
| `crates/schedule/src/api.rs` | 修改 | `CreateTaskRequest` 加 `enabled`;删除 `build_update_task`,合并到 `build_task`;用 `now_iso` |
| `crates/schedule/src/main.rs` | 修改 | `Executor::new()` |
| `crates/cli/src/main.rs` | 修改 | 新增 `print_response` helper 去样板 |

---

## Task 1: DB 层迁移到 r2d2 连接池

**Files:**
- Modify: `crates/schedule/Cargo.toml`
- Modify: `crates/schedule/src/db.rs:67-149`(`Db` 结构与 `new`/`init_schema`)

- [ ] **Step 1: 添加依赖**

编辑 `crates/schedule/Cargo.toml`,在 `[dependencies]` 末尾追加两行:

```toml
r2d2 = "0.8"
r2d2_sqlite = "0.35"
```

完整 `[dependencies]` 段应为:

```toml
[dependencies]
desirable = "1.2"
tokio = { version = "1", features = ["full"] }
tokio-util = { version = "0.7", features = ["time"] }
rusqlite = { version = "0.40.1", features = ["bundled"] }
reqwest = { version = "0.13.4", features = ["json"] }
cron = "0.17.0"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
chrono = "0.4"
tracing = "0.1"
tracing-subscriber = "0.3"
anyhow = "1"
futures-util = "0.3"
r2d2 = "0.8"
r2d2_sqlite = "0.35"
```

- [ ] **Step 2: 改写 `Db` 结构与构造函数**

替换 `crates/schedule/src/db.rs:67-186`(`#[derive(Clone)] pub struct Db` 到 `init_schema` 结束)为:

```rust
#[derive(Clone)]
pub struct Db {
    pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
}

#[derive(Debug)]
struct SqliteInit;

impl r2d2::CustomizeConnection<Connection, rusqlite::Error> for SqliteInit {
    fn on_acquire(&self, conn: &mut Connection) -> Result<(), rusqlite::Error> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(())
    }
}

/// 在 spawn_blocking 中执行一个 DB 闭包,统一处理 pool 获取与错误传播。
async fn spawn_db<F, T>(pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>, f: F) -> anyhow::Result<T>
where
    F: FnOnce(&Connection) -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    Ok(tokio::task::spawn_blocking(move || {
        let conn = pool.get()?;
        f(&conn)
    })
    .await??)
}

impl Db {
    pub fn new(db_path: &str) -> anyhow::Result<Self> {
        if let Some(parent) = Path::new(db_path).parent() {
            std::fs::create_dir_all(parent).context("create db parent dir")?;
        }
        let manager = r2d2_sqlite::SqliteConnectionManager::file(db_path);
        let pool = r2d2::Pool::builder()
            .max_size(8)
            .min_idle(Some(1))
            .connection_customizer(Box::new(SqliteInit))
            .build(manager)
            .context("build sqlite pool")?;
        let db = Self { pool };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> anyhow::Result<()> {
        let conn = self.pool.get()?;
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
}
```

注意:顶部 `use` 语句已含 `use rusqlite::{Connection, params};`,此处保留 `Connection`。

- [ ] **Step 3: 移除 `private` 模块,改为顶层函数**

替换 `crates/schedule/src/db.rs:72-134`(整个 `mod private { ... }`)为两个顶层函数:

```rust
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
            headers: http_headers.and_then(|h| serde_json::from_str(&h).ok()),
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
```

- [ ] **Step 4: 重写 `create_task` 用 pool**

替换 `crates/schedule/src/db.rs:188-246`(`create_task` 整个方法)为:

```rust
    pub async fn create_task(&self, task: &Task) -> anyhow::Result<()> {
        let pool = self.pool.clone();
        let task = task.clone();
        spawn_db(pool, move |conn| {
            let (schedule_type, cron_expr, delay_secs) = match &task.schedule {
                ScheduleConfig::Cron { expr } => ("cron", Some(expr.as_str()), None),
                ScheduleConfig::Once { delay_secs } => {
                    ("once", None, Some(*delay_secs as i64))
                }
            };
            let (task_type_str, method, url, headers, body, cmd) = match &task.task_type {
                TaskType::Http {
                    method,
                    url,
                    headers,
                    body,
                } => {
                    let h = headers
                        .as_ref()
                        .and_then(|v| serde_json::to_string(v).ok());
                    (
                        "http",
                        Some(method.as_str()),
                        Some(url.as_str()),
                        h,
                        body.as_deref(),
                        None,
                    )
                }
                TaskType::Shell { cmd } => ("shell", None, None, None, None, Some(cmd.as_str())),
            };

            conn.execute(
                "INSERT INTO tasks (id, name, task_type, enabled, schedule_type, cron_expr, delay_secs,
                 http_method, http_url, http_headers, http_body, shell_cmd, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    task.id,
                    task.name,
                    task_type_str,
                    task.enabled as i64,
                    schedule_type,
                    cron_expr,
                    delay_secs,
                    method.unwrap_or("GET"),
                    url.unwrap_or(""),
                    headers,
                    body,
                    cmd.unwrap_or(""),
                    task.created_at,
                    task.updated_at,
                ],
            )?;
            Ok(())
        })
        .await
    }
```

- [ ] **Step 5: 重写 `list_enabled_tasks` 与 `list_all_tasks`**

替换 `crates/schedule/src/db.rs:248-272`(两个 list 方法)为:

```rust
    pub async fn list_enabled_tasks(&self) -> anyhow::Result<Vec<Task>> {
        let pool = self.pool.clone();
        spawn_db(pool, move |conn| {
            let mut stmt = conn.prepare("SELECT * FROM tasks WHERE enabled = 1")?;
            let tasks = stmt
                .query_map([], row_to_task)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(tasks)
        })
        .await
    }

    pub async fn list_all_tasks(&self) -> anyhow::Result<Vec<Task>> {
        let pool = self.pool.clone();
        spawn_db(pool, move |conn| {
            let mut stmt = conn.prepare("SELECT * FROM tasks ORDER BY created_at DESC")?;
            let tasks = stmt
                .query_map([], row_to_task)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(tasks)
        })
        .await
    }
```

- [ ] **Step 6: 重写 `get_task`**

替换 `crates/schedule/src/db.rs:274-288`(`get_task`)为:

```rust
    pub async fn get_task(&self, id: &str) -> anyhow::Result<Option<Task>> {
        let pool = self.pool.clone();
        let id = id.to_string();
        spawn_db(pool, move |conn| {
            let mut stmt = conn.prepare("SELECT * FROM tasks WHERE id = ?1")?;
            let mut rows = stmt.query_map(params![&id], row_to_task)?;
            match rows.next() {
                Some(Ok(task)) => Ok(Some(task)),
                Some(Err(e)) => Err(e.into()),
                None => Ok(None),
            }
        })
        .await
    }
```

- [ ] **Step 7: 重写 `update_task`**

替换 `crates/schedule/src/db.rs:290-345`(`update_task`)为:

```rust
    pub async fn update_task(&self, task: &Task) -> anyhow::Result<()> {
        let pool = self.pool.clone();
        let task = task.clone();
        spawn_db(pool, move |conn| {
            let now = chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string();
            let (schedule_type, cron_expr, delay_secs) = match &task.schedule {
                ScheduleConfig::Cron { expr } => ("cron", Some(expr.as_str()), None),
                ScheduleConfig::Once { delay_secs } => ("once", None, Some(*delay_secs as i64)),
            };
            let (task_type_str, method, url, headers, body, cmd) = match &task.task_type {
                TaskType::Http {
                    method,
                    url,
                    headers,
                    body,
                } => {
                    let h = headers.as_ref().and_then(|v| serde_json::to_string(v).ok());
                    (
                        "http",
                        Some(method.as_str()),
                        Some(url.as_str()),
                        h,
                        body.as_deref(),
                        None,
                    )
                }
                TaskType::Shell { cmd } => ("shell", None, None, None, None, Some(cmd.as_str())),
            };
            conn.execute(
                "UPDATE tasks SET name=?1, task_type=?2, enabled=?3, schedule_type=?4, cron_expr=?5,
                 delay_secs=?6, http_method=?7, http_url=?8, http_headers=?9, http_body=?10,
                 shell_cmd=?11, updated_at=?12 WHERE id=?13",
                params![
                    task.name,
                    task_type_str,
                    task.enabled as i64,
                    schedule_type,
                    cron_expr,
                    delay_secs,
                    method.unwrap_or("GET"),
                    url.unwrap_or(""),
                    headers,
                    body,
                    cmd.unwrap_or(""),
                    now,
                    task.id,
                ],
            )?;
            Ok(())
        })
        .await
    }
```

注意:`update_task` 中的 `now` 仍内联格式化串——Task 3 会统一为 `now_iso()`,此处先保留便于此 task 仅聚焦 pool 迁移。

- [ ] **Step 8: 重写 `delete_task` 与 `set_enabled`**

替换 `crates/schedule/src/db.rs:347-377`(`delete_task`、`set_enabled`)为:

```rust
    pub async fn delete_task(&self, id: &str) -> anyhow::Result<bool> {
        let pool = self.pool.clone();
        let id = id.to_string();
        spawn_db(pool, move |conn| {
            conn.execute(
                "DELETE FROM task_executions WHERE task_id = ?1",
                params![&id],
            )?;
            let affected = conn.execute("DELETE FROM tasks WHERE id = ?1", params![&id])?;
            Ok(affected > 0)
        })
        .await
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> anyhow::Result<bool> {
        let pool = self.pool.clone();
        let id = id.to_string();
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        spawn_db(pool, move |conn| {
            let affected = conn.execute(
                "UPDATE tasks SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
                params![enabled as i64, now, &id],
            )?;
            Ok(affected > 0)
        })
        .await
    }
```

- [ ] **Step 9: 重写 execution 三个方法**

替换 `crates/schedule/src/db.rs:379-471`(`create_execution`、`update_execution`、`list_executions`、`get_execution`)为:

```rust
    pub async fn create_execution(&self, exec: &TaskExecution) -> anyhow::Result<()> {
        let pool = self.pool.clone();
        let exec = exec.clone();
        spawn_db(pool, move |conn| {
            conn.execute(
                "INSERT INTO task_executions (id, task_id, status, output, http_status, started_at, finished_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    exec.id,
                    exec.task_id,
                    exec.status,
                    exec.output,
                    exec.http_status,
                    exec.started_at,
                    exec.finished_at,
                ],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn update_execution(
        &self,
        exec_id: &str,
        status: &str,
        output: &str,
        http_status: Option<i64>,
    ) -> anyhow::Result<()> {
        let pool = self.pool.clone();
        let exec_id = exec_id.to_string();
        let status = status.to_string();
        let output = output.to_string();
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        spawn_db(pool, move |conn| {
            conn.execute(
                "UPDATE task_executions SET status=?1, output=?2, http_status=?3, finished_at=?4 WHERE id=?5",
                params![status, output, http_status, now, exec_id],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn list_executions(
        &self,
        task_id: Option<&str>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<TaskExecution>> {
        let pool = self.pool.clone();
        let task_id = task_id.map(String::from);
        let limit = limit.unwrap_or(50) as i64;
        spawn_db(pool, move |conn| {
            let rows = if let Some(ref tid) = task_id {
                let mut stmt = conn.prepare(
                    "SELECT * FROM task_executions WHERE task_id = ?1 ORDER BY started_at DESC LIMIT ?2",
                )?;
                stmt.query_map(params![tid, limit], row_to_execution)?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            } else {
                let mut stmt = conn.prepare(
                    "SELECT * FROM task_executions ORDER BY started_at DESC LIMIT ?1",
                )?;
                stmt.query_map(params![limit], row_to_execution)?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            Ok(rows)
        })
        .await
    }

    pub async fn get_execution(&self, id: &str) -> anyhow::Result<Option<TaskExecution>> {
        let pool = self.pool.clone();
        let id = id.to_string();
        spawn_db(pool, move |conn| {
            let mut stmt = conn.prepare("SELECT * FROM task_executions WHERE id = ?1")?;
            let mut rows = stmt.query_map(params![&id], row_to_execution)?;
            match rows.next() {
                Some(Ok(exec)) => Ok(Some(exec)),
                Some(Err(e)) => Err(e.into()),
                None => Ok(None),
            }
        })
        .await
    }
```

- [ ] **Step 10: VERIFY**

```bash
cargo build && cargo clippy 2>&1 | grep -E 'warning|error' | head && cargo fmt
```
Expected: 构建通过,无新增 warning。`grep` 输出为空(或仅已有的 warning,不应有 error)。

- [ ] **Step 11: Commit**

```bash
git add crates/schedule/Cargo.toml crates/schedule/src/db.rs Cargo.lock
git commit -m "refactor(db): migrate from Arc<Mutex<Connection>> to r2d2 pool

- Add r2d2 + r2d2_sqlite dependencies
- Db holds a Pool<SqliteConnectionManager> (max_size=8, min_idle=1)
- Each connection acquires PRAGMA WAL + foreign_keys via CustomizeConnection
- Introduce spawn_db helper to unify spawn_blocking + error propagation
- Move row_to_task/row_to_execution out of private module to top-level"
```

---

## Task 2: Executor 复用 Client + 超时

**Files:**
- Modify: `crates/schedule/src/executor.rs:1-105`
- Modify: `crates/schedule/src/main.rs:29`

- [ ] **Step 1: 改写 `Executor` 持有 Client**

替换 `crates/schedule/src/executor.rs` 全文为:

```rust
use crate::db::{Task, TaskType};
use std::time::Duration;

pub struct ExecutionResult {
    pub status: String,
    pub output: String,
    pub http_status: Option<i64>,
}

pub struct Executor {
    client: reqwest::Client,
}

impl Executor {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client");
        Self { client }
    }

    pub async fn execute(&self, task: &Task) -> ExecutionResult {
        match &task.task_type {
            TaskType::Http {
                method,
                url,
                headers,
                body,
            } => self.execute_http(method, url, headers, body.as_deref()).await,
            TaskType::Shell { cmd } => Self::execute_shell(cmd).await,
        }
    }

    async fn execute_http(
        &self,
        method: &str,
        url: &str,
        headers: &Option<serde_json::Value>,
        body: Option<&str>,
    ) -> ExecutionResult {
        let mut req = match method.to_uppercase().as_str() {
            "GET" => self.client.get(url),
            "POST" => self.client.post(url),
            "PUT" => self.client.put(url),
            "DELETE" => self.client.delete(url),
            _ => self.client.get(url),
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
                let is_success = resp.status().is_success();
                let output = resp.text().await.unwrap_or_default();
                ExecutionResult {
                    status: if is_success {
                        "success".to_string()
                    } else {
                        "failure".to_string()
                    },
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

- [ ] **Step 2: 改 `main.rs` 构造**

替换 `crates/schedule/src/main.rs:29`:

```rust
    let executor = Arc::new(Executor::new());
```

- [ ] **Step 3: VERIFY**

```bash
cargo build && cargo clippy 2>&1 | grep -E 'warning|error' | head && cargo fmt
```
Expected: 构建通过,无 error。

- [ ] **Step 4: Commit**

```bash
git add crates/schedule/src/executor.rs crates/schedule/src/main.rs
git commit -m "refactor(executor): reuse reqwest::Client and add 30s timeout

Executor now holds a single Client built once in Executor::new(),
enabling connection/TLS pool reuse across HTTP task executions.
A 30s request timeout prevents long-tail tasks from hanging spawned tasks."
```

---

## Task 3: 提取时间格式常量与 helper

**Files:**
- Modify: `crates/schedule/src/db.rs`(顶部新增常量 + helper;`Task::new` 与 `update_task`、`set_enabled`、`update_execution` 内联串替换)
- Modify: `crates/schedule/src/api.rs:105-107`(`build_update_task` 中 `now`)
- Modify: `crates/schedule/src/scheduler.rs:42-55`(`remaining_delay` 中重复 parse)、`scheduler.rs:139-141` 与 `scheduler.rs:127-128`(started_at / next_delay 不直接用时间格式,无需改)

- [ ] **Step 1: 在 `db.rs` 顶部加常量与 helper**

在 `crates/schedule/src/db.rs` 的 `use` 语句之后、`#[derive(Debug, Clone, Serialize, Deserialize)] pub struct Task` 之前插入:

```rust
/// ISO-8601 UTC 时间戳格式,与 schema 的 `datetime('now')` 默认值兼容。
const TIMESTAMP_FMT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

/// 当前 UTC 时间的格式化字符串。
fn now_iso() -> String {
    chrono::Utc::now().format(TIMESTAMP_FMT).to_string()
}

/// 解析 ISO-8601 时间戳;兼容毫秒与任意精度小数秒两种写法。
fn parse_iso(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    use chrono::TimeZone;
    chrono::NaiveDateTime::parse_from_str(s, TIMESTAMP_FMT)
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.fZ"))
        .ok()
        .map(|dt| chrono::Utc.from_utc_datetime(&dt))
}
```

注意:`parse_iso` 用 `chrono::Utc.from_utc_datetime` 替代原来 `dt.and_utc()`,二者等价;保留 `.and_utc()` 亦可,此处用 `from_utc_datetime` 显式带时区。若希望最小改动,可用 `.map(|dt| dt.and_utc())`。

- [ ] **Step 2: 替换 `db.rs` 中 4 处内联格式化串**

`crates/schedule/src/db.rs` `Task::new`:

```rust
impl Task {
    pub fn new(name: String, task_type: TaskType, schedule: ScheduleConfig) -> Self {
        let now = now_iso();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            task_type,
            enabled: true,
            schedule,
            created_at: now.clone(),
            updated_at: now,
        }
    }
}
```

`update_task` 中:
```rust
            let now = now_iso();
```
替换原 `let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();`

`set_enabled` 中:
```rust
        let now = now_iso();
```
替换原 `let now = chrono::Utc::now().format(...).to_string();`

`update_execution` 中:
```rust
        let now = now_iso();
```
替换原 `let now = chrono::Utc::now().format(...).to_string();`

- [ ] **Step 3: 替换 `api.rs` 中格式化串**

`crates/schedule/src/api.rs:105-107`(`build_update_task` 中):

```rust
    let now = crate::db::now_iso();
```
替换:
```rust
    let now = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
```

注意:此 task 暂不动 `build_update_task` 的存在(Task 4 才合并),仅替换时间串。`now_iso` 与 `parse_iso` 是 `db.rs` 内私有函数;为让 `api.rs` 调用,需将 `now_iso` 改为 `pub(crate)`:

```rust
pub(crate) fn now_iso() -> String {
```

- [ ] **Step 4: 重写 `scheduler.rs` 的 `remaining_delay` 用 `parse_iso`**

替换 `crates/schedule/src/scheduler.rs:42-72`(整个 `remaining_delay` 方法)为:

```rust
    pub fn remaining_delay(&self, created_at: &str) -> Option<Duration> {
        match self {
            ScheduleConfig::Once { delay_secs } => {
                let remaining = match crate::db::parse_iso(created_at) {
                    Some(created) => {
                        let now = chrono::Utc::now();
                        let elapsed = (now - created).num_seconds().max(0) as u64;
                        if elapsed >= *delay_secs {
                            None
                        } else {
                            Some(Duration::from_secs(*delay_secs - elapsed))
                        }
                    }
                    None => Some(Duration::from_secs(*delay_secs)),
                };
                remaining
            }
            ScheduleConfig::Cron { .. } => Some(self.next_delay()),
        }
    }
```

并相应把 `db.rs` 的 `parse_iso` 改为 `pub(crate)`:

```rust
pub(crate) fn parse_iso(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
```

同时 `scheduler.rs` 内 `next_delay` 与执行 spawn 内的 `started_at` 仍用内联格式化——Task 7 会加日志时统一处理,本 task 只清理 `remaining_delay` 的重复 parse。

- [ ] **Step 5: VERIFY**

```bash
cargo build && cargo clippy 2>&1 | grep -E 'warning|error' | head && cargo fmt
```
Expected: 构建通过。若 `grep` 输出 clippy 提示 `parse_iso`/`now_iso` 未使用,需确认 api.rs 与 scheduler.rs 已正确引用 `crate::db::now_iso` / `crate::db::parse_iso`。

- [ ] **Step 6: Commit**

```bash
git add crates/schedule/src/db.rs crates/schedule/src/api.rs crates/schedule/src/scheduler.rs
git commit -m "refactor: extract TIMESTAMP_FMT, now_iso, parse_iso helpers

Replace 6 inline timestamp format strings and the duplicated
NaiveDateTime parse fallback in remaining_delay with shared helpers."
```

---

## Task 4: 合并 `build_task` / `build_update_task`

**Files:**
- Modify: `crates/schedule/src/api.rs:58-117`、`api.rs:176-194`(PUT handler)

- [ ] **Step 1: 重写 `build_task` 支持可选 id**

替换 `crates/schedule/src/api.rs:58-117`(`build_task` 与 `build_update_task` 两个函数)为单个函数:

```rust
fn build_task(body: CreateTaskRequest) -> Task {
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

    let mut task = Task::new(body.name, task_type, schedule);
    if let Some(enabled) = body.enabled {
        task.enabled = enabled;
    }
    task
}
```

注意:此步依赖 Task 5 先给 `CreateTaskRequest` 加 `enabled` 字段。**执行顺序:本 Task 4 与 Task 5 必须一起完成**——先做 Task 5 的 Step 1(加字段),再做本 Task 4。为避免顺序耦合,plan 中把 Task 5 Step 1 提到本 Task 之前。见 Task 5。

- [ ] **Step 2: 改写 PUT handler 用 `build_task` + 覆盖 id**

替换 `crates/schedule/src/api.rs:176-194`(PUT `/api/tasks/:id` handler 中 `build_update_task` 调用段):

```rust
            let body: CreateTaskRequest = req
                .body()
                .await
                .map_err(|e| err_msg(400, format!("invalid body: {e}")))?;
            let mut task = build_task(body);
            task.id = id;
            task.updated_at = crate::db::now_iso();
            db.update_task(&task)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?;
            let _ = tx.send(ControlCmd::Update(task.clone()));
            ok(task)
```

说明:PUT 复用 `build_task` 生成完整 task,覆盖 `id` 为路径参数。`build_task` 内 `Task::new` 会生成新 `created_at`/`updated_at`——`update_task` 的 SQL 不写 `created_at`(见 db.rs SQL),所以 `created_at` 不被覆盖;但 `updated_at` 由 `update_task` 闭包内重新 `now_iso()` 生成,这里再设一次仅为让响应体携带新时间。可省略 `task.updated_at = ...` 一行,让响应体用 `Task::new` 的时间(略旧 1ms)——为精确,保留该行。

- [ ] **Step 3: VERIFY**

```bash
cargo build && cargo clippy 2>&1 | grep -E 'warning|error' | head && cargo fmt
```
Expected: 构建通过。

- [ ] **Step 4: Commit**

```bash
git add crates/schedule/src/api.rs
git commit -m "refactor(api): merge build_update_task into build_task

build_update_task was ~60 lines duplicating build_task. PUT handler now
reuses build_task and overrides the id from the path. enabled field is
honored when present (see CreateTaskRequest.enabled)."
```

---

## Task 5: PUT 支持 `enabled` 字段

**Files:**
- Modify: `crates/schedule/src/api.rs:38-50`(`CreateTaskRequest`)、CLI 不动

- [ ] **Step 1: 给 `CreateTaskRequest` 加 `enabled` 字段**

> **顺序提示:** 此 Step 必须先于 Task 4 Step 1 完成。

替换 `crates/schedule/src/api.rs:38-50`(`CreateTaskRequest` 结构)为:

```rust
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
    enabled: Option<bool>,
}
```

- [ ] **Step 2: VERIFY**

```bash
cargo build && cargo clippy 2>&1 | grep -E 'warning|error' | head && cargo fmt
```
Expected: 构建通过。`enabled` 字段在 Task 4 后才被 `build_task` 使用;此 task 仅加字段,可能有 "field never read" warning——Task 4 完成后消失。若 clippy 报该 warning,可临时在 `enabled` 上加 `#[allow(dead_code)]`,Task 4 后移除;或直接合并到 Task 4 一起提交。**推荐:Task 4 与 Task 5 合并为一次提交**,标题用:

```bash
git commit -m "refactor(api): merge build_task variants and support enabled on PUT

- CreateTaskRequest gains optional enabled field
- build_update_task removed; PUT reuses build_task + overrides id
- build_task honors enabled when present (defaults to true via Task::new)"
```

- [ ] **Step 3: Commit**

(与 Task 4 合并提交,见 Task 4 Step 4 / 上文推荐标题。)

---

## Task 6: CLI 提取 `print_response` helper

**Files:**
- Modify: `crates/cli/src/main.rs:128-241`

- [ ] **Step 1: 添加 helper 函数**

在 `crates/cli/src/main.rs` 的 `async fn main()` 之前(`CreateTaskBody` 结构定义之后,约 line 127)插入:

```rust
async fn print_response(resp: reqwest::Response) -> Result<()> {
    let val: serde_json::Value = resp.json().await?;
    println!("{}", serde_json::to_string_pretty(&val)?);
    Ok(())
}
```

注意:`Result` 已通过 `use anyhow::{Context, Result};` 引入。

- [ ] **Step 2: 替换各子命令的 json+println 样板**

`crates/cli/src/main.rs` `TaskCmd::Add`(约 line 167-168):

替换:
```rust
                let task: serde_json::Value = resp.json().await?;
                println!("{}", serde_json::to_string_pretty(&task)?);
```
为:
```rust
                print_response(resp).await?;
```

`TaskCmd::List`(约 line 173-184):此处有过滤逻辑,不直接用 helper。保留但简化打印。替换:
```rust
                let tasks: Vec<serde_json::Value> = resp.json().await?;

                let filtered: Vec<&serde_json::Value> = if let Some(enabled) = args.enabled {
                    tasks
                        .iter()
                        .filter(|t| t["enabled"].as_bool() == Some(enabled))
                        .collect()
                } else {
                    tasks.iter().collect()
                };

                println!("{}", serde_json::to_string_pretty(&filtered)?);
```
为:
```rust
                let tasks: Vec<serde_json::Value> = resp.json().await?;
                let filtered: Vec<&serde_json::Value> = match args.enabled {
                    Some(enabled) => tasks
                        .iter()
                        .filter(|t| t["enabled"].as_bool() == Some(enabled))
                        .collect(),
                    None => tasks.iter().collect(),
                };
                println!("{}", serde_json::to_string_pretty(&filtered)?);
```
(List 因有过滤,保留 `println!`,仅把 `if let` 改 `match` 以紧凑——helper 不适用。)

`TaskCmd::Show`(约 line 191-192):
```rust
                let task: serde_json::Value = resp.json().await?;
                println!("{}", serde_json::to_string_pretty(&task)?);
```
→
```rust
                print_response(resp).await?;
```

`TaskCmd::Rm`(约 line 199-200)、`TaskCmd::Enable`(约 line 207-208)、`TaskCmd::Disable`(约 line 215-216):同样模式,各 2 行 → `print_response(resp).await?;`

`ExecCmd::List`(约 line 226-227):
```rust
                let execs: Vec<serde_json::Value> = resp.json().await?;
                println!("{}", serde_json::to_string_pretty(&execs)?);
```
→
```rust
                print_response(resp).await?;
```

`ExecCmd::Show`(约 line 234-235):
```rust
                let exec: serde_json::Value = resp.json().await?;
                println!("{}", serde_json::to_string_pretty(&exec)?);
```
→
```rust
                print_response(resp).await?;
```

- [ ] **Step 3: VERIFY**

```bash
cargo build && cargo clippy 2>&1 | grep -E 'warning|error' | head && cargo fmt
```
Expected: 构建通过,无 warning。

- [ ] **Step 4: Commit**

```bash
git add crates/cli/src/main.rs
git commit -m "refactor(cli): extract print_response helper

Each subcommand's resp.json().await? + to_string_pretty pattern collapses
to a single print_response(resp).await? call. List retains inline printing
due to its client-side enabled filter."
```

---

## Task 7: Scheduler 错误处理 + 结构化日志

**Files:**
- Modify: `crates/schedule/src/scheduler.rs:111-177`(`run` 方法)
- Modify: `crates/schedule/src/scheduler.rs:83-90`(`load_tasks` 加过期日志)

- [ ] **Step 1: `load_tasks` 过期 Once 日志**

替换 `crates/schedule/src/scheduler.rs:83-90`(`load_tasks`)为:

```rust
    pub fn load_tasks(&mut self, tasks: Vec<Task>) {
        for task in tasks {
            match task.schedule.remaining_delay(&task.created_at) {
                Some(delay) => {
                    let key = self.queue.insert(task.clone(), delay);
                    self.keys.insert(task.id.clone(), key);
                }
                None => {
                    tracing::info!(
                        task_id = %task.id,
                        name = %task.name,
                        "skipping expired once-task on startup"
                    );
                }
            }
        }
    }
```

- [ ] **Step 2: 重写 `run` 主循环执行分支**

替换 `crates/schedule/src/scheduler.rs:117-176`(`loop { tokio::select! { ... } }` 整段)为:

```rust
        loop {
            tokio::select! {
                Some(expired) = self.queue.next() => {
                    let task = expired.into_inner();

                    match &task.schedule {
                        ScheduleConfig::Once { .. } => {
                            self.keys.remove(&task.id);
                        }
                        ScheduleConfig::Cron { .. } => {
                            let next = task.schedule.next_delay();
                            let key = self.queue.insert(task.clone(), next);
                            self.keys.insert(task.id.clone(), key);
                        }
                    }

                    tracing::debug!(task_id = %task.id, name = %task.name, "task due");

                    let exec = executor.clone();
                    let db = db.clone();
                    let task_clone = task.clone();

                    tokio::spawn(async move {
                        let exec_id = Uuid::new_v4().to_string();
                        let started_at = crate::db::now_iso();
                        let start = std::time::Instant::now();

                        if let Err(e) = db
                            .create_execution(&TaskExecution {
                                id: exec_id.clone(),
                                task_id: task_clone.id.clone(),
                                status: "running".to_string(),
                                output: None,
                                http_status: None,
                                started_at,
                                finished_at: None,
                            })
                            .await
                        {
                            tracing::warn!(
                                error = %e,
                                task_id = %task_clone.id,
                                "create execution failed; skipping run"
                            );
                            return;
                        }

                        tracing::info!(
                            task_id = %task_clone.id,
                            exec_id = %exec_id,
                            "execution started"
                        );

                        let result = exec.execute(&task_clone).await;

                        if let Err(e) = db
                            .update_execution(
                                &exec_id,
                                &result.status,
                                &result.output,
                                result.http_status,
                            )
                            .await
                        {
                            tracing::warn!(
                                error = %e,
                                task_id = %task_clone.id,
                                exec_id = %exec_id,
                                "update execution failed"
                            );
                        }

                        tracing::info!(
                            task_id = %task_clone.id,
                            exec_id = %exec_id,
                            status = %result.status,
                            duration_ms = start.elapsed().as_millis() as u64,
                            "execution finished"
                        );
                    });
                }

                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        ControlCmd::Add(task) => {
                            tracing::debug!(task_id = %task.id, "scheduler add");
                            self.insert(task);
                        }
                        ControlCmd::Remove(id) => {
                            tracing::debug!(task_id = %id, "scheduler remove");
                            self.remove(&id);
                        }
                        ControlCmd::Update(task) => {
                            tracing::debug!(task_id = %task.id, "scheduler update");
                            self.update(task);
                        }
                    }
                }
            }
        }
```

- [ ] **Step 3: VERIFY**

```bash
cargo build && cargo clippy 2>&1 | grep -E 'warning|error' | head && cargo fmt
```
Expected: 构建通过,无 warning。

- [ ] **Step 4: Commit**

```bash
git add crates/schedule/src/scheduler.rs
git commit -m "feat(scheduler): structured logging and non-silent execution errors

- load_tasks logs expired once-tasks instead of silently dropping
- run loop emits debug/info logs at due/start/finish and warn on db errors
- create_execution failure now aborts the run (no orphan execution);
  update_execution failure is warned but does not mask completion
- duration_ms captured per execution"
```

---

## Task 8: 全量验证 + 手动冒烟

**Files:** 无改动

- [ ] **Step 1: 全量构建检查**

```bash
cargo build && cargo clippy --all-targets 2>&1 | grep -E 'warning|error' | head -20 && cargo fmt --check
```
Expected: 构建成功;clippy 无 error;`cargo fmt --check` 无输出(已格式化)。

- [ ] **Step 2: 手动冒烟(可选,需启动服务)**

终端 1:
```bash
cargo run -p i-rs-schedule
```
预期日志: `loaded N enabled tasks from db` + `starting server on http://127.0.0.1:3000`

终端 2(CLI):
```bash
# 创建 cron 任务(每分钟)
cargo run -p i-rs-schedule-cli -- task add --name smoke --type http --cron "0 * * * * *" --url "https://httpbin.org/get"

# 列表
cargo run -p i-rs-schedule-cli -- task list

# 手动触发 run,观察 execution 记录
TASK_ID=$(cargo run -p i-rs-schedule-cli -- task list 2>/dev/null | grep -A1 smoke | head -1 | jq -r .id 2>/dev/null)
# 若无 jq,手动从 list 输出取 id
curl -s -X POST http://localhost:3000/api/tasks/<id>/run | python3 -m json.tool

# 查询执行记录
cargo run -p i-rs-schedule-cli -- exec list --limit 5

# PUT 更新带 enabled
curl -s -X PUT http://localhost:3000/api/tasks/<id> \
  -H 'Content-Type: application/json' \
  -d '{"name":"smoke","task_type":"http","schedule_type":"cron","cron_expr":"0 * * * * *","http_url":"https://httpbin.org/get","enabled":false}' | python3 -m json.tool

# 验证 enabled=false 生效
cargo run -p i-rs-schedule-cli -- task list

# 清理
cargo run -p i-rs-schedule-cli -- task rm --id <id>
```
预期:PUT 后 `enabled` 为 false;run 触发的 execution 记录含 `status`/`output`/`http_status`;服务端日志可见 `execution started` / `execution finished` 带 `duration_ms`。

- [ ] **Step 3: 终结 commit(如有 fmt 变动)**

```bash
git status
# 若有未提交改动:
git add -A && git commit -m "style: final rustfmt pass"
# 若干净则跳过
```

---

## 完成后产物

- DB 层并发模型从单连接 Mutex 升级为 8 连接池
- HTTP 任务执行复用连接池 + 30s 超时
- 6 处时间格式串、`build_task` 重复、CLI 样板全部消除
- scheduler 不再静默吞错,关键路径有结构化日志
- PUT 支持 `enabled` 字段更新
- 共 6 个语义化 commit(pool / executor / helpers / api / cli / scheduler)
