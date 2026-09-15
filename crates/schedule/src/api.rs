use crate::db::{Db, ScheduleConfig, Task, TaskType};
use crate::executor::Executor;
use crate::scheduler::ControlCmd;
use desirable::{Middleware, Next, Request, Response, Result as DesirableResult, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Serialize)]
struct ApiResponse {
    code: u16,
    message: String,
    data: serde_json::Value,
}

// Response 的 Err 变体较大(由 desirable 框架定义,无法瘦身);
// boxing 会改变返回类型引发连锁,故 allow 掉 result_large_err。
#[allow(clippy::result_large_err)]
fn ok<T: Serialize>(data: T) -> Result<Response, Response> {
    let body = ApiResponse {
        code: 0,
        message: "ok".into(),
        data: serde_json::to_value(&data).unwrap_or(serde_json::Value::Null),
    };
    Ok(Response::json(body))
}

fn err_msg(status: u16, msg: impl Into<String>) -> Response {
    let body = ApiResponse {
        code: status,
        message: msg.into(),
        data: serde_json::Value::Null,
    };
    Response::with_status(status, serde_json::to_string(&body).unwrap()).unwrap()
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
    notify_type: Option<String>,
    notify_url: Option<String>,
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

    let notify_type = body.notify_type.unwrap_or_else(|| "none".into());
    let notify_url = body.notify_url.unwrap_or_default();
    match notify_type.as_str() {
        "none" => {}
        "webhook" | "feishu" | "dingtalk" => {
            if notify_url.trim().is_empty() {
                return Err(format!(
                    "notify_url is required when notify_type is {notify_type}"
                ));
            }
        }
        other => return Err(format!("invalid notify_type: {other}")),
    }

    let mut task = Task::new(body.name, task_type, schedule);
    if let Some(enabled) = body.enabled {
        task.enabled = enabled;
    }
    task.notify_type = notify_type;
    task.notify_url = notify_url;
    Ok(task)
}

/// 可选 Bearer 认证中间件:token 匹配才放行,否则 401。
struct Auth {
    token: String,
}

#[async_trait::async_trait]
impl Middleware for Auth {
    async fn handle(&self, req: Request, next: Next<'_>) -> DesirableResult {
        let expected = format!("Bearer {}", self.token);
        let authorized = req
            .inner
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == expected);
        if authorized {
            next.run(req).await
        } else {
            Ok(err_msg(401, "unauthorized"))
        }
    }
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

            let exec_record = exec.execute_and_record(&db, &task).await;
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

    // 可选 Bearer 认证:设置 SCHEDULE_TOKEN 后全 API 要求携带,否则 401。
    if let Ok(token) = std::env::var("SCHEDULE_TOKEN")
        && !token.is_empty()
    {
        router.with(Auth { token });
    }

    router
}
