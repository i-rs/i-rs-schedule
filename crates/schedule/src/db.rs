use anyhow::Context;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

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
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
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

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

mod private {
    use super::*;

    pub fn row_to_task(row: &rusqlite::Row) -> rusqlite::Result<Task> {
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

    pub fn row_to_execution(row: &rusqlite::Row) -> rusqlite::Result<TaskExecution> {
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

    pub async fn create_task(&self, task: &Task) -> anyhow::Result<()> {
        let conn = self.conn.clone();
        let task = task.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let conn = conn.lock().unwrap();
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
        .await??;
        Ok(())
    }

    pub async fn list_enabled_tasks(&self) -> anyhow::Result<Vec<Task>> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<Task>> {
            let conn = conn.lock().unwrap();
            let mut stmt = conn.prepare("SELECT * FROM tasks WHERE enabled = 1")?;
            let tasks = stmt
                .query_map([], private::row_to_task)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(tasks)
        })
        .await?
    }

    pub async fn list_all_tasks(&self) -> anyhow::Result<Vec<Task>> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<Task>> {
            let conn = conn.lock().unwrap();
            let mut stmt = conn.prepare("SELECT * FROM tasks ORDER BY created_at DESC")?;
            let tasks = stmt
                .query_map([], private::row_to_task)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(tasks)
        })
        .await?
    }

    pub async fn get_task(&self, id: &str) -> anyhow::Result<Option<Task>> {
        let conn = self.conn.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Option<Task>> {
            let conn = conn.lock().unwrap();
            let mut stmt = conn.prepare("SELECT * FROM tasks WHERE id = ?1")?;
            let mut rows = stmt.query_map(params![&id], private::row_to_task)?;
            match rows.next() {
                Some(Ok(task)) => Ok(Some(task)),
                Some(Err(e)) => Err(e.into()),
                None => Ok(None),
            }
        })
        .await?
    }

    pub async fn update_task(&self, task: &Task) -> anyhow::Result<()> {
        let conn = self.conn.clone();
        let task = task.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let conn = conn.lock().unwrap();
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
        .await??;
        Ok(())
    }

    pub async fn delete_task(&self, id: &str) -> anyhow::Result<bool> {
        let conn = self.conn.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> anyhow::Result<bool> {
            let conn = conn.lock().unwrap();
            conn.execute(
                "DELETE FROM task_executions WHERE task_id = ?1",
                params![&id],
            )?;
            let affected = conn.execute("DELETE FROM tasks WHERE id = ?1", params![&id])?;
            Ok(affected > 0)
        })
        .await?
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> anyhow::Result<bool> {
        let conn = self.conn.clone();
        let id = id.to_string();
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        tokio::task::spawn_blocking(move || -> anyhow::Result<bool> {
            let conn = conn.lock().unwrap();
            let affected = conn.execute(
                "UPDATE tasks SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
                params![enabled as i64, now, &id],
            )?;
            Ok(affected > 0)
        })
        .await?
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
        .await??;
        Ok(())
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
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let conn = conn.lock().unwrap();
            conn.execute(
                "UPDATE task_executions SET status=?1, output=?2, http_status=?3, finished_at=?4 WHERE id=?5",
                params![status, output, http_status, now, exec_id],
            )?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    pub async fn list_executions(
        &self,
        task_id: Option<&str>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<TaskExecution>> {
        let conn = self.conn.clone();
        let task_id = task_id.map(String::from);
        let limit = limit.unwrap_or(50) as i64;
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<TaskExecution>> {
                let conn = conn.lock().unwrap();
                let rows = if let Some(ref tid) = task_id {
                    let mut stmt = conn.prepare(
                        "SELECT * FROM task_executions WHERE task_id = ?1 ORDER BY started_at DESC LIMIT ?2",
                    )?;
                    stmt.query_map(params![tid, limit], private::row_to_execution)?
                        .collect::<rusqlite::Result<Vec<_>>>()?
                } else {
                    let mut stmt = conn.prepare(
                        "SELECT * FROM task_executions ORDER BY started_at DESC LIMIT ?1",
                    )?;
                    stmt.query_map(params![limit], private::row_to_execution)?
                        .collect::<rusqlite::Result<Vec<_>>>()?
                };
                Ok(rows)
            })
            .await?
    }

    pub async fn get_execution(&self, id: &str) -> anyhow::Result<Option<TaskExecution>> {
        let conn = self.conn.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Option<TaskExecution>> {
            let conn = conn.lock().unwrap();
            let mut stmt = conn.prepare("SELECT * FROM task_executions WHERE id = ?1")?;
            let mut rows = stmt.query_map(params![&id], |row| private::row_to_execution(row))?;
            match rows.next() {
                Some(Ok(exec)) => Ok(Some(exec)),
                Some(Err(e)) => Err(e.into()),
                None => Ok(None),
            }
        })
        .await?
    }
}
