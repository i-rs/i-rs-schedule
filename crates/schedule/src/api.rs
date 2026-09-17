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
    max_concurrent: Option<i64>,
    trigger_task_ids: Option<Vec<String>>,
    tags: Option<Vec<String>>,
    trigger_on: Option<String>,
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
    let max_concurrent = body.max_concurrent.unwrap_or(1);
    if !(0..=64).contains(&max_concurrent) {
        return Err("max_concurrent must be between 0 and 64 (0 = unlimited)".into());
    }
    let mut trigger_task_ids = body.trigger_task_ids.unwrap_or_default();
    trigger_task_ids.retain(|id| !id.trim().is_empty());
    let mut tags: Vec<String> = body
        .tags
        .unwrap_or_default()
        .into_iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    tags.dedup();
    if tags.len() > 20 {
        return Err("too many tags (max 20)".into());
    }
    for tag in &tags {
        if tag.len() > 32 {
            return Err("tag too long (max 32 bytes)".into());
        }
    }
    if trigger_task_ids.contains(&id_hint) {
        return Err("a task cannot trigger itself".into());
    }
    let trigger_on = body.trigger_on.unwrap_or_else(|| "success".into());
    if !matches!(trigger_on.as_str(), "success" | "failure" | "always") {
        return Err(format!("invalid trigger_on: {trigger_on}"));
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
    task.max_concurrent = max_concurrent;
    task.trigger_task_ids = trigger_task_ids;
    task.tags = tags;
    task.trigger_on = trigger_on;
    Ok(task)
}

/// 认证设置:静态机器 token 与管理员账号(有其一则启用认证)。
#[derive(Debug, Clone, Default)]
pub struct AuthSettings {
    pub static_token: Option<String>,
    pub has_admin: bool,
}

/// Bearer 认证中间件:静态 token / 会话 / API token 任一匹配即放行;
/// /healthz 与 /metrics 豁免(探活与抓取不应依赖凭据)。
struct Auth {
    static_token: Option<String>,
    has_admin: bool,
    db: std::sync::Arc<Db>,
}

#[async_trait::async_trait]
impl Middleware for Auth {
    async fn handle(&self, req: Request, next: Next<'_>) -> DesirableResult {
        // 探活、指标与登录端点豁免(登录是获取凭据的入口)
        if req.path().starts_with("/healthz")
            || req.path().starts_with("/metrics")
            || req.path().starts_with("/api/auth/login")
        {
            return next.run(req).await;
        }
        if self.static_token.is_none() && !self.has_admin {
            // 开放模式:未配置任何认证
            return next.run(req).await;
        }
        let bearer = req
            .inner
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|v| v.to_string());
        if let Some(token) = bearer {
            if self.static_token.as_deref() == Some(token.as_str()) {
                return next.run(req).await;
            }
            if self.db.session_valid(&token).await.unwrap_or(false) {
                return next.run(req).await;
            }
            if self.db.api_token_valid(&token).await.unwrap_or(false) {
                return next.run(req).await;
            }
        }
        Ok(err_msg(401, "unauthorized"))
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
                if t.trigger_task_ids.is_empty() {
                    return Ok(());
                }
                for next in &t.trigger_task_ids {
                    if next == source_id {
                        return Err(err_msg(400, "circular trigger chain"));
                    }
                }
                // 多下游时逐支检查太深,这里沿第一个下游继续(足够防环)
                current = t.trigger_task_ids[0].clone();
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
    auth: AuthSettings,
    maintenance: crate::scheduler::Maintenance,
    backup: crate::backup::BackupState,
) -> Router {
    let db = Arc::new(db);
    let cmd_tx = Arc::new(cmd_tx);

    let mut router = Router::new();

    let db_post = db.clone();
    let tx_post = cmd_tx.clone();
    let ev_post = executor.events().clone();
    router.post("/api/tasks", move |mut req: Request| {
        let db = db_post.clone();
        let tx = tx_post.clone();
        let ev = ev_post.clone();
        async move {
            let body: CreateTaskRequest = req
                .body()
                .await
                .map_err(|e| err_msg(400, format!("invalid body: {e}")))?;
            let task = build_task(body).map_err(|e| err_msg(400, e))?;
            for target in &task.trigger_task_ids {
                check_trigger_chain(&db, &task.id, target).await?;
            }
            db.create_task(&task)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?;
            let _ = db
                .audit("task.create", &format!("created task {}", task.name))
                .await;
            let _ = tx.send(ControlCmd::Add(task.clone()));
            ev.bump();
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
    let ev_update = executor.events().clone();
    router.put("/api/tasks/:id", move |mut req: Request| {
        let db = db_update.clone();
        let tx = tx_update.clone();
        let ev = ev_update.clone();
        async move {
            let id: String = req.param("id").map_err(|_| err_msg(400, "missing id"))?;
            let body: CreateTaskRequest = req
                .body()
                .await
                .map_err(|e| err_msg(400, format!("invalid body: {e}")))?;
            let mut task = build_task_with_hint(body, id.clone()).map_err(|e| err_msg(400, e))?;
            task.id = id.clone();
            for target in &task.trigger_task_ids {
                check_trigger_chain(&db, &id, target).await?;
            }
            task.updated_at = crate::db::now_iso();
            db.update_task(&task)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?;
            let _ = db
                .audit("task.update", &format!("updated task {}", task.name))
                .await;
            let _ = tx.send(ControlCmd::Update(task.clone()));
            ev.bump();
            ok(with_next(task))
        }
    });

    let db_delete = db.clone();
    let tx_delete = cmd_tx.clone();
    let ev_delete = executor.events().clone();
    router.delete("/api/tasks/:id", move |req: Request| {
        let db = db_delete.clone();
        let tx = tx_delete.clone();
        let ev = ev_delete.clone();
        async move {
            let id: String = req.param("id").map_err(|_| err_msg(400, "missing id"))?;
            match db
                .delete_task(&id)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?
            {
                true => {
                    let _ = db.audit("task.delete", &format!("deleted task {id}")).await;
                    let _ = tx.send(ControlCmd::Remove(id));
                    ev.bump();
                    ok(serde_json::json!({ "deleted": true }))
                }
                false => Err(err_msg(404, "not found")),
            }
        }
    });

    // 批量操作:逐个复用单任务逻辑(含 scheduler 控制命令与审计)。
    let db_batch = db.clone();
    let tx_batch = cmd_tx.clone();
    let ev_batch = executor.events().clone();
    router.post("/api/tasks/batch", move |mut req: Request| {
        let db = db_batch.clone();
        let tx = tx_batch.clone();
        let ev = ev_batch.clone();
        async move {
            #[derive(Deserialize)]
            struct BatchBody {
                ids: Vec<String>,
                action: String,
            }
            let body: BatchBody = req
                .body()
                .await
                .map_err(|e| err_msg(400, format!("invalid body: {e}")))?;
            if !matches!(body.action.as_str(), "enable" | "disable" | "delete") {
                return Err(err_msg(400, "action must be enable | disable | delete"));
            }
            let mut changed = 0u64;
            for id in &body.ids {
                let result = match body.action.as_str() {
                    "enable" => {
                        let ok = db
                            .set_enabled(id, true)
                            .await
                            .map_err(|e| err_msg(500, format!("db error: {e}")))?;
                        if ok {
                            if let Ok(Some(task)) = db.get_task(id).await {
                                let _ = tx.send(ControlCmd::Add(task));
                            }
                            let _ = db
                                .audit("task.enable", &format!("batch enabled task {id}"))
                                .await;
                        }
                        ok
                    }
                    "disable" => {
                        let ok = db
                            .set_enabled(id, false)
                            .await
                            .map_err(|e| err_msg(500, format!("db error: {e}")))?;
                        if ok {
                            let _ = tx.send(ControlCmd::Remove(id.clone()));
                            let _ = db
                                .audit("task.disable", &format!("batch disabled task {id}"))
                                .await;
                        }
                        ok
                    }
                    _ => {
                        let ok = db
                            .delete_task(id)
                            .await
                            .map_err(|e| err_msg(500, format!("db error: {e}")))?;
                        if ok {
                            let _ = tx.send(ControlCmd::Remove(id.clone()));
                            let _ = db
                                .audit("task.delete", &format!("batch deleted task {id}"))
                                .await;
                        }
                        ok
                    }
                };
                if result {
                    changed += 1;
                }
            }
            if changed > 0 {
                ev.bump();
            }
            ok(serde_json::json!({ "changed": changed }))
        }
    });

    let db_enable = db.clone();
    let tx_enable = cmd_tx.clone();
    let ev_enable = executor.events().clone();
    router.post("/api/tasks/:id/enable", move |req: Request| {
        let db = db_enable.clone();
        let tx = tx_enable.clone();
        let ev = ev_enable.clone();
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
                    ev.bump();
                    ok(serde_json::json!({ "enabled": true }))
                }
                false => Err(err_msg(404, "not found")),
            }
        }
    });

    let db_disable = db.clone();
    let tx_disable = cmd_tx.clone();
    let ev_disable = executor.events().clone();
    router.post("/api/tasks/:id/disable", move |req: Request| {
        let db = db_disable.clone();
        let tx = tx_disable.clone();
        let ev = ev_disable.clone();
        async move {
            let id: String = req.param("id").map_err(|_| err_msg(400, "missing id"))?;
            match db
                .set_enabled(&id, false)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?
            {
                true => {
                    let _ = tx.send(ControlCmd::Remove(id));
                    ev.bump();
                    ok(serde_json::json!({ "enabled": false }))
                }
                false => Err(err_msg(404, "not found")),
            }
        }
    });

    let ex_live = executor.clone();
    let ex_events = executor.clone();
    let ev_import = executor.events().clone();
    let ev_maint = executor.events().clone();
    let ev_vars = executor.events().clone();
    let maintenance_get = maintenance.clone();
    let maintenance_post = maintenance.clone();
    let db_run = db.clone();
    let executor_ntest = executor.clone();
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
            let _ = db
                .audit("task.run", &format!("manually ran task {}", task.name))
                .await;
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

    // 实时输出:长轮询。live 条目存在 → 按 cursor 等新快照;
    // 不存在(已完成 / 重启后)→ 回落 DB,done 即终止前端轮询。
    {
        let db_live = db.clone();
        router.get("/api/executions/:id/live", move |req: Request| {
            let db = db_live.clone();
            let ex = ex_live.clone();
            async move {
                let id: String = req.param("id").map_err(|_| err_msg(400, "missing id"))?;
                #[derive(Deserialize)]
                struct LiveQuery {
                    cursor: Option<u64>,
                }
                let cursor = req
                    .query::<LiveQuery>()
                    .ok()
                    .flatten()
                    .and_then(|q| q.cursor)
                    .unwrap_or(0);
                if let Some(live) = ex.live().get(&id) {
                    let snap = live
                        .wait_snapshot(cursor, std::time::Duration::from_secs(25))
                        .await;
                    return ok(serde_json::json!({
                        "version": snap.version,
                        "output": snap.output,
                        "total": snap.total,
                        "done": snap.done,
                    }));
                }
                match db
                    .get_execution(&id)
                    .await
                    .map_err(|e| err_msg(500, format!("db error: {e}")))?
                {
                    Some(exec) => ok(serde_json::json!({
                        "version": 0,
                        "output": exec.output,
                        "total": serde_json::Value::Null,
                        "done": exec.status != "running",
                        "status": exec.status,
                    })),
                    None => Err(err_msg(404, "not found")),
                }
            }
        });
    }

    // 事件长轮询:任何执行/任务变化推进游标,前端据此触发刷新。
    router.get("/api/events", move |req: Request| {
        let ex = ex_events.clone();
        async move {
            #[derive(Deserialize)]
            struct EventsQuery {
                cursor: Option<u64>,
            }
            let cursor = req
                .query::<EventsQuery>()
                .ok()
                .flatten()
                .and_then(|q| q.cursor)
                .unwrap_or(0);
            let next = ex
                .events()
                .wait_for(cursor, std::time::Duration::from_secs(25))
                .await;
            ok(serde_json::json!({ "cursor": next }))
        }
    });

    // 维护模式:查询与切换(持久化 + 广播 scheduler + 事件刷新)。
    {
        let db_maint = db.clone();
        let tx_maint = cmd_tx.clone();
        router.get("/api/maintenance", move |_req: Request| {
            let maintenance = maintenance_get.clone();
            async move { ok(serde_json::json!({ "enabled": maintenance.enabled() })) }
        });
        router.post("/api/maintenance", move |mut req: Request| {
            let db = db_maint.clone();
            let tx = tx_maint.clone();
            let ev = ev_maint.clone();
            let maintenance = maintenance_post.clone();
            async move {
                #[derive(Deserialize)]
                struct MaintenanceBody {
                    enabled: bool,
                }
                let body: MaintenanceBody = req
                    .body()
                    .await
                    .map_err(|e| err_msg(400, format!("invalid body: {e}")))?;
                maintenance.set(body.enabled);
                db.set_setting("maintenance", if body.enabled { "1" } else { "0" })
                    .await
                    .map_err(|e| err_msg(500, format!("db error: {e}")))?;
                let _ = tx.send(ControlCmd::SetMaintenance(body.enabled));
                let _ = db
                    .audit(
                        "maintenance",
                        if body.enabled {
                            "maintenance mode enabled"
                        } else {
                            "maintenance mode disabled"
                        },
                    )
                    .await;
                ev.bump();
                ok(serde_json::json!({ "enabled": body.enabled }))
            }
        });
    }

    // 全局变量:secret 的 value 永不回传前端。
    let db_vars_list = db.clone();
    let db_vars_set = db.clone();
    let db_vars_del = db.clone();
    let ev_vars_list = ev_vars.clone();
    let ev_vars_set = ev_vars.clone();
    let ev_vars_del = ev_vars.clone();
    router.get("/api/vars", move |_req: Request| {
        let db = db_vars_list.clone();
        let ev = ev_vars_list.clone();
        async move {
            let _ = ev;
            let vars = db
                .list_variables()
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?;
            let items: Vec<serde_json::Value> = vars
                .iter()
                .map(|v| {
                    serde_json::json!({
                        "key": v.key,
                        "value": if v.is_secret { serde_json::Value::Null } else { serde_json::Value::String(v.value.clone()) },
                        "is_secret": v.is_secret,
                    })
                })
                .collect();
            ok(items)
        }
    });
    router.post("/api/vars", move |mut req: Request| {
        let db = db_vars_set.clone();
        let ev = ev_vars_set.clone();
        async move {
            #[derive(Deserialize)]
            struct VarBody {
                key: String,
                value: String,
                #[serde(default)]
                is_secret: bool,
            }
            let body: VarBody = req
                .body()
                .await
                .map_err(|e| err_msg(400, format!("invalid body: {e}")))?;
            let key = body.key.trim().to_string();
            if key.is_empty()
                || key.len() > 64
                || !key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
            {
                return Err(err_msg(400, "key must be 1..=64 chars of [A-Za-z0-9_.-]"));
            }
            if body.value.len() > 8192 {
                return Err(err_msg(400, "value too long (max 8192 bytes)"));
            }
            db.upsert_variable(&crate::db::Variable {
                key,
                value: body.value,
                is_secret: body.is_secret,
            })
            .await
            .map_err(|e| err_msg(500, format!("db error: {e}")))?;
            ev.bump();
            ok(serde_json::json!({ "ok": true }))
        }
    });
    router.delete("/api/vars/:key", move |req: Request| {
        let db = db_vars_del.clone();
        let ev = ev_vars_del.clone();
        async move {
            let key: String = req.param("key").map_err(|_| err_msg(400, "missing key"))?;
            let removed = db
                .delete_variable(&key)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?;
            if removed {
                ev.bump();
            }
            ok(serde_json::json!({ "removed": removed }))
        }
    });

    // 健康检查:不认证、不查库,探活专用。
    {
        let maintenance_h = maintenance.clone();
        router.get("/healthz", move |_req: Request| {
            let maintenance = maintenance_h.clone();
            let backup = backup.clone();
            async move {
                ok(serde_json::json!({
                    "status": "ok",
                    "maintenance": maintenance.enabled(),
                    "last_backup": backup.get(),
                }))
            }
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

    // 通知测试:用真实渠道配置同步发送一条测试消息,返回投递结果。
    {
        let db_ntest = db.clone();
        router.post("/api/tasks/:id/notify-test", move |req: Request| {
            let db = db_ntest.clone();
            let executor = executor_ntest.clone();
            async move {
                let id: String = req.param("id").map_err(|_| err_msg(400, "missing id"))?;
                let task = db
                    .get_task(&id)
                    .await
                    .map_err(|e| err_msg(500, format!("db error: {e}")))?
                    .ok_or_else(|| err_msg(404, "not found"))?;
                if task.notify_type == "none" {
                    return Err(err_msg(400, "notifications not configured for this task"));
                }
                let test_exec = crate::db::TaskExecution {
                    id: "test".into(),
                    task_id: task.id.clone(),
                    attempt: 0,
                    status: "test".into(),
                    output: Some("This is a test notification from i-rs-schedule".into()),
                    http_status: None,
                    started_at: crate::db::now_iso(),
                    finished_at: Some(crate::db::now_iso()),
                };
                match executor
                    .notifier()
                    .send_sync_channel(
                        &task.notify_type,
                        &task.notify_url,
                        &task,
                        "test",
                        "测试通知",
                        &test_exec,
                        0,
                    )
                    .await
                {
                    Ok(()) => ok(serde_json::json!({ "delivered": true, "detail": "notification delivered" })),
                    Err(detail) => ok(serde_json::json!({ "delivered": false, "detail": detail })),
                }
            }
        });
    }

    // Cron 预览:返回未来 5 次触发时间(UTC ISO),供表单实时预览。
    {
        router.post("/api/cron/preview", move |mut req: Request| async move {
            #[derive(Deserialize)]
            struct CronPreview {
                expr: String,
                timezone: String,
            }
            let body: CronPreview = req
                .body()
                .await
                .map_err(|e| err_msg(400, format!("invalid body: {e}")))?;
            let tz = body
                .timezone
                .parse::<chrono_tz::Tz>()
                .map_err(|e| err_msg(400, format!("invalid timezone: {e}")))?;
            let schedule: cron::Schedule = body
                .expr
                .parse()
                .map_err(|e| err_msg(400, format!("invalid cron expr: {e}")))?;
            let times: Vec<String> = schedule
                .upcoming(tz)
                .take(5)
                .map(|t| {
                    t.with_timezone(&chrono::Utc)
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                })
                .collect();
            ok(serde_json::json!({ "times": times }))
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
                let mut by_day: std::collections::HashMap<String, (i64, i64, Option<f64>)> = rows
                    .into_iter()
                    .map(|(day, success, failure, avg)| (day, (success, failure, avg)))
                    .collect();
                let days: Vec<serde_json::Value> = (0..14)
                    .map(|i| {
                        let d = (chrono::Utc::now() - chrono::Duration::days(13 - i))
                            .format("%Y-%m-%d")
                            .to_string();
                        let (success, failure, avg) = by_day.remove(&d).unwrap_or((0, 0, None));
                        serde_json::json!({
                            "date": d,
                            "success": success,
                            "failure": failure,
                            "avg_duration_ms": avg.map(|v| (v * 10.0).round() / 10.0),
                        })
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
            let ev = ev_import.clone();
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
                if imported > 0 {
                    ev.bump();
                }
                ok(serde_json::json!({ "imported": imported, "skipped": skipped }))
            }
        });
    }

    // 可选认证:配置了静态 token 或管理员账号才启用。
    if auth.static_token.is_some() || auth.has_admin {
        router.with(Auth {
            static_token: auth.static_token,
            has_admin: auth.has_admin,
            db: db.clone(),
        });
    }

    // 登录:校验管理员账密,签发 30 天会话 token。
    if auth.has_admin {
        let db_login = db.clone();
        router.post("/api/auth/login", move |mut req: Request| {
            let db = db_login.clone();
            async move {
                #[derive(Deserialize)]
                struct LoginBody {
                    username: String,
                    password: String,
                }
                let body: LoginBody = req
                    .body()
                    .await
                    .map_err(|e| err_msg(400, format!("invalid body: {e}")))?;
                if !db
                    .verify_admin(&body.username, &body.password)
                    .await
                    .unwrap_or(false)
                {
                    return Err(err_msg(401, "invalid credentials"));
                }
                let (token, expires_at) = db
                    .create_session(30)
                    .await
                    .map_err(|e| err_msg(500, format!("session error: {e}")))?;
                let _ = db
                    .audit("login", &format!("user {} logged in", body.username))
                    .await;
                ok(serde_json::json!({ "token": token, "expires_at": expires_at }))
            }
        });
    }

    // Token 管理:列表 / 生成(明文仅返回一次)/ 吊销。
    {
        let db_list = db.clone();
        router.get("/api/auth/tokens", move |_req: Request| {
            let db = db_list.clone();
            async move {
                let rows = db
                    .list_api_tokens()
                    .await
                    .map_err(|e| err_msg(500, format!("db error: {e}")))?;
                let tokens: Vec<serde_json::Value> = rows
                    .into_iter()
                    .map(|(id, name, created_at)| {
                        serde_json::json!({ "id": id, "name": name, "created_at": created_at })
                    })
                    .collect();
                ok(tokens)
            }
        });

        let db_create = db.clone();
        router.post("/api/auth/tokens", move |mut req: Request| {
            let db = db_create.clone();
            async move {
                #[derive(Deserialize)]
                struct NewToken {
                    name: String,
                }
                let body: NewToken = req
                    .body()
                    .await
                    .map_err(|e| err_msg(400, format!("invalid body: {e}")))?;
                let (id, name, token) = db
                    .create_api_token(&body.name)
                    .await
                    .map_err(|e| err_msg(500, format!("db error: {e}")))?;
                ok(serde_json::json!({ "id": id, "name": name, "token": token }))
            }
        });

        let db_revoke = db.clone();
        router.delete("/api/auth/tokens/:id", move |req: Request| {
            let db = db_revoke.clone();
            async move {
                let id: String = req.param("id").map_err(|_| err_msg(400, "missing id"))?;
                let revoked = db
                    .revoke_api_token(&id)
                    .await
                    .map_err(|e| err_msg(500, format!("db error: {e}")))?;
                if revoked {
                    ok(serde_json::json!({ "revoked": true }))
                } else {
                    Err(err_msg(404, "not found"))
                }
            }
        });
    }

    // 审计日志查询。
    // (limit 通过 query string 传入)
    {
        let db_audit = db.clone();
        router.get("/api/audit", move |req: Request| {
            let db = db_audit.clone();
            async move {
                #[derive(Deserialize)]
                struct AuditQuery {
                    limit: Option<i64>,
                }
                let limit: i64 = req
                    .query::<AuditQuery>()
                    .ok()
                    .flatten()
                    .and_then(|q| q.limit)
                    .unwrap_or(100);
                let rows = db
                    .list_audit(limit)
                    .await
                    .map_err(|e| err_msg(500, format!("db error: {e}")))?;
                let items: Vec<serde_json::Value> = rows
                    .into_iter()
                    .map(|(ts, action, summary)| {
                        serde_json::json!({ "ts": ts, "action": action, "summary": summary })
                    })
                    .collect();
                ok(items)
            }
        });
    }

    router
}
