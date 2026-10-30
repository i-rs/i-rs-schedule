use crate::db::{Task, TaskExecution};
use std::time::Duration;

/// 失败/恢复通知推送。fire-and-forget:发送失败仅记 warn,不影响执行主流程。
#[derive(Clone)]
pub struct Notifier {
    client: reqwest::Client,
    /// 通用 webhook 渠道的 HMAC 签名密钥(空 = 不签名)
    webhook_secret: Option<String>,
}

impl Notifier {
    pub fn new(webhook_secret: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build notifier client");
        Self {
            client,
            webhook_secret,
        }
    }

    /// HMAC-SHA256 签名(通用 webhook 渠道接收方校验用)。
    fn sign(&self, body: &str) -> Option<String> {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        let secret = self.webhook_secret.as_deref()?.trim();
        if secret.is_empty() {
            return None;
        }
        let mut mac = <Hmac<Sha256>>::new_from_slice(secret.as_bytes()).ok()?;
        mac.update(body.as_bytes());
        let out = mac.finalize().into_bytes();
        Some(format!("sha256={out:x}"))
    }

    /// 显式指定渠道的推送(全局回落时渠道与任务字段无关)。
    #[allow(clippy::too_many_arguments)]
    pub fn send_channel(
        &self,
        notify_type: &str,
        notify_url: &str,
        task: &Task,
        event: &str,
        title: &str,
        exec: &TaskExecution,
        duration_ms: u64,
    ) {
        if notify_type == "none" || notify_url.trim().is_empty() {
            return;
        }
        let notify_type = notify_type.to_string();
        let notify_url = notify_url.to_string();
        let task = task.clone();
        let exec = exec.clone();
        let event = event.to_string();
        let title = title.to_string();
        let this = self.clone();
        tokio::spawn(async move {
            if let Err(detail) = this
                .send_sync_channel(
                    &notify_type,
                    &notify_url,
                    &task,
                    &event,
                    &title,
                    &exec,
                    duration_ms,
                )
                .await
            {
                tracing::warn!(event = %event, detail = %detail, "notification delivery failed");
            }
        });
    }

    /// 同步发送一条通知并返回投递结果(供"发送测试"等需要即时反馈的场景)。
    #[allow(clippy::too_many_arguments)]
    pub async fn send_sync_channel(
        &self,
        notify_type: &str,
        notify_url: &str,
        task: &Task,
        event: &str,
        title: &str,
        exec: &TaskExecution,
        duration_ms: u64,
    ) -> Result<(), String> {
        if notify_type == "none" || notify_url.trim().is_empty() {
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

        let url = notify_url.to_string();
        let body = match notify_type {
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

        // 通用 webhook 且配置了密钥:序列化后整体签名,附 X-i-rs-Signature 头
        let mut req = if notify_type == "webhook" {
            let body_str = serde_json::to_string(&body).unwrap_or_default();
            let mut r = self.client.post(&url).body(body_str.clone());
            if let Some(sig) = self.sign(&body_str) {
                r = r.header("X-i-rs-Signature", sig);
            }
            r
        } else {
            self.client.post(&url).json(&body)
        };
        req = req.header("Content-Type", "application/json");

        match req.send().await {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ScheduleConfig, TaskType};
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    fn sample_task() -> Task {
        Task::new(
            "t".into(),
            TaskType::Shell { cmd: "echo".into() },
            ScheduleConfig::Once { delay_secs: 0 },
        )
    }

    fn sample_exec() -> TaskExecution {
        TaskExecution {
            id: "e1".into(),
            task_id: "t1".into(),
            attempt: 0,
            status: "failure".into(),
            output: Some("boom".into()),
            http_status: None,
            started_at: crate::db::now_iso(),
            finished_at: Some(crate::db::now_iso()),
        }
    }

    /// 本地单连接 HTTP 接收端:读完请求(累积至 content-length 或读超时),
    /// 回 200 后返回 (headers 文本, body)。
    async fn recv_one(addr: std::net::SocketAddr) -> (String, String) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind(addr).unwrap();
        listener.set_nonblocking(true).unwrap();
        // 轮询等待连接(accept 后显式恢复阻塞 + 读超时)
        let stream = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        };
        stream.set_nonblocking(false).ok();
        stream
            .set_read_timeout(Some(Duration::from_millis(300)))
            .ok();

        let mut raw: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match (&stream).read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    raw.extend_from_slice(&chunk[..n]);
                    // 已满足 content-length 则提前结束
                    if let Ok(text) = std::str::from_utf8(&raw) {
                        if let Some((headers, body)) = text.split_once("\r\n\r\n") {
                            let len = headers
                                .lines()
                                .find_map(|l| {
                                    let (k, v) = l.split_once(':')?;
                                    k.eq_ignore_ascii_case("content-length")
                                        .then(|| v.trim().parse::<usize>().ok())?
                                })
                                .unwrap_or(0);
                            if body.len() >= len && len > 0 {
                                break;
                            }
                        }
                    }
                }
                Err(_) => break, // 读超时:请求已完整
            }
        }
        let _ = (&stream).write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n");
        let _ = (&stream).flush();
        drop(stream);
        let raw = String::from_utf8_lossy(&raw).to_string();
        let (headers, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
        (headers.to_string(), body.to_string())
    }

    #[tokio::test]
    async fn webhook_signed_with_hmac_when_secret_set() {
        let secret = "topsecret";
        let notifier = Notifier::new(Some(secret.into()));
        let addr: std::net::SocketAddr = "127.0.0.1:19871".parse().unwrap();
        let receiver = tokio::spawn(recv_one(addr));

        // 稍等接收端就绪
        tokio::time::sleep(Duration::from_millis(100)).await;
        notifier
            .send_sync_channel(
                "webhook",
                "http://127.0.0.1:19871/hook",
                &sample_task(),
                "task_failure",
                "任务失败",
                &sample_exec(),
                12,
            )
            .await
            .unwrap();
        let (headers, body) = receiver.await.unwrap();

        let sig_line = headers
            .lines()
            .find(|l| l.to_lowercase().starts_with("x-i-rs-signature"))
            .expect("signature header present");
        let provided = sig_line.split_once(':').unwrap().1.trim();

        let mut mac = <Hmac<Sha256>>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body.as_bytes());
        let expected = format!("sha256={:x}", mac.finalize().into_bytes());
        assert_eq!(provided, expected, "HMAC must match body exactly");
    }

    #[tokio::test]
    async fn webhook_unsigned_without_secret() {
        let notifier = Notifier::new(None);
        let addr: std::net::SocketAddr = "127.0.0.1:19872".parse().unwrap();
        let receiver = tokio::spawn(recv_one(addr));
        tokio::time::sleep(Duration::from_millis(100)).await;
        notifier
            .send_sync_channel(
                "webhook",
                "http://127.0.0.1:19872/hook",
                &sample_task(),
                "task_failure",
                "任务失败",
                &sample_exec(),
                12,
            )
            .await
            .unwrap();
        let (headers, _) = receiver.await.unwrap();
        assert!(!headers.to_lowercase().contains("x-i-rs-signature"));
    }
}
