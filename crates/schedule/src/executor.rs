use crate::db::{Task, TaskType};
use std::time::Duration;

pub struct ExecutionResult {
    pub status: String,
    pub output: String,
    pub http_status: Option<i64>,
}

pub struct Executor {
    client: reqwest::Client,
}

impl Executor {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client");
        Self { client }
    }

    pub async fn execute(&self, task: &Task) -> ExecutionResult {
        match &task.task_type {
            TaskType::Http {
                method,
                url,
                headers,
                body,
            } => {
                self.execute_http(method, url, headers, body.as_deref())
                    .await
            }
            TaskType::Shell { cmd } => Self::execute_shell(cmd).await,
        }
    }

    async fn execute_http(
        &self,
        method: &str,
        url: &str,
        headers: &Option<serde_json::Value>,
        body: Option<&str>,
    ) -> ExecutionResult {
        let mut req = match method.to_uppercase().as_str() {
            "GET" => self.client.get(url),
            "POST" => self.client.post(url),
            "PUT" => self.client.put(url),
            "DELETE" => self.client.delete(url),
            _ => self.client.get(url),
        };

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

    async fn execute_shell(cmd: &str) -> ExecutionResult {
        match tokio::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .output()
            .await
        {
            Ok(out) => {
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
            Err(e) => ExecutionResult {
                status: "failure".to_string(),
                output: e.to_string(),
                http_status: None,
            },
        }
    }
}
