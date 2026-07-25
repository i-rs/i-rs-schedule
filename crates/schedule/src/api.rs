use crate::db::{Db, ScheduleConfig, Task, TaskType};
use crate::scheduler::ControlCmd;
use desirable::{Request, Response, Router};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::mpsc;

type ApiResult = std::result::Result<Response, Response>;

fn ok_response(r: Response) -> ApiResult {
    Ok(r)
}

fn json_response<T>(data: T) -> Response
where
    T: serde::Serialize + Send + Sync + 'static,
{
    Response::json(data).unwrap()
}

fn error_response(status: u16, message: &str) -> Response {
    Response::with_status(
        status,
        serde_json::json!({ "error": message }).to_string(),
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

#[derive(Deserialize)]
struct ExecQuery {
    task_id: Option<String>,
    limit: Option<u32>,
}

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

    Task::new(body.name, task_type, schedule)
}

fn build_update_task(id: String, body: CreateTaskRequest) -> Task {
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
    Task {
        id,
        name: body.name,
        task_type,
        enabled: true,
        schedule,
        created_at: now.clone(),
        updated_at: now,
    }
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
            let body: CreateTaskRequest = req
                .body()
                .await
                .map_err(|e| error_response(400, &format!("invalid body: {e}")))?;
            let task = build_task(body);
            db.create_task(&task)
                .await
                .map_err(|e| error_response(500, &format!("db error: {e}")))?;
            let _ = tx.send(ControlCmd::Add(task.clone()));
            ok_response(json_response(task))
        }
    });

    let db_get_all = db.clone();
    router.get("/api/tasks", move |_req: Request| {
        let db = db_get_all.clone();
        async move {
            let tasks = db
                .list_all_tasks()
                .await
                .map_err(|e| error_response(500, &format!("db error: {e}")))?;
            ok_response(json_response(tasks))
        }
    });

    let db_get_one = db.clone();
    router.get("/api/tasks/:id", move |req: Request| {
        let db = db_get_one.clone();
        async move {
            let id: String = req
                .param("id")
                .map_err(|_| error_response(400, "missing id"))?;
            match db
                .get_task(&id)
                .await
                .map_err(|e| error_response(500, &format!("db error: {e}")))?
            {
                Some(task) => ok_response(json_response(task)),
                None => Err(error_response(404, "not found")),
            }
        }
    });

    let db_update = db.clone();
    let tx_update = cmd_tx.clone();
    router.put("/api/tasks/:id", move |mut req: Request| {
        let db = db_update.clone();
        let tx = tx_update.clone();
        async move {
            let id: String = req
                .param("id")
                .map_err(|_| error_response(400, "missing id"))?;
            let body: CreateTaskRequest = req
                .body()
                .await
                .map_err(|e| error_response(400, &format!("invalid body: {e}")))?;
            let task = build_update_task(id, body);
            db.update_task(&task)
                .await
                .map_err(|e| error_response(500, &format!("db error: {e}")))?;
            let _ = tx.send(ControlCmd::Update(task.clone()));
            ok_response(json_response(task))
        }
    });

    let db_delete = db.clone();
    let tx_delete = cmd_tx.clone();
    router.delete("/api/tasks/:id", move |req: Request| {
        let db = db_delete.clone();
        let tx = tx_delete.clone();
        async move {
            let id: String = req
                .param("id")
                .map_err(|_| error_response(400, "missing id"))?;
            match db
                .delete_task(&id)
                .await
                .map_err(|e| error_response(500, &format!("db error: {e}")))?
            {
                true => {
                    let _ = tx.send(ControlCmd::Remove(id));
                    let resp = serde_json::json!({ "deleted": true });
                    ok_response(json_response(resp))
                }
                false => Err(error_response(404, "not found")),
            }
        }
    });

    let db_enable = db.clone();
    let tx_enable = cmd_tx.clone();
    router.post("/api/tasks/:id/enable", move |req: Request| {
        let db = db_enable.clone();
        let tx = tx_enable.clone();
        async move {
            let id: String = req
                .param("id")
                .map_err(|_| error_response(400, "missing id"))?;
            match db
                .set_enabled(&id, true)
                .await
                .map_err(|e| error_response(500, &format!("db error: {e}")))?
            {
                true => {
                    if let Ok(Some(task)) = db.get_task(&id).await {
                        let _ = tx.send(ControlCmd::Add(task));
                    }
                    let resp = serde_json::json!({ "enabled": true });
                    ok_response(json_response(resp))
                }
                false => Err(error_response(404, "not found")),
            }
        }
    });

    let db_disable = db.clone();
    let tx_disable = cmd_tx.clone();
    router.post("/api/tasks/:id/disable", move |req: Request| {
        let db = db_disable.clone();
        let tx = tx_disable.clone();
        async move {
            let id: String = req
                .param("id")
                .map_err(|_| error_response(400, "missing id"))?;
            match db
                .set_enabled(&id, false)
                .await
                .map_err(|e| error_response(500, &format!("db error: {e}")))?
            {
                true => {
                    let _ = tx.send(ControlCmd::Remove(id));
                    let resp = serde_json::json!({ "enabled": false });
                    ok_response(json_response(resp))
                }
                false => Err(error_response(404, "not found")),
            }
        }
    });

    let db_exec_list = db.clone();
    router.get("/api/executions", move |req: Request| {
        let db = db_exec_list.clone();
        async move {
            let query_parts: Option<ExecQuery> = req
                .query()
                .map_err(|e| error_response(400, &format!("invalid query: {e}")))?;
            let task_id = query_parts.as_ref().and_then(|q| q.task_id.as_deref());
            let limit = query_parts.as_ref().and_then(|q| q.limit);
            let execs = db
                .list_executions(task_id, limit)
                .await
                .map_err(|e| error_response(500, &format!("db error: {e}")))?;
            ok_response(json_response(execs))
        }
    });

    let db_exec_one = db.clone();
    router.get("/api/executions/:id", move |req: Request| {
        let db = db_exec_one.clone();
        async move {
            let id: String = req
                .param("id")
                .map_err(|_| error_response(400, "missing id"))?;
            match db
                .get_execution(&id)
                .await
                .map_err(|e| error_response(500, &format!("db error: {e}")))?
            {
                Some(exec) => ok_response(json_response(exec)),
                None => Err(error_response(404, "not found")),
            }
        }
    });

    router
}
