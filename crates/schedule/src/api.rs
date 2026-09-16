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

/// API 响应的 Task 包装:附带 next_run_at 计算字段。
#[derive(Serialize)]
struct TaskWithNext {
    #[serde(flatten)]
    task: Task,
    next_run_at: Option<String>,
}

fn with_next(task: Task) -> TaskWithNext {
    let next_run_at = crate::schedule::next_run_at(&task);
    TaskWithNext { task, next_run_at }
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
    timezone: Option<String>,
    timeout_secs: Option<u64>,
    max_retries: Option<i64>,
    trigger_task_id: Option<String>,
    notify_type: Option<String>,
    notify_url: Option<String>,
}

#[derive(Deserialize)]
struct ExecQuery {
    task_id: Option<String>,
    limit: Option<u32>,
}

fn build_task(body: CreateTaskRequest) -> Result<Task, String> {
    build_task_with_hint(body, String::new())
}

fn build_task_with_hint(body: CreateTaskRequest, id_hint: String) -> Result<Task, String> {
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

    let timezone = body.timezone.unwrap_or_else(|| "UTC".into());
    crate::schedule::validate_timezone(&timezone)?;
    let timeout_secs = body.timeout_secs.unwrap_or(30);
    if timeout_secs == 0 || timeout_secs > 3600 {
        return Err("timeout_secs must be between 1 and 3600".into());
    }
    let max_retries = body.max_retries.unwrap_or(0);
    if !(0..=10).contains(&max_retries) {
        return Err("max_retries must be between 0 and 10".into());
    }
    let trigger_task_id = body.trigger_task_id.unwrap_or_default();
    if !trigger_task_id.is_empty() && trigger_task_id == id_hint {
        return Err("a task cannot trigger itself".into());
    }

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
    task.timezone = timezone;
    task.timeout_secs = timeout_secs;
    task.max_retries = max_retries;
    task.trigger_task_id = trigger_task_id;
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

/// 校验触发链:目标存在且沿链不会回到 source(最多 20 步)。
#[allow(clippy::result_large_err)]
async fn check_trigger_chain(db: &Db, source_id: &str, target_id: &str) -> Result<(), Response> {
    let mut current = target_id.to_string();
    for _ in 0..20 {
        if current == source_id {
            return Err(err_msg(400, "circular trigger chain"));
        }
        match db.get_task(&current).await {
            Ok(Some(t)) => {
                if t.trigger_task_id.is_empty() {
                    return Ok(());
                }
                current = t.trigger_task_id;
            }
            Ok(None) => return Err(err_msg(400, format!("trigger target {current} not found"))),
            Err(e) => return Err(err_msg(500, format!("db error: {e}"))),
        }
    }
    Err(err_msg(400, "trigger chain too long (max 20)"))
}

pub fn build_router(
    db: Db,
    cmd_tx: mpsc::UnboundedSender<ControlCmd>,
    executor: Arc<Executor>,
    auth_token: Option<String>,
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
            if !task.trigger_task_id.is_empty() {
                check_trigger_chain(&db, &task.id, &task.trigger_task_id).await?;
            }
            db.create_task(&task)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?;
            let _ = tx.send(ControlCmd::Add(task.clone()));
            ok(with_next(task))
        }
    });

    let db_get_all = db.clone();
    router.get("/api/tasks", move |_req: Request| {
        let db = db_get_all.clone();
        async move {
            let tasks = db
                .list_all_tasks()
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?
                .into_iter()
                .map(with_next)
                .collect::<Vec<_>>();
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
                Some(task) => ok(with_next(task)),
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
            let mut task = build_task_with_hint(body, id.clone()).map_err(|e| err_msg(400, e))?;
            task.id = id.clone();
            if !task.trigger_task_id.is_empty() {
                check_trigger_chain(&db, &id, &task.trigger_task_id).await?;
            }
            task.updated_at = crate::db::now_iso();
            db.update_task(&task)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?;
            let _ = tx.send(ControlCmd::Update(task.clone()));
            ok(with_next(task))
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

    // 健康检查:不认证、不查库,探活专用。
    {
        router.get("/healthz", move |_req: Request| async move {
            ok(serde_json::json!({ "status": "ok" }))
        });
    }

    // Prometheus 指标(文本 exposition 格式)。
    {
        let db_metrics = db.clone();
        router.get("/metrics", move |_req: Request| {
            let db = db_metrics.clone();
            async move {
                let (total, success, failure, enabled) = db.stats().await.unwrap_or((0, 0, 0, 0));
                let body = format!(
                    "# HELP irs_executions_total Total number of task executions recorded.\n\
                     # TYPE irs_executions_total counter\n\
                     irs_executions_total {total}\n\
                     # HELP irs_executions_success_total Successful task executions.\n\
                     # TYPE irs_executions_success_total counter\n\
                     irs_executions_success_total {success}\n\
                     # HELP irs_executions_failure_total Failed task executions.\n\
                     # TYPE irs_executions_failure_total counter\n\
                     irs_executions_failure_total {failure}\n\
                     # HELP irs_tasks_enabled Currently enabled tasks.\n\
                     # TYPE irs_tasks_enabled gauge\n\
                     irs_tasks_enabled {enabled}\n"
                );
                // body 为纯文本 String,with_status 实际不会失败(与 err_msg 的 unwrap 同理)。
                let resp: Response = Response::with_status(200, body).unwrap();
                Ok::<Response, Response>(resp)
            }
        });
    }

    // 单任务执行统计(详情抽屉用)。
    {
        let db_tstats = db.clone();
        router.get("/api/tasks/:id/stats", move |req: Request| {
            let db = db_tstats.clone();
            async move {
                let id: String = req.param("id").map_err(|_| err_msg(400, "missing id"))?;
                let (total, success, failure, avg_ms) = db
                    .task_stats(&id)
                    .await
                    .map_err(|e| err_msg(500, format!("db error: {e}")))?;
                ok(serde_json::json!({
                    "total": total,
                    "success": success,
                    "failure": failure,
                    "avg_duration_ms": avg_ms.map(|v| (v * 10.0).round() / 10.0),
                }))
            }
        });
    }

    // 近 14 天每日执行统计(success/failure),供 Dashboard 图表。
    {
        let db_daily = db.clone();
        router.get("/api/stats/daily", move |_req: Request| {
            let db = db_daily.clone();
            async move {
                let cutoff = (chrono::Utc::now() - chrono::Duration::days(13))
                    .format("%Y-%m-%d")
                    .to_string();
                let rows = db
                    .daily_stats(cutoff)
                    .await
                    .map_err(|e| err_msg(500, format!("db error: {e}")))?;
                let mut by_day: std::collections::HashMap<String, (i64, i64)> = rows
                    .into_iter()
                    .map(|(day, success, failure)| (day, (success, failure)))
                    .collect();
                let days: Vec<serde_json::Value> = (0..14)
                    .map(|i| {
                        let d = (chrono::Utc::now() - chrono::Duration::days(13 - i))
                            .format("%Y-%m-%d")
                            .to_string();
                        let (success, failure) = by_day.remove(&d).unwrap_or((0, 0));
                        serde_json::json!({ "date": d, "success": success, "failure": failure })
                    })
                    .collect();
                ok(days)
            }
        });
    }

    // 导出:全部任务(不含执行历史)。
    {
        let db_export = db.clone();
        router.get("/api/export/tasks", move |_req: Request| {
            let db = db_export.clone();
            async move {
                let tasks = db
                    .list_all_tasks()
                    .await
                    .map_err(|e| err_msg(500, format!("db error: {e}")))?;
                ok(serde_json::json!({ "version": "0.2", "tasks": tasks }))
            }
        });
    }

    // 导入:跳过已存在的 id,返回 {imported, skipped}。
    {
        let db_import = db.clone();
        router.post("/api/import/tasks", move |mut req: Request| {
            let db = db_import.clone();
            async move {
                #[derive(Deserialize)]
                struct ImportBody {
                    tasks: Vec<Task>,
                }
                let body: ImportBody = req
                    .body()
                    .await
                    .map_err(|e| err_msg(400, format!("invalid body: {e}")))?;
                let mut imported = 0u64;
                let mut skipped = 0u64;
                for task in body.tasks {
                    match db.get_task(&task.id).await {
                        Ok(Some(_)) => {
                            skipped += 1;
                        }
                        Ok(None) => match db.create_task(&task).await {
                            Ok(()) => imported += 1,
                            Err(e) => return Err(err_msg(500, format!("db error: {e}"))),
                        },
                        Err(e) => return Err(err_msg(500, format!("db error: {e}"))),
                    }
                }
                ok(serde_json::json!({ "imported": imported, "skipped": skipped }))
            }
        });
    }

    // 可选 Bearer 认证:配置了 token(环境变量或 config.toml)则全 API 要求携带。
    if let Some(token) = auth_token
        && !token.is_empty()
    {
        router.with(Auth { token });
    }

    router
}
