use anyhow::Context;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub use crate::schedule::ScheduleConfig;

/// ISO-8601 UTC 时间戳格式,与 schema 的 `datetime('now')` 默认值兼容。
const TIMESTAMP_FMT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

/// 当前 UTC 时间的格式化字符串。
pub(crate) fn now_iso() -> String {
    chrono::Utc::now().format(TIMESTAMP_FMT).to_string()
}

fn default_max_concurrent() -> i64 {
    1
}

/// 解析 ISO-8601 时间戳;兼容毫秒与任意精度小数秒两种写法。
pub(crate) fn parse_iso(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::NaiveDateTime::parse_from_str(s, TIMESTAMP_FMT)
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.fZ"))
        .ok()
        .map(|dt| dt.and_utc())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub task_type: TaskType,
    pub enabled: bool,
    pub schedule: ScheduleConfig,
    pub timezone: String,
    pub timeout_secs: u64,
    pub max_retries: i64,
    /// 并发互斥:同任务在途执行达到上限即跳过;0 = 不限并行
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: i64,
    #[serde(default)]
    pub trigger_task_ids: Vec<String>,
    /// 标签(JSON 数组),组织与批量操作用
    #[serde(default)]
    pub tags: Vec<String>,
    pub trigger_on: String,
    #[serde(rename = "notify_type")]
    pub notify_type: String,
    #[serde(default)]
    pub notify_url: String,
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
pub struct TaskExecution {
    pub id: String,
    pub task_id: String,
    pub attempt: i64,
    pub status: String,
    pub output: Option<String>,
    pub http_status: Option<i64>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

impl Task {
    pub fn new(name: String, task_type: TaskType, schedule: ScheduleConfig) -> Self {
        let now = now_iso();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            task_type,
            enabled: true,
            schedule,
            timezone: "UTC".to_string(),
            timeout_secs: 30,
            max_retries: 0,
            max_concurrent: 1,
            trigger_task_ids: Vec::new(),
            tags: Vec::new(),
            trigger_on: "success".into(),
            notify_type: "none".to_string(),
            notify_url: String::new(),
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

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
async fn spawn_db<F, T>(
    pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
    f: F,
) -> anyhow::Result<T>
where
    F: FnOnce(&Connection) -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let conn = pool.get()?;
        f(&conn)
    })
    .await?
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
    let timezone: String = row.get("timezone")?;
    let timeout_secs: u64 = row.get::<_, i64>("timeout_secs")? as u64;
    let max_retries: i64 = row.get("max_retries")?;
    let max_concurrent: i64 = row.get("max_concurrent")?;
    let trigger_ids_raw: String = row.get("trigger_task_ids")?;
    let trigger_task_ids: Vec<String> = serde_json::from_str(&trigger_ids_raw).unwrap_or_default();
    let tags_raw: String = row.get("tags")?;
    let tags: Vec<String> = serde_json::from_str(&tags_raw).unwrap_or_default();
    let trigger_on: String = row.get("trigger_on")?;
    let notify_type: String = row.get("notify_type")?;
    let notify_url: String = row.get("notify_url")?;

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
        timezone,
        timeout_secs,
        max_retries,
        max_concurrent,
        trigger_task_ids,
        tags,
        trigger_on,
        notify_type,
        notify_url,
        created_at,
        updated_at,
    })
}

fn row_to_execution(row: &rusqlite::Row) -> rusqlite::Result<TaskExecution> {
    Ok(TaskExecution {
        id: row.get("id")?,
        task_id: row.get("task_id")?,
        attempt: row.get("attempt")?,
        status: row.get("status")?,
        output: row.get("output")?,
        http_status: row.get("http_status")?,
        started_at: row.get("started_at")?,
        finished_at: row.get("finished_at")?,
    })
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
        db.cleanup_stale_running()?;
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
                timezone    TEXT NOT NULL DEFAULT 'UTC',
                timeout_secs INTEGER NOT NULL DEFAULT 30,
                max_retries INTEGER NOT NULL DEFAULT 0,
                max_concurrent INTEGER NOT NULL DEFAULT 1,
                trigger_task_ids TEXT NOT NULL DEFAULT '[]', -- 成功后触发的下游任务(id 数组)
                tags         TEXT NOT NULL DEFAULT '[]', -- 标签(JSON 数组)
                trigger_on   TEXT NOT NULL DEFAULT 'success',
                notify_type TEXT NOT NULL DEFAULT 'none',
                notify_url  TEXT NOT NULL DEFAULT '',
                created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS task_executions (
                id          TEXT PRIMARY KEY,
                task_id     TEXT NOT NULL,
                status      TEXT NOT NULL,
                attempt     INTEGER NOT NULL DEFAULT 0,
                output      TEXT,
                http_status INTEGER,
                started_at  TEXT NOT NULL DEFAULT (datetime('now')),
                finished_at TEXT,
                FOREIGN KEY (task_id) REFERENCES tasks(id)
            );

            CREATE TABLE IF NOT EXISTS users (
                username      TEXT PRIMARY KEY,
                password_hash TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sessions (
                token_hash TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS api_tokens (
                id         TEXT PRIMARY KEY,
                name       TEXT NOT NULL,
                token_hash TEXT UNIQUE NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS audit_log (
                id      TEXT PRIMARY KEY,
                ts      TEXT NOT NULL,
                action  TEXT NOT NULL,
                summary TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_task_executions_task_id ON task_executions(task_id);
            CREATE INDEX IF NOT EXISTS idx_task_executions_started_at ON task_executions(started_at);
            ",
        )
        .context("init sqlite schema")?;
        self.migrate_schema(&conn)?;
        self.migrate_executions_schema(&conn)?;
        Ok(())
    }

    /// 存量库的列迁移:PRAGMA 检查后 ALTER TABLE 补列,幂等。
    fn migrate_schema(&self, conn: &Connection) -> anyhow::Result<()> {
        let existing: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(tasks)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        if !existing.iter().any(|c| c == "timezone") {
            conn.execute(
                "ALTER TABLE tasks ADD COLUMN timezone TEXT NOT NULL DEFAULT 'UTC'",
                [],
            )?;
            tracing::info!("migrated tasks table: added timezone");
        }
        if !existing.iter().any(|c| c == "timeout_secs") {
            conn.execute(
                "ALTER TABLE tasks ADD COLUMN timeout_secs INTEGER NOT NULL DEFAULT 30",
                [],
            )?;
            tracing::info!("migrated tasks table: added timeout_secs");
        }
        if !existing.iter().any(|c| c == "max_retries") {
            conn.execute(
                "ALTER TABLE tasks ADD COLUMN max_retries INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
            tracing::info!("migrated tasks table: added max_retries");
        }
        if !existing.iter().any(|c| c == "max_concurrent") {
            conn.execute(
                "ALTER TABLE tasks ADD COLUMN max_concurrent INTEGER NOT NULL DEFAULT 1",
                [],
            )?;
            tracing::info!("migrated tasks table: added max_concurrent");
        }
        if !existing.iter().any(|c| c == "trigger_task_ids") {
            conn.execute(
                "ALTER TABLE tasks ADD COLUMN trigger_task_ids TEXT NOT NULL DEFAULT '[]'",
                [],
            )?;
            tracing::info!("migrated tasks table: added trigger_task_ids");
        }
        if !existing.iter().any(|c| c == "tags") {
            conn.execute(
                "ALTER TABLE tasks ADD COLUMN tags TEXT NOT NULL DEFAULT '[]'",
                [],
            )?;
            tracing::info!("migrated tasks table: added tags");
        }
        if !existing.iter().any(|c| c == "trigger_on") {
            conn.execute(
                "ALTER TABLE tasks ADD COLUMN trigger_on TEXT NOT NULL DEFAULT 'success'",
                [],
            )?;
            tracing::info!("migrated tasks table: added trigger_on");
        }
        // 旧单下游列数据并入数组列
        if existing.iter().any(|c| c == "trigger_task_id") {
            conn.execute(
                "UPDATE tasks SET trigger_task_ids = json_array(trigger_task_id)
                 WHERE trigger_task_id != '' AND trigger_task_ids = '[]'",
                [],
            )?;
        }
        if !existing.iter().any(|c| c == "notify_type") {
            conn.execute(
                "ALTER TABLE tasks ADD COLUMN notify_type TEXT NOT NULL DEFAULT 'none'",
                [],
            )?;
            tracing::info!("migrated tasks table: added notify_type");
        }
        if !existing.iter().any(|c| c == "notify_url") {
            conn.execute(
                "ALTER TABLE tasks ADD COLUMN notify_url TEXT NOT NULL DEFAULT ''",
                [],
            )?;
            tracing::info!("migrated tasks table: added notify_url");
        }
        Ok(())
    }

    /// `task_executions` 补列(attempt)。
    fn migrate_executions_schema(&self, conn: &Connection) -> anyhow::Result<()> {
        let existing: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(task_executions)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        if !existing.iter().any(|c| c == "attempt") {
            conn.execute(
                "ALTER TABLE task_executions ADD COLUMN attempt INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
            tracing::info!("migrated task_executions table: added attempt");
        }
        Ok(())
    }

    /// 将上次进程残留的 `status='running'` 执行记录标记为 `interrupted`。
    /// 进程崩溃/被杀后这些记录会永远停在 running;启动时修复以保证可观察性。
    fn cleanup_stale_running(&self) -> anyhow::Result<()> {
        let conn = self.pool.get()?;
        let now = now_iso();
        let affected = conn.execute(
            "UPDATE task_executions SET status = ?1, finished_at = ?2 WHERE status = 'running'",
            params!["interrupted", now],
        )?;
        if affected > 0 {
            tracing::info!(
                affected,
                "marked stale running executions as interrupted on startup"
            );
        }
        Ok(())
    }

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
                 http_method, http_url, http_headers, http_body, shell_cmd, timezone, timeout_secs,
                 max_retries, max_concurrent, trigger_task_ids, tags, trigger_on, notify_type,
                 notify_url, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
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
                    task.timezone,
                    task.timeout_secs as i64,
                    task.max_retries,
                    task.max_concurrent,
                    serde_json::to_string(&task.trigger_task_ids).unwrap(),
                    serde_json::to_string(&task.tags).unwrap(),
                    task.trigger_on,
                    task.notify_type,
                    task.notify_url,
                    task.created_at,
                    task.updated_at,
                ],
            )?;
            Ok(())
        })
        .await
    }

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

    pub async fn update_task(&self, task: &Task) -> anyhow::Result<()> {
        let pool = self.pool.clone();
        let task = task.clone();
        let now = now_iso();
        spawn_db(pool, move |conn| {
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
                 shell_cmd=?11, timezone=?12, timeout_secs=?13, max_retries=?14, max_concurrent=?15,
                 trigger_task_ids=?16, tags=?17, trigger_on=?18, notify_type=?19, notify_url=?20,
                 updated_at=?21 WHERE id=?22",
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
                    task.timezone,
                    task.timeout_secs as i64,
                    task.max_retries,
                    task.max_concurrent,
                    serde_json::to_string(&task.trigger_task_ids).unwrap(),
                    serde_json::to_string(&task.tags).unwrap(),
                    task.trigger_on,
                    task.notify_type,
                    task.notify_url,
                    now,
                    task.id,
                ],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn delete_task(&self, id: &str) -> anyhow::Result<bool> {
        let pool = self.pool.clone();
        let id = id.to_string();
        spawn_db(pool, move |conn| {
            // 事务保证任务与执行记录原子删除,避免中间失败留下不一致。
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "DELETE FROM task_executions WHERE task_id = ?1",
                params![&id],
            )?;
            let affected = tx.execute("DELETE FROM tasks WHERE id = ?1", params![&id])?;
            tx.commit()?;
            Ok(affected > 0)
        })
        .await
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> anyhow::Result<bool> {
        let pool = self.pool.clone();
        let id = id.to_string();
        let now = now_iso();
        spawn_db(pool, move |conn| {
            let affected = conn.execute(
                "UPDATE tasks SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
                params![enabled as i64, now, &id],
            )?;
            Ok(affected > 0)
        })
        .await
    }

    pub async fn create_execution(&self, exec: &TaskExecution) -> anyhow::Result<()> {
        let pool = self.pool.clone();
        let exec = exec.clone();
        spawn_db(pool, move |conn| {
            conn.execute(
                "INSERT INTO task_executions (id, task_id, attempt, status, output, http_status, started_at, finished_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    exec.id,
                    exec.task_id,
                    exec.attempt,
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
        let now = now_iso();
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

    /// 查询某任务在指定 execution 之前最近的一条记录(恢复通知的判定依据)。
    pub async fn get_previous_execution(
        &self,
        task_id: &str,
        before_exec_id: &str,
    ) -> anyhow::Result<Option<TaskExecution>> {
        let pool = self.pool.clone();
        let task_id = task_id.to_string();
        let before_exec_id = before_exec_id.to_string();
        spawn_db(pool, move |conn| {
            let mut stmt = conn.prepare(
                "SELECT * FROM task_executions WHERE task_id = ?1 AND id != ?2
                 ORDER BY started_at DESC, rowid DESC LIMIT 1",
            )?;
            let mut rows = stmt.query_map(params![&task_id, &before_exec_id], row_to_execution)?;
            match rows.next() {
                Some(Ok(exec)) => Ok(Some(exec)),
                Some(Err(e)) => Err(e.into()),
                None => Ok(None),
            }
        })
        .await
    }

    /// 执行统计(供 /metrics):总数、成功数、失败数、启用任务数。
    pub async fn stats(&self) -> anyhow::Result<(i64, i64, i64, i64)> {
        let pool = self.pool.clone();
        spawn_db(pool, move |conn| {
            let total: i64 =
                conn.query_row("SELECT COUNT(*) FROM task_executions", [], |r| r.get(0))?;
            let success: i64 = conn.query_row(
                "SELECT COUNT(*) FROM task_executions WHERE status = 'success'",
                [],
                |r| r.get(0),
            )?;
            let failure: i64 = conn.query_row(
                "SELECT COUNT(*) FROM task_executions WHERE status = 'failure'",
                [],
                |r| r.get(0),
            )?;
            let enabled: i64 =
                conn.query_row("SELECT COUNT(*) FROM tasks WHERE enabled = 1", [], |r| {
                    r.get(0)
                })?;
            Ok((total, success, failure, enabled))
        })
        .await
    }

    /// 单任务执行统计:总数、成功数、失败数、平均耗时(毫秒,未完成的不计入)。
    pub async fn task_stats(&self, task_id: &str) -> anyhow::Result<(i64, i64, i64, Option<f64>)> {
        let pool = self.pool.clone();
        let task_id = task_id.to_string();
        spawn_db(pool, move |conn| {
            let row = conn.query_row(
                "SELECT COUNT(*),
                        SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END),
                        SUM(CASE WHEN status = 'failure' THEN 1 ELSE 0 END),
                        AVG(CASE WHEN finished_at IS NOT NULL
                                 THEN (julianday(finished_at) - julianday(started_at)) * 86400000 END)
                 FROM task_executions WHERE task_id = ?1",
                params![task_id],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                        r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                        r.get::<_, Option<f64>>(3)?,
                    ))
                },
            )?;
            Ok(row)
        })
        .await
    }

    /// 近 N 天每日执行统计:(day, success, failure, 平均耗时ms),按日升序。缺失日由调用方补零。
    pub async fn daily_stats(
        &self,
        cutoff_day: String,
    ) -> anyhow::Result<Vec<(String, i64, i64, Option<f64>)>> {
        let pool = self.pool.clone();
        spawn_db(pool, move |conn| {
            let mut stmt = conn.prepare(
                "SELECT substr(started_at, 1, 10) AS day,
                        SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END),
                        SUM(CASE WHEN status = 'failure' THEN 1 ELSE 0 END),
                        AVG(CASE WHEN finished_at IS NOT NULL
                                 THEN (julianday(finished_at) - julianday(started_at)) * 86400000 END)
                 FROM task_executions WHERE started_at >= ?1 GROUP BY day ORDER BY day",
            )?;
            let rows = stmt.query_map(params![cutoff_day], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<f64>>(3)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
        })
        .await
    }

    // ---- 认证相关 ----

    pub fn sha256_hex(input: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub async fn seed_admin(&self, username: &str, password: &str) -> anyhow::Result<()> {
        let pool = self.pool.clone();
        let username = username.to_string();
        let hash = Self::sha256_hex(password);
        spawn_db(pool, move |conn| {
            conn.execute(
                "INSERT INTO users (username, password_hash) VALUES (?1, ?2)
                 ON CONFLICT(username) DO UPDATE SET password_hash = excluded.password_hash",
                params![username, hash],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn verify_admin(&self, username: &str, password: &str) -> anyhow::Result<bool> {
        let pool = self.pool.clone();
        let username = username.to_string();
        let hash = Self::sha256_hex(password);
        spawn_db(pool, move |conn| {
            let stored: Option<String> = conn
                .query_row(
                    "SELECT password_hash FROM users WHERE username = ?1",
                    params![username],
                    |r| r.get(0),
                )
                .ok();
            Ok(stored == Some(hash))
        })
        .await
    }

    pub async fn create_session(&self, ttl_days: i64) -> anyhow::Result<(String, String)> {
        let pool = self.pool.clone();
        spawn_db(pool, move |conn| {
            use rand::RngCore;
            let mut bytes = [0u8; 32];
            rand::rng().fill_bytes(&mut bytes);
            let token = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
            let token_hash = Self::sha256_hex(&token);
            let now = now_iso();
            let expires = (chrono::Utc::now() + chrono::Duration::days(ttl_days))
                .format(TIMESTAMP_FMT)
                .to_string();
            conn.execute(
                "INSERT INTO sessions (token_hash, created_at, expires_at) VALUES (?1, ?2, ?3)",
                params![token_hash, now, expires],
            )?;
            Ok((token, expires))
        })
        .await
    }

    /// Bearer token 是否对应有效会话;顺带清理过期会话(惰性)。
    pub async fn session_valid(&self, token: &str) -> anyhow::Result<bool> {
        let pool = self.pool.clone();
        let token_hash = Self::sha256_hex(token);
        let now = now_iso();
        spawn_db(pool, move |conn| {
            conn.execute("DELETE FROM sessions WHERE expires_at < ?1", params![now])?;
            let valid: Option<String> = conn
                .query_row(
                    "SELECT expires_at FROM sessions WHERE token_hash = ?1",
                    params![token_hash],
                    |r| r.get(0),
                )
                .ok();
            Ok(valid.map(|e| e >= now).unwrap_or(false))
        })
        .await
    }

    pub async fn create_api_token(&self, name: &str) -> anyhow::Result<(String, String, String)> {
        let pool = self.pool.clone();
        let name = name.to_string();
        spawn_db(pool, move |conn| {
            use rand::RngCore;
            let mut bytes = [0u8; 32];
            rand::rng().fill_bytes(&mut bytes);
            let token = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
            let token_hash = Self::sha256_hex(&token);
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO api_tokens (id, name, token_hash, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![id, name, token_hash, now_iso()],
            )?;
            Ok((id, name, token))
        })
        .await
    }

    pub async fn list_api_tokens(&self) -> anyhow::Result<Vec<(String, String, String)>> {
        let pool = self.pool.clone();
        spawn_db(pool, move |conn| {
            let mut stmt = conn
                .prepare("SELECT id, name, created_at FROM api_tokens ORDER BY created_at DESC")?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
    }

    pub async fn revoke_api_token(&self, id: &str) -> anyhow::Result<bool> {
        let pool = self.pool.clone();
        let id = id.to_string();
        spawn_db(pool, move |conn| {
            let n = conn.execute("DELETE FROM api_tokens WHERE id = ?1", params![id])?;
            Ok(n > 0)
        })
        .await
    }

    pub async fn api_token_valid(&self, token: &str) -> anyhow::Result<bool> {
        let pool = self.pool.clone();
        let token_hash = Self::sha256_hex(token);
        spawn_db(pool, move |conn| {
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM api_tokens WHERE token_hash = ?1",
                params![token_hash],
                |r| r.get(0),
            )?;
            Ok(n > 0)
        })
        .await
    }

    pub async fn audit(&self, action: &str, summary: &str) -> anyhow::Result<()> {
        let pool = self.pool.clone();
        let action = action.to_string();
        let summary = summary.to_string();
        spawn_db(pool, move |conn| {
            conn.execute(
                "INSERT INTO audit_log (id, ts, action, summary) VALUES (?1, ?2, ?3, ?4)",
                params![uuid::Uuid::new_v4().to_string(), now_iso(), action, summary],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn list_audit(&self, limit: i64) -> anyhow::Result<Vec<(String, String, String)>> {
        let pool = self.pool.clone();
        spawn_db(pool, move |conn| {
            let mut stmt = conn
                .prepare("SELECT ts, action, summary FROM audit_log ORDER BY ts DESC LIMIT ?1")?;
            let rows = stmt
                .query_map(params![limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
    }

    /// 读取一个设置项(settings 表)。
    pub async fn get_setting(&self, key: &str) -> anyhow::Result<Option<String>> {
        let pool = self.pool.clone();
        let key = key.to_string();
        spawn_db(pool, move |conn| {
            let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
            let mut rows = stmt.query_map(params![key], |r| r.get::<_, String>(0))?;
            Ok(rows.next().transpose()?)
        })
        .await
    }

    /// SQLite 在线备份:VACUUM INTO 目标文件(文件必须已删除或不存在)。
    pub async fn vacuum_into(&self, target: &str) -> anyhow::Result<()> {
        let pool = self.pool.clone();
        let target = target.to_string();
        spawn_db(pool, move |conn| {
            conn.execute("VACUUM INTO ?1", params![target])?;
            Ok(())
        })
        .await
    }

    /// 写入一个设置项(幂等 upsert)。
    pub async fn set_setting(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let pool = self.pool.clone();
        let (key, value) = (key.to_string(), value.to_string());
        spawn_db(pool, move |conn| {
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
            Ok(())
        })
        .await
    }

    /// 删除早于 cutoff 的执行记录,返回删除行数。
    pub async fn purge_executions_older_than(&self, cutoff_iso: String) -> anyhow::Result<u64> {
        let pool = self.pool.clone();
        spawn_db(pool, move |conn| {
            let affected = conn.execute(
                "DELETE FROM task_executions WHERE started_at < ?1",
                params![cutoff_iso],
            )?;
            Ok(affected as u64)
        })
        .await
    }
}
