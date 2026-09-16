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
        let task = task.clone();
        let exec = exec.clone();
        let event = event.to_string();
        let title = title.to_string();
        let this = self.clone();
        tokio::spawn(async move {
            if let Err(detail) = this.send_sync(&task, &event, &title, &exec, duration_ms).await {
                tracing::warn!(event = %event, detail = %detail, "notification delivery failed");
            }
        });
    }

    /// 同步发送一条通知并返回投递结果(供"发送测试"等需要即时反馈的场景)。
    pub async fn send_sync(
        &self,
        task: &Task,
        event: &str,
        title: &str,
        exec: &TaskExecution,
        duration_ms: u64,
    ) -> Result<(), String> {
        if task.notify_type == "none" || task.notify_url.trim().is_empty() {
            return Err("notifications not configured".into());
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

        match self.client.post(&url).json(&body).send().await {
            Ok(resp) if resp.status().is_success() => Ok(()),
            Ok(resp) => Err(format!("endpoint returned {}", resp.status())),
            Err(e) => Err(e.to_string()),
        }
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
