use crate::db::{Db, ScheduleConfig, Task, TaskExecution, TaskType};
use crate::executor::Executor;
use crate::scheduler::ControlCmd;
use desirable::{Request, Response, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Serialize)]
struct ApiResponse {
    code: u16,
    message: String,
    data: serde_json::Value,
}

fn ok<T: Serialize>(data: T) -> Result<Response, Response> {
    let body = ApiResponse {
        code: 0,
        message: "ok".into(),
        data: serde_json::to_value(&data).unwrap_or(serde_json::Value::Null),
    };
    Ok(Response::json(body))
}

fn err(status: u16, msg: String) -> Response {
    let body = ApiResponse {
        code: status,
        message: msg,
        data: serde_json::Value::Null,
    };
    Response::with_status(status, serde_json::to_string(&body).unwrap()).unwrap()
}

fn err_msg(status: u16, msg: impl Into<String>) -> Response {
    err(status, msg.into())
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
    enabled: Option<bool>,
}

#[derive(Deserialize)]
struct ExecQuery {
    task_id: Option<String>,
    limit: Option<u32>,
}

fn build_task(body: CreateTaskRequest) -> Result<Task, String> {
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

    schedule.validate()?;

    let mut task = Task::new(body.name, task_type, schedule);
    if let Some(enabled) = body.enabled {
        task.enabled = enabled;
    }
    Ok(task)
}

pub fn build_router(
    db: Db,
    cmd_tx: mpsc::UnboundedSender<ControlCmd>,
    executor: Arc<Executor>,
) -> Router {
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
                .map_err(|e| err_msg(400, format!("invalid body: {e}")))?;
            let task = build_task(body).map_err(|e| err_msg(400, e))?;
            db.create_task(&task)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?;
            let _ = tx.send(ControlCmd::Add(task.clone()));
            ok(task)
        }
    });

    let db_get_all = db.clone();
    router.get("/api/tasks", move |_req: Request| {
        let db = db_get_all.clone();
        async move {
            let tasks = db
                .list_all_tasks()
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?;
            ok(tasks)
        }
    });

    let db_get_one = db.clone();
    router.get("/api/tasks/:id", move |req: Request| {
        let db = db_get_one.clone();
        async move {
            let id: String = req.param("id").map_err(|_| err_msg(400, "missing id"))?;
            match db
                .get_task(&id)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?
            {
                Some(task) => ok(task),
                None => Err(err_msg(404, "not found")),
            }
        }
    });

    let db_update = db.clone();
    let tx_update = cmd_tx.clone();
    router.put("/api/tasks/:id", move |mut req: Request| {
        let db = db_update.clone();
        let tx = tx_update.clone();
        async move {
            let id: String = req.param("id").map_err(|_| err_msg(400, "missing id"))?;
            let body: CreateTaskRequest = req
                .body()
                .await
                .map_err(|e| err_msg(400, format!("invalid body: {e}")))?;
            let mut task = build_task(body).map_err(|e| err_msg(400, e))?;
            task.id = id;
            task.updated_at = crate::db::now_iso();
            db.update_task(&task)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?;
            let _ = tx.send(ControlCmd::Update(task.clone()));
            ok(task)
        }
    });

    let db_delete = db.clone();
    let tx_delete = cmd_tx.clone();
    router.delete("/api/tasks/:id", move |req: Request| {
        let db = db_delete.clone();
        let tx = tx_delete.clone();
        async move {
            let id: String = req.param("id").map_err(|_| err_msg(400, "missing id"))?;
            match db
                .delete_task(&id)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?
            {
                true => {
                    let _ = tx.send(ControlCmd::Remove(id));
                    ok(serde_json::json!({ "deleted": true }))
                }
                false => Err(err_msg(404, "not found")),
            }
        }
    });

    let db_enable = db.clone();
    let tx_enable = cmd_tx.clone();
    router.post("/api/tasks/:id/enable", move |req: Request| {
        let db = db_enable.clone();
        let tx = tx_enable.clone();
        async move {
            let id: String = req.param("id").map_err(|_| err_msg(400, "missing id"))?;
            match db
                .set_enabled(&id, true)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?
            {
                true => {
                    if let Ok(Some(task)) = db.get_task(&id).await {
                        let _ = tx.send(ControlCmd::Add(task));
                    }
                    ok(serde_json::json!({ "enabled": true }))
                }
                false => Err(err_msg(404, "not found")),
            }
        }
    });

    let db_disable = db.clone();
    let tx_disable = cmd_tx.clone();
    router.post("/api/tasks/:id/disable", move |req: Request| {
        let db = db_disable.clone();
        let tx = tx_disable.clone();
        async move {
            let id: String = req.param("id").map_err(|_| err_msg(400, "missing id"))?;
            match db
                .set_enabled(&id, false)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?
            {
                true => {
                    let _ = tx.send(ControlCmd::Remove(id));
                    ok(serde_json::json!({ "enabled": false }))
                }
                false => Err(err_msg(404, "not found")),
            }
        }
    });

    let db_run = db.clone();
    router.post("/api/tasks/:id/run", move |req: Request| {
        let db = db_run.clone();
        let exec = executor.clone();
        async move {
            let id: String = req.param("id").map_err(|_| err_msg(400, "missing id"))?;
            let task = db
                .get_task(&id)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?
                .ok_or_else(|| err_msg(404, "not found"))?;

            let exec_id = uuid::Uuid::new_v4().to_string();
            let started_at = chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string();
            let _ = db
                .create_execution(&TaskExecution {
                    id: exec_id.clone(),
                    task_id: task.id.clone(),
                    status: "running".to_string(),
                    output: None,
                    http_status: None,
                    started_at: started_at.clone(),
                    finished_at: None,
                })
                .await;

            let result = exec.execute(&task).await;
            let _ = db
                .update_execution(&exec_id, &result.status, &result.output, result.http_status)
                .await;

            let exec_record = db
                .get_execution(&exec_id)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?
                .unwrap();
            ok(exec_record)
        }
    });

    let db_exec_list = db.clone();
    router.get("/api/executions", move |req: Request| {
        let db = db_exec_list.clone();
        async move {
            let query_parts: Option<ExecQuery> = req
                .query()
                .map_err(|e| err_msg(400, format!("invalid query: {e}")))?;
            let task_id = query_parts.as_ref().and_then(|q| q.task_id.as_deref());
            let limit = query_parts.as_ref().and_then(|q| q.limit);
            let execs = db
                .list_executions(task_id, limit)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?;
            ok(execs)
        }
    });

    let db_exec_one = db.clone();
    router.get("/api/executions/:id", move |req: Request| {
        let db = db_exec_one.clone();
        async move {
            let id: String = req.param("id").map_err(|_| err_msg(400, "missing id"))?;
            match db
                .get_execution(&id)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?
            {
                Some(exec) => ok(exec),
                None => Err(err_msg(404, "not found")),
            }
        }
    });

    router
}
