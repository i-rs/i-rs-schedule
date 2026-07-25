use crate::db::{Db, ScheduleConfig, Task, TaskType};
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

            let now = chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string();
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
            let limit = req.query("limit").and_then(|s: &str| s.parse::<u32>().ok());
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
