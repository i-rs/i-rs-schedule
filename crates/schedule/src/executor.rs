use crate::db::{Db, Task, TaskExecution, TaskType};
use crate::notify::Notifier;
use std::collections::HashMap;
use std::time::Duration;

/// 链式触发的上下文:触发方(上游)的最终输出与状态。
#[derive(Debug, Clone)]
pub struct TriggerContext {
    pub output: String,
    pub status: String,
}

/// Webhook 触发的上下文:请求体与 query 参数。
#[derive(Debug, Clone)]
pub struct EventContext {
    pub body: String,
    pub query: HashMap<String, String>,
}

/// 单次执行的插值上下文(变量一次性从 DB 加载)。
type Vars = HashMap<String, String>;

/// 统一插值:{{trigger.*}} / {{event.*}} / {{var.*}};逐类单遍替换,不递归展开。
fn interpolate(
    input: &str,
    vars: &Vars,
    trigger: Option<&TriggerContext>,
    event: Option<&EventContext>,
) -> String {
    let mut s = input.to_string();
    if let Some(ctx) = trigger {
        s = s
            .replace("{{trigger.output}}", &truncate_str(&ctx.output, 10_000))
            .replace("{{trigger.status}}", &ctx.status);
    }
    if let Some(ev) = event {
        s = s.replace("{{event.body}}", &truncate_str(&ev.body, 10_000));
        for (k, v) in &ev.query {
            s = s.replace(&format!("{{{{event.query.{k}}}}}"), v);
        }
    }
    for (k, v) in vars {
        s = s.replace(&format!("{{{{var.{k}}}}}"), v);
    }
    s
}

/// HTTP headers 的值做插值(非字符串值原样保留)。
fn interpolate_headers(
    headers: &serde_json::Value,
    vars: &Vars,
    trigger: Option<&TriggerContext>,
    event: Option<&EventContext>,
) -> serde_json::Value {
    match headers {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, v)| match v {
                    serde_json::Value::String(s) => (
                        k.clone(),
                        serde_json::Value::String(interpolate(s, vars, trigger, event)),
                    ),
                    other => (k.clone(), other.clone()),
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

pub struct ExecutionResult {
    pub status: String,
    pub output: String,
    pub http_status: Option<i64>,
}

#[derive(Clone)]
pub struct Executor {
    client: reqwest::Client,
    notifier: Notifier,
    /// 全局通知渠道(任务未配置时的回落):(notify_type, notify_url)
    global_notify: Option<(String, String)>,
    /// 输出持久化上限(KB,0 = 不限)
    max_output_kb: usize,
    /// 运行中 execution 的实时输出注册表(live 端点用)
    live: crate::live::LiveRegistry,
    /// 全局变更事件(前端事件长轮询用)
    events: crate::live::Events,
    /// 任务级在途执行计数:max_concurrent 互斥用
    in_flight: std::sync::Arc<std::sync::Mutex<HashMap<String, usize>>>,
    /// 全局并发兜底:所有执行共享的信号量(超限等待)
    global_permits: std::sync::Arc<tokio::sync::Semaphore>,
}

impl Executor {
    /// 通知器访问(通知测试端点用)。
    pub fn notifier(&self) -> &Notifier {
        &self.notifier
    }

    pub fn with_global_concurrency(
        global_notify: Option<(String, String)>,
        max_output_kb: usize,
        events: crate::live::Events,
        global_max_concurrent: usize,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            notifier: Notifier::new(),
            global_notify,
            max_output_kb,
            live: crate::live::LiveRegistry::default(),
            events,
            in_flight: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
            global_permits: std::sync::Arc::new(tokio::sync::Semaphore::new(global_max_concurrent)),
        }
    }

    /// 实时输出注册表访问(live 端点用)。
    pub fn live(&self) -> &crate::live::LiveRegistry {
        &self.live
    }

    /// 全局事件访问(events 端点与各变更埋点用)。
    pub fn events(&self) -> &crate::live::Events {
        &self.events
    }

    /// 执行任务;shell cmd 与 http url/body/header 值支持
    /// {{trigger.*}} / {{event.*}} / {{var.*}} 插值。
    /// live 为该次 execution 的实时输出句柄:Shell 增量写入,HTTP 忽略。
    async fn execute_with_trigger(
        &self,
        task: &Task,
        trigger: Option<&TriggerContext>,
        event: Option<&EventContext>,
        vars: &Vars,
        live: &std::sync::Arc<crate::live::LiveOutput>,
    ) -> ExecutionResult {
        match &task.task_type {
            TaskType::Http {
                method,
                url,
                headers,
                body,
            } => {
                let url = interpolate(url, vars, trigger, event);
                let body = body
                    .as_deref()
                    .map(|b| interpolate(b, vars, trigger, event));
                let headers = headers
                    .as_ref()
                    .map(|h| interpolate_headers(h, vars, trigger, event));
                self.execute_http(method, &url, &headers, body.as_deref(), task.timeout_secs)
                    .await
            }
            TaskType::Shell { cmd } => {
                let cmd = interpolate(cmd, vars, trigger, event);
                Self::execute_shell(&cmd, Duration::from_secs(task.timeout_secs), live).await
            }
        }
    }

    /// 执行任务并记录到 DB。封装 create_execution → execute → update_execution,
    /// scheduler 与 `/run` 端点共用,保证行为一致。
    ///
    /// create_execution 失败时返回一个内存构造的 `status="skipped"` 记录(不入库),
    /// 并打 warn 日志;execute 与 update_execution 的失败均告警但不影响返回。
    pub async fn execute_and_record(&self, db: &Db, task: &Task) -> TaskExecution {
        self.execute_and_record_depth(db, task, 0, None, None).await
    }

    /// Webhook 等事件触发的执行入口(与 cron/manual 同一并发/重试/通知管线)。
    pub async fn execute_and_record_event(
        &self,
        db: &Db,
        task: &Task,
        event: EventContext,
    ) -> TaskExecution {
        self.execute_and_record_depth(db, task, 0, None, Some(event))
            .await
    }

    /// depth 用于链式触发的环防护(最大 10 层)。装箱返回以打破递归 future 的 Send 推导。
    pub fn execute_and_record_depth<'a>(
        &'a self,
        db: &'a Db,
        task: &'a Task,
        depth: u32,
        trigger: Option<TriggerContext>,
        event: Option<EventContext>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = TaskExecution> + Send + 'a>> {
        Box::pin(self.execute_and_record_depth_impl(db, task, depth, trigger, event))
    }

    async fn execute_and_record_depth_impl(
        &self,
        db: &Db,
        task: &Task,
        depth: u32,
        trigger: Option<TriggerContext>,
        event: Option<EventContext>,
    ) -> TaskExecution {
        // 任务级并发互斥:达到 max_concurrent 即跳过并落库(重试退避期间占住槽位)。
        // max_concurrent = 0 表示不限并行。
        if task.max_concurrent > 0 && !self.acquire_slot(&task.id, task.max_concurrent) {
            return self
                .record_skipped(
                    db,
                    task,
                    "overlaps with a previous run (max_concurrent reached)",
                )
                .await;
        }
        // 全局并发兜底:超限等待(信号量 guard 在作用域结束时释放)
        let _permit = self.global_permits.clone().acquire_owned().await;

        let result = self.run_with_retries(db, task, depth, trigger, event).await;

        if task.max_concurrent > 0 {
            self.release_slot(&task.id);
        }
        result
    }

    /// 通知渠道解析:任务配置优先,回落全局渠道(若已配置)。
    pub fn resolve_notify_channel(&self, task: &Task) -> (String, String) {
        if task.notify_type != "none" {
            (task.notify_type.clone(), task.notify_url.clone())
        } else {
            match &self.global_notify {
                Some((nt, nu)) => (nt.clone(), nu.clone()),
                None => ("none".into(), String::new()),
            }
        }
    }

    /// 任务级在途计数 +1;已达上限返回 false。
    fn acquire_slot(&self, task_id: &str, max: i64) -> bool {
        let mut map = self.in_flight.lock().unwrap();
        let cur = map.entry(task_id.to_string()).or_insert(0);
        if *cur >= max as usize {
            false
        } else {
            *cur += 1;
            true
        }
    }

    fn release_slot(&self, task_id: &str) {
        let mut map = self.in_flight.lock().unwrap();
        if let Some(c) = map.get_mut(task_id) {
            *c -= 1;
            if *c == 0 {
                map.remove(task_id);
            }
        }
    }

    /// 落库一条 skipped 记录(并发跳过等场景),并推送事件刷新。
    pub(crate) async fn record_skipped(&self, db: &Db, task: &Task, reason: &str) -> TaskExecution {
        let exec = TaskExecution {
            id: uuid::Uuid::new_v4().to_string(),
            task_id: task.id.clone(),
            attempt: 0,
            status: "skipped".to_string(),
            output: Some(reason.to_string()),
            http_status: None,
            started_at: crate::db::now_iso(),
            finished_at: Some(crate::db::now_iso()),
        };
        if let Err(e) = db.create_execution(&exec).await {
            tracing::warn!(error = %e, task_id = %task.id, "record skipped execution failed");
        } else {
            self.events.bump();
        }
        tracing::info!(task_id = %task.id, reason = %reason, "execution skipped");
        exec
    }

    async fn run_with_retries(
        &self,
        db: &Db,
        task: &Task,
        depth: u32,
        trigger: Option<TriggerContext>,
        event: Option<EventContext>,
    ) -> TaskExecution {
        let mut attempt: i64 = 0;
        let (mut final_exec, mut duration_ms) = self
            .execute_attempt(db, task, attempt, trigger.clone(), event.clone())
            .await;
        // 失败且还有重试额度:指数退避后重试(30s 起步,封顶 8 分钟)。
        while final_exec.status == "failure" && attempt < task.max_retries {
            let backoff = Duration::from_secs((30u64 << attempt.min(4)).min(480));
            tracing::warn!(
                task_id = %task.id,
                attempt,
                backoff_secs = backoff.as_secs(),
                "attempt failed; retrying"
            );
            tokio::time::sleep(backoff).await;
            attempt += 1;
            let (exec, ms) = self
                .execute_attempt(db, task, attempt, trigger.clone(), event.clone())
                .await;
            final_exec = exec;
            duration_ms = ms;
        }

        // 通知只看最终结果:失败必推;成功且上一次为失败/中断时推送恢复。
        let (notify_type, notify_url) = self.resolve_notify_channel(task);
        if notify_type != "none" {
            if final_exec.status == "failure" {
                self.notifier.send_channel(
                    &notify_type,
                    &notify_url,
                    task,
                    "task_failure",
                    "任务失败",
                    &final_exec,
                    duration_ms,
                );
            } else if final_exec.status == "success"
                && let Ok(Some(prev)) = db.get_previous_execution(&task.id, &final_exec.id).await
                && (prev.status == "failure" || prev.status == "interrupted")
            {
                self.notifier.send_channel(
                    &notify_type,
                    &notify_url,
                    task,
                    "task_recovery",
                    "任务恢复",
                    &final_exec,
                    duration_ms,
                );
            }
        }

        // 依赖链:按策略触发下游任务(带深度防护)。
        let policy_matched = match task.trigger_on.as_str() {
            "failure" => final_exec.status == "failure",
            "always" => true,
            _ => final_exec.status == "success",
        };
        if policy_matched && !task.trigger_task_ids.is_empty() && depth < 10 {
            let ctx = TriggerContext {
                output: final_exec.output.clone().unwrap_or_default(),
                status: final_exec.status.clone(),
            };
            for next_id in &task.trigger_task_ids {
                if let Ok(Some(next)) = db.get_task(next_id).await {
                    if next.enabled {
                        tracing::info!(
                            from = %task.id,
                            to = %next.id,
                            depth,
                            "triggering downstream task"
                        );
                        let exec = self.clone();
                        let next_db = db.clone();
                        let next = next.clone();
                        let ctx = ctx.clone();
                        tokio::spawn(async move {
                            let _ = exec
                                .execute_and_record_depth(
                                    &next_db,
                                    &next,
                                    depth + 1,
                                    Some(ctx),
                                    None,
                                )
                                .await;
                        });
                    } else {
                        tracing::warn!(task_id = %next_id, "downstream task disabled; skipping trigger");
                    }
                } else {
                    tracing::warn!(task_id = %next_id, "downstream task missing; skipping trigger");
                }
            }
        }

        final_exec
    }

    /// 执行单次尝试并落库,返回(记录, 本次耗时)。
    async fn execute_attempt(
        &self,
        db: &Db,
        task: &Task,
        attempt: i64,
        trigger: Option<TriggerContext>,
        event: Option<EventContext>,
    ) -> (TaskExecution, u64) {
        let exec_id = uuid::Uuid::new_v4().to_string();
        let started_at = crate::db::now_iso();
        let start = std::time::Instant::now();

        let running = TaskExecution {
            id: exec_id.clone(),
            task_id: task.id.clone(),
            attempt,
            status: "running".to_string(),
            output: None,
            http_status: None,
            started_at: started_at.clone(),
            finished_at: None,
        };

        if let Err(e) = db.create_execution(&running).await {
            tracing::warn!(
                error = %e,
                task_id = %task.id,
                "create execution failed; skipping run"
            );
            let skipped = TaskExecution {
                status: "skipped".to_string(),
                finished_at: Some(crate::db::now_iso()),
                ..running
            };
            return (skipped, 0);
        }
        self.events.bump();

        tracing::info!(
            task_id = %task.id,
            exec_id = %exec_id,
            attempt,
            "execution started"
        );

        // 全局变量一次性加载(插值用);加载失败按无变量继续
        let vars: Vars = db
            .list_variables()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|v| (v.key, v.value))
            .collect();

        // 实时输出句柄:HTTP 任务虽无流式内容,统一创建保证 /live 端点行为一致
        let live = self.live.create(&exec_id, self.max_output_kb);
        let result = self
            .execute_with_trigger(task, trigger.as_ref(), event.as_ref(), &vars, &live)
            .await;
        live.finish();

        if let Err(e) = db
            .update_execution(&exec_id, &result.status, &result.output, result.http_status)
            .await
        {
            tracing::warn!(
                error = %e,
                task_id = %task.id,
                exec_id = %exec_id,
                "update execution failed"
            );
        }
        self.events.bump();
        self.live.remove(&exec_id);

        let duration_ms = start.elapsed().as_millis() as u64;

        tracing::info!(
            task_id = %task.id,
            exec_id = %exec_id,
            status = %result.status,
            duration_ms,
            "execution finished"
        );

        let duration_ms = start.elapsed().as_millis() as u64;
        let exec = db
            .get_execution(&exec_id)
            .await
            .ok()
            .flatten()
            .unwrap_or(running);
        (exec, duration_ms)
    }

    async fn execute_http(
        &self,
        method: &str,
        url: &str,
        headers: &Option<serde_json::Value>,
        body: Option<&str>,
        task_timeout: u64,
    ) -> ExecutionResult {
        let method = reqwest::Method::from_bytes(method.to_uppercase().as_bytes())
            .unwrap_or(reqwest::Method::GET);
        let mut req = self.client.request(method, url);

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

        let req = req.timeout(Duration::from_secs(task_timeout));

        match req.send().await {
            Ok(resp) => {
                let http_status = resp.status().as_u16() as i64;
                let is_success = resp.status().is_success();
                let mut out = crate::output::BoundedOutput::new(self.max_output_kb);
                match resp.bytes().await {
                    Ok(bytes) => out.push(&bytes),
                    Err(e) => out.push(e.to_string().as_bytes()),
                }
                ExecutionResult {
                    status: if is_success {
                        "success".to_string()
                    } else {
                        "failure".to_string()
                    },
                    output: out.snapshot(),
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

    /// Shell 执行:spawn + stdout/stderr 双路增量读取写入 live(实时视图),
    /// 结束后从 live 快照作为持久化输出(与实时视图同源,超限头尾截断)。
    async fn execute_shell(
        cmd: &str,
        timeout: Duration,
        live: &std::sync::Arc<crate::live::LiveOutput>,
    ) -> ExecutionResult {
        let mut child = match tokio::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                return ExecutionResult {
                    status: "failure".to_string(),
                    output: e.to_string(),
                    http_status: None,
                };
            }
        };
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");

        // 双路并发读取,按到达顺序合并(与实时视图一致)
        let r1 = tokio::spawn(read_stream(stdout, live.clone()));
        let r2 = tokio::spawn(read_stream(stderr, live.clone()));

        let mut exit_status: Option<std::process::ExitStatus> = None;
        let mut timed_out = false;
        match tokio::time::timeout(timeout, child.wait()).await {
            Ok(Ok(status)) => exit_status = Some(status),
            Ok(Err(e)) => {
                // wait 本身出错罕见;视作失败
                live.push(e.to_string().as_bytes());
            }
            Err(_) => {
                timed_out = true;
                let _ = child.kill().await;
            }
        }
        // 子进程已结束/被杀,管道随即 EOF;兜底超时防读取挂死
        let _ = tokio::time::timeout(Duration::from_secs(5), async {
            let _ = tokio::join!(r1, r2);
        })
        .await;

        // 流结束标记在快照之前:最终快照 done=true,输出内容不变
        live.finish();
        let success = !timed_out && exit_status.is_some_and(|s| s.success());
        let mut output = live.snapshot().output;
        if timed_out {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&format!(
                "shell command timed out after {}s",
                timeout.as_secs()
            ));
        }
        ExecutionResult {
            status: if success {
                "success".to_string()
            } else {
                "failure".to_string()
            },
            output,
            http_status: None,
        }
    }
}

/// 持续读取一个管道直到 EOF,把每个数据块推入实时输出。
async fn read_stream<R: tokio::io::AsyncRead + Unpin>(
    mut pipe: R,
    live: std::sync::Arc<crate::live::LiveOutput>,
) {
    use tokio::io::AsyncReadExt;
    let mut buf = [0u8; 8192];
    loop {
        match pipe.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => live.push(&buf[..n]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::LiveOutput;
    use std::sync::Arc;

    #[tokio::test]
    async fn shell_streams_output_incrementally() {
        let live = Arc::new(LiveOutput::new(64));
        let bg = live.clone();
        let watcher = tokio::spawn(async move {
            // 首个 chunk 到达(version>=1)即证明实时流在执行期间生效
            let snap = bg.wait_snapshot(0, Duration::from_secs(5)).await;
            snap.version >= 1
        });
        let result = Executor::execute_shell(
            "echo line1; sleep 0.3; echo line2",
            Duration::from_secs(10),
            &live,
        )
        .await;
        assert_eq!(result.status, "success");
        assert_eq!(result.output, "line1\nline2\n");
        assert!(
            watcher.await.unwrap(),
            "live stream should emit during execution"
        );
        assert!(live.snapshot().done);
    }

    #[tokio::test]
    async fn shell_timeout_kills_and_keeps_partial_output() {
        let live = Arc::new(LiveOutput::new(64));
        let result =
            Executor::execute_shell("echo start; sleep 30", Duration::from_secs(1), &live).await;
        assert_eq!(result.status, "failure");
        assert!(result.output.contains("start"));
        assert!(result.output.contains("timed out after 1s"));
        assert!(live.snapshot().done);
    }

    #[tokio::test]
    async fn shell_stderr_merged_and_marks_failure() {
        let live = Arc::new(LiveOutput::new(64));
        let result = Executor::execute_shell(
            "echo out; echo err >&2; exit 3",
            Duration::from_secs(10),
            &live,
        )
        .await;
        assert_eq!(result.status, "failure");
        assert!(result.output.contains("out"));
        assert!(result.output.contains("err"));
    }
}

#[cfg(test)]
mod concurrency_tests {
    use super::*;
    use crate::db::ScheduleConfig;

    fn temp_db() -> (Db, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("irs-exec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::new(dir.join("t.db").to_str().unwrap()).unwrap();
        (db, dir)
    }

    fn sleep_task(max_concurrent: i64) -> Task {
        let mut task = Task::new(
            "t".into(),
            TaskType::Shell {
                cmd: "sleep 0.4".into(),
            },
            ScheduleConfig::Once { delay_secs: 0 },
        );
        task.timeout_secs = 10;
        task.max_concurrent = max_concurrent;
        task
    }

    #[tokio::test]
    async fn overlapping_runs_skip_when_limit_reached() {
        let (db, dir) = temp_db();
        let ex = Executor::with_global_concurrency(None, 64, crate::live::Events::new(), 32);
        let task = sleep_task(1);
        // 先入库:task_executions 对 tasks 有外键约束
        db.create_task(&task).await.unwrap();
        let task2 = task.clone();
        let (a, b) = tokio::join!(
            ex.execute_and_record(&db, &task),
            ex.execute_and_record(&db, &task2),
        );
        let statuses = [a.status, b.status];
        assert!(
            statuses.contains(&"success".to_string()) && statuses.contains(&"skipped".to_string()),
            "expect one success + one skipped, got {statuses:?}"
        );
        // 槽位已释放:再次执行应正常成功
        let c = ex.execute_and_record(&db, &task).await;
        assert_eq!(c.status, "success");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn zero_max_concurrent_allows_parallel() {
        let (db, dir) = temp_db();
        let ex = Executor::with_global_concurrency(None, 64, crate::live::Events::new(), 32);
        let task = sleep_task(0);
        db.create_task(&task).await.unwrap();
        let task2 = task.clone();
        let (a, b) = tokio::join!(
            ex.execute_and_record(&db, &task),
            ex.execute_and_record(&db, &task2),
        );
        assert_eq!(a.status, "success");
        assert_eq!(b.status, "success");
        std::fs::remove_dir_all(&dir).ok();
    }
}
