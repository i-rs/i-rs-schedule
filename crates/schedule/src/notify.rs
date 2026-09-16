use crate::db::{Task, TaskExecution};
use std::time::Duration;

/// 失败/恢复通知推送。fire-and-forget:发送失败仅记 warn,不影响执行主流程。
#[derive(Clone)]
pub struct Notifier {
    client: reqwest::Client,
}

impl Notifier {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build notifier client");
        Self { client }
    }

    /// 按任务的通知配置推送;未配置(notify_type=none)时为 no-op。
    pub fn send(
        &self,
        task: &Task,
        event: &str,
        title: &str,
        exec: &TaskExecution,
        duration_ms: u64,
    ) {
        if task.notify_type == "none" || task.notify_url.trim().is_empty() {
            return;
        }

        let text = format!(
            "[i-rs-schedule] {title}\n名称: {}\n任务ID: {}\n状态: {}\n耗时: {}ms\n输出: {}",
            task.name,
            task.id,
            exec.status,
            duration_ms,
            truncate(exec.output.as_deref().unwrap_or(""), 2000),
        );

        let url = task.notify_url.clone();
        let body = match task.notify_type.as_str() {
            "feishu" => serde_json::json!({
                "msg_type": "text",
                "content": {"text": text},
            }),
            "dingtalk" => serde_json::json!({
                "msgtype": "text",
                "text": {"content": text},
            }),
            _ => serde_json::json!({
                "event": event,
                "task_id": task.id,
                "task_name": task.name,
                "status": exec.status,
                "duration_ms": duration_ms,
                "output": truncate(exec.output.as_deref().unwrap_or(""), 2000),
                "exec_id": exec.id,
                "started_at": exec.started_at,
                "finished_at": exec.finished_at,
            }),
        };

        let client = self.client.clone();
        let event = event.to_string();
        tokio::spawn(async move {
            match client.post(&url).json(&body).send().await {
                Ok(resp) if resp.status().is_success() => {
                    tracing::info!(url = %url, event = %event, "notification sent");
                }
                Ok(resp) => {
                    tracing::warn!(
                        url = %url,
                        status = %resp.status(),
                        "notification endpoint returned error"
                    );
                }
                Err(e) => {
                    tracing::warn!(url = %url, error = %e, "failed to send notification");
                }
            }
        });
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max_chars).collect();
        format!("{cut}…")
    }
}
