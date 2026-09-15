use crate::db::{Db, Task, TaskExecution, TaskType};
use crate::notify::Notifier;
use std::time::Duration;

pub struct ExecutionResult {
    pub status: String,
    pub output: String,
    pub http_status: Option<i64>,
}

pub struct Executor {
    client: reqwest::Client,
    notifier: Notifier,
}

impl Executor {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            notifier: Notifier::new(),
        }
    }

    pub async fn execute(&self, task: &Task) -> ExecutionResult {
        match &task.task_type {
            TaskType::Http {
                method,
                url,
                headers,
                body,
            } => {
                self.execute_http(method, url, headers, body.as_deref(), task.timeout_secs)
                    .await
            }
            TaskType::Shell { cmd } => {
                Self::execute_shell(cmd, Duration::from_secs(task.timeout_secs)).await
            }
        }
    }

    /// 执行任务并记录到 DB。封装 create_execution → execute → update_execution,
    /// scheduler 与 `/run` 端点共用,保证行为一致。
    ///
    /// create_execution 失败时返回一个内存构造的 `status="skipped"` 记录(不入库),
    /// 并打 warn 日志;execute 与 update_execution 的失败均告警但不影响返回。
    pub async fn execute_and_record(&self, db: &Db, task: &Task) -> TaskExecution {
        let mut attempt: i64 = 0;
        let (mut final_exec, mut duration_ms) = self.execute_attempt(db, task, attempt).await;
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
            let (exec, ms) = self.execute_attempt(db, task, attempt).await;
            final_exec = exec;
            duration_ms = ms;
        }

        // 通知只看最终结果:失败必推;成功且上一次为失败/中断时推送恢复。
        if task.notify_type != "none" {
            if final_exec.status == "failure" {
                self.notifier
                    .send(task, "task_failure", "任务失败", &final_exec, duration_ms);
            } else if final_exec.status == "success"
                && let Ok(Some(prev)) = db.get_previous_execution(&task.id, &final_exec.id).await
                && (prev.status == "failure" || prev.status == "interrupted")
            {
                self.notifier
                    .send(task, "task_recovery", "任务恢复", &final_exec, duration_ms);
            }
        }

        final_exec
    }

    /// 执行单次尝试并落库,返回(记录, 本次耗时)。
    async fn execute_attempt(&self, db: &Db, task: &Task, attempt: i64) -> (TaskExecution, u64) {
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

        tracing::info!(
            task_id = %task.id,
            exec_id = %exec_id,
            attempt,
            "execution started"
        );

        let result = self.execute(task).await;

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

    async fn execute_shell(cmd: &str, timeout: Duration) -> ExecutionResult {
        // 与 HTTP 一致按任务配置超时,防止无限运行占住执行槽。
        match tokio::time::timeout(
            timeout,
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .output(),
        )
        .await
        {
            Ok(Ok(out)) => {
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
            Ok(Err(e)) => ExecutionResult {
                status: "failure".to_string(),
                output: e.to_string(),
                http_status: None,
            },
            Err(_) => ExecutionResult {
                status: "failure".to_string(),
                output: format!("shell command timed out after {}s", timeout.as_secs()),
                http_status: None,
            },
        }
    }
}
