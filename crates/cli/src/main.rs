use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;

#[derive(Parser)]
#[command(
    name = "i-rs-cli",
    version,
    about = "CLI for i-rs-schedule task management"
)]
struct Cli {
    #[arg(long, default_value = "http://localhost:3000")]
    server: String,

    /// API token(优先于 SCHEDULE_TOKEN 环境变量)
    #[arg(long)]
    token: Option<String>,

    #[command(subcommand)]
    command: Command,
}

// AddArgs 携带全部创建参数,与查询类子命令的变体大小差异是结构性的,非性能路径。
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum Command {
    #[command(subcommand)]
    Task(TaskCmd),

    #[command(subcommand)]
    Exec(ExecCmd),

    #[command(subcommand)]
    Cron(CronCmd),

    #[command(subcommand)]
    Auth(AuthCmd),

    #[command(subcommand)]
    Token(TokenCmd),
}

#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum TaskCmd {
    Add(AddArgs),
    Update(UpdateArgs),
    List(ListArgs),
    Show(ShowArgs),
    Rm(RmArgs),
    Enable(IdArgs),
    Disable(IdArgs),
    Run(IdArgs),
    Stats(IdArgs),
    NotifyTest(IdArgs),
    Export,
    Import(ImportArgs),
}

#[derive(Subcommand)]
enum ExecCmd {
    List(ExecListArgs),
    Show(IdArgs),
}

#[derive(Args)]
struct AddArgs {
    #[arg(long)]
    name: String,

    #[arg(long, value_parser = ["http", "shell"])]
    r#type: String,

    #[arg(long)]
    cron: Option<String>,

    #[arg(long)]
    delay_secs: Option<u64>,

    #[arg(long)]
    url: Option<String>,

    #[arg(long, default_value = "GET")]
    method: Option<String>,

    #[arg(long)]
    headers: Option<String>,

    #[arg(long)]
    body: Option<String>,

    #[arg(long)]
    cmd: Option<String>,

    #[arg(long, default_value_t = 30)]
    timeout_secs: u64,

    #[arg(long, default_value_t = 0)]
    max_retries: i64,

    #[arg(long, default_value = "")]
    trigger_on_success: Vec<String>,

    #[arg(long, default_value = "success")]
    trigger_on: String,

    #[arg(long, default_value = "UTC")]
    timezone: String,

    #[arg(long, default_value = "none")]
    notify_type: String,

    #[arg(long, default_value = "")]
    notify_url: String,

    #[arg(long)]
    enabled: Option<bool>,
}

#[derive(Args)]
struct ListArgs {
    #[arg(long)]
    enabled: Option<bool>,
}

#[derive(Args)]
struct UpdateArgs {
    #[arg(long)]
    id: String,

    #[arg(long)]
    name: Option<String>,

    #[arg(long, value_parser = ["http", "shell"])]
    task_type: Option<String>,

    #[arg(long)]
    cron: Option<String>,

    #[arg(long)]
    delay_secs: Option<u64>,

    #[arg(long)]
    url: Option<String>,

    #[arg(long)]
    method: Option<String>,

    #[arg(long)]
    headers: Option<String>,

    #[arg(long)]
    body: Option<String>,

    #[arg(long)]
    cmd: Option<String>,

    #[arg(long)]
    timezone: Option<String>,

    #[arg(long)]
    timeout_secs: Option<u64>,

    #[arg(long)]
    max_retries: Option<i64>,

    #[arg(long, value_delimiter = ',')]
    trigger_on_success: Vec<String>,

    #[arg(long, default_value = "success")]
    trigger_on: String,

    #[arg(long)]
    notify_type: Option<String>,

    #[arg(long)]
    notify_url: Option<String>,

    #[arg(long)]
    enabled: Option<bool>,
}

#[derive(Args)]
struct ImportArgs {
    #[arg(long)]
    file: String,
}

#[derive(Args)]
struct CronPreviewArgs {
    #[arg(long)]
    expr: String,

    #[arg(long, default_value = "UTC")]
    timezone: String,
}

#[derive(Args)]
struct LoginArgs {
    #[arg(long)]
    username: String,

    #[arg(long)]
    password: String,
}

#[derive(Args)]
struct AuditArgs {
    #[arg(long, default_value_t = 50)]
    limit: i64,
}

#[derive(Args)]
struct CreateTokenArgs {
    #[arg(long)]
    name: String,
}

#[derive(Args)]
struct ShowArgs {
    #[arg(long)]
    id: String,
}

#[derive(Args)]
struct RmArgs {
    #[arg(long)]
    id: String,
}

#[derive(Args)]
struct IdArgs {
    #[arg(long)]
    id: String,
}

#[derive(Args)]
struct ExecListArgs {
    #[arg(long)]
    task_id: Option<String>,

    #[arg(long, default_value = "50")]
    limit: u32,
}

#[derive(Serialize)]
struct CreateTaskBody {
    name: String,
    task_type: String,
    schedule_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cron_expr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    delay_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_headers: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shell_cmd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timezone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_retries: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trigger_task_ids: Option<Vec<String>>,
    trigger_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notify_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notify_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
}

#[derive(Subcommand)]
enum CronCmd {
    Preview(CronPreviewArgs),
}

#[derive(Subcommand)]
enum AuthCmd {
    Login(LoginArgs),
    Audit(AuditArgs),
}

#[derive(Subcommand)]
enum TokenCmd {
    List,
    Create(CreateTokenArgs),
    Revoke(IdArgs),
}

async fn print_response(resp: reqwest::Response) -> Result<()> {
    let val: serde_json::Value = resp.json().await?;
    // 服务端错误信封(code != 0)输出到 stderr 并以非零码退出,便于脚本感知失败。
    let code = val.get("code").and_then(|c| c.as_u64()).unwrap_or(0);
    if code != 0 {
        let msg = val
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        eprintln!("error ({code}): {msg}");
        std::process::exit(1);
    }
    println!("{}", serde_json::to_string_pretty(&val)?);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let base = std::env::var("SCHEDULE_SERVER").unwrap_or(cli.server);
    let token = cli.token.or_else(|| std::env::var("SCHEDULE_TOKEN").ok());

    let mut builder = reqwest::Client::builder();
    if let Some(token) = token.as_deref().filter(|t| !t.is_empty()) {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "authorization",
            reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                .context("invalid token characters")?,
        );
        builder = builder.default_headers(headers);
    }
    let client = builder.build()?;

    match cli.command {
        Command::Task(cmd) => match cmd {
            TaskCmd::Add(args) => {
                let schedule_type = if args.delay_secs.is_some() {
                    "once"
                } else {
                    "cron"
                };
                let headers_json: Option<serde_json::Value> = args
                    .headers
                    .as_ref()
                    .and_then(|h| serde_json::from_str(h).ok());

                let body = CreateTaskBody {
                    name: args.name,
                    task_type: args.r#type,
                    schedule_type: schedule_type.to_string(),
                    cron_expr: args.cron,
                    delay_secs: args.delay_secs,
                    http_method: args.method,
                    http_url: args.url,
                    http_headers: headers_json,
                    http_body: args.body,
                    shell_cmd: args.cmd,
                    timezone: Some(args.timezone),
                    timeout_secs: Some(args.timeout_secs),
                    max_retries: Some(args.max_retries),
                    trigger_task_ids: Some(
                        args.trigger_on_success
                            .into_iter()
                            .filter(|s| !s.is_empty())
                            .collect(),
                    ),
                    trigger_on: Some(args.trigger_on),
                    enabled: args.enabled,
                    notify_type: Some(args.notify_type),
                    notify_url: Some(args.notify_url),
                };

                let resp = client
                    .post(format!("{base}/api/tasks"))
                    .json(&body)
                    .send()
                    .await
                    .context("failed to create task")?;

                print_response(resp).await?;
            }
            TaskCmd::Update(args) => {
                // 取当前任务 → 合并 flags → PUT(服务端对缺失字段会用默认值,因此必须全量提交)
                let resp = client
                    .get(format!("{base}/api/tasks/{}", args.id))
                    .send()
                    .await
                    .context("fetch current task")?;
                let envv: serde_json::Value = resp.json().await?;
                let cur = envv
                    .get("data")
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("task not found"))?;

                let mut payload = serde_json::json!({
                    "name": cur["name"],
                    "task_type": cur["task_type"]["type"],
                    "schedule_type": cur["schedule"]["type"],
                    "timezone": cur["timezone"],
                    "timeout_secs": cur["timeout_secs"],
                    "max_retries": cur["max_retries"],
                    "enabled": cur["enabled"],
                    "notify_type": cur["notify_type"],
                    "notify_url": cur["notify_url"],
                    "trigger_on": cur["trigger_on"],
                });
                if cur["schedule"]["type"] == "cron" {
                    payload["cron_expr"] = cur["schedule"]["expr"].clone();
                } else {
                    payload["delay_secs"] = cur["schedule"]["delay_secs"].clone();
                }
                if cur["task_type"]["type"] == "http" {
                    payload["http_method"] = cur["task_type"]["method"].clone();
                    payload["http_url"] = cur["task_type"]["url"].clone();
                    if cur["task_type"]["body"].is_string() {
                        payload["http_body"] = cur["task_type"]["body"].clone();
                    }
                    if cur["task_type"]["headers"].is_object()
                        && !cur["task_type"]["headers"].as_object().unwrap().is_empty()
                    {
                        payload["http_headers"] = cur["task_type"]["headers"].clone();
                    }
                } else {
                    payload["shell_cmd"] = cur["task_type"]["cmd"].clone();
                }
                if cur["trigger_task_ids"].is_array()
                    && !cur["trigger_task_ids"].as_array().unwrap().is_empty()
                {
                    payload["trigger_task_ids"] = cur["trigger_task_ids"].clone();
                }

                if let Some(v) = args.name {
                    payload["name"] = serde_json::Value::String(v);
                }
                if let Some(v) = args.task_type {
                    payload["task_type"] = serde_json::Value::String(v);
                }
                if let Some(v) = args.cron {
                    payload["cron_expr"] = serde_json::Value::String(v);
                    payload["schedule_type"] = serde_json::Value::String("cron".into());
                }
                if let Some(v) = args.delay_secs {
                    payload["delay_secs"] = serde_json::Value::from(v);
                    payload["schedule_type"] = serde_json::Value::String("once".into());
                }
                if let Some(v) = args.url {
                    payload["http_url"] = serde_json::Value::String(v);
                }
                if let Some(v) = args.method {
                    payload["http_method"] = serde_json::Value::String(v);
                }
                if let Some(v) = args.headers {
                    payload["http_headers"] = serde_json::from_str(&v)?;
                }
                if let Some(v) = args.body {
                    payload["http_body"] = serde_json::Value::String(v);
                }
                if let Some(v) = args.cmd {
                    payload["shell_cmd"] = serde_json::Value::String(v);
                }
                if let Some(v) = args.timezone {
                    payload["timezone"] = serde_json::Value::String(v);
                }
                if let Some(v) = args.timeout_secs {
                    payload["timeout_secs"] = serde_json::Value::from(v);
                }
                if let Some(v) = args.max_retries {
                    payload["max_retries"] = serde_json::Value::from(v);
                }
                if !args.trigger_on_success.is_empty() {
                    payload["trigger_task_ids"] = serde_json::json!(args.trigger_on_success);
                }
                if args.trigger_on != "success" {
                    payload["trigger_on"] = serde_json::Value::String(args.trigger_on.clone());
                }
                if let Some(v) = args.notify_type {
                    payload["notify_type"] = serde_json::Value::String(v);
                }
                if let Some(v) = args.notify_url {
                    payload["notify_url"] = serde_json::Value::String(v);
                }
                if let Some(v) = args.enabled {
                    payload["enabled"] = serde_json::Value::Bool(v);
                }

                let resp = client
                    .put(format!("{base}/api/tasks/{}", args.id))
                    .json(&payload)
                    .send()
                    .await?;
                print_response(resp).await?;
            }
            TaskCmd::List(args) => {
                let url = format!("{base}/api/tasks");
                let resp = client.get(&url).send().await?;
                // 服务端返回 {code, message, data} 信封,取 data 数组做本地过滤。
                let val: serde_json::Value = resp.json().await?;
                let code = val.get("code").and_then(|c| c.as_u64()).unwrap_or(0);
                if code != 0 {
                    let msg = val
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown error");
                    eprintln!("error ({code}): {msg}");
                    std::process::exit(1);
                }
                let tasks = val
                    .get("data")
                    .and_then(|d| d.as_array())
                    .cloned()
                    .unwrap_or_default();
                let filtered: Vec<&serde_json::Value> = match args.enabled {
                    Some(enabled) => tasks
                        .iter()
                        .filter(|t| t["enabled"].as_bool() == Some(enabled))
                        .collect(),
                    None => tasks.iter().collect(),
                };
                println!("{}", serde_json::to_string_pretty(&filtered)?);
            }
            TaskCmd::Show(args) => {
                let resp = client
                    .get(format!("{base}/api/tasks/{}", args.id))
                    .send()
                    .await?;
                print_response(resp).await?;
            }
            TaskCmd::Rm(args) => {
                let resp = client
                    .delete(format!("{base}/api/tasks/{}", args.id))
                    .send()
                    .await?;
                print_response(resp).await?;
            }
            TaskCmd::Enable(args) => {
                let resp = client
                    .post(format!("{base}/api/tasks/{}/enable", args.id))
                    .send()
                    .await?;
                print_response(resp).await?;
            }
            TaskCmd::Disable(args) => {
                let resp = client
                    .post(format!("{base}/api/tasks/{}/disable", args.id))
                    .send()
                    .await?;
                print_response(resp).await?;
            }
            TaskCmd::Run(args) => {
                let resp = client
                    .post(format!("{base}/api/tasks/{}/run", args.id))
                    .send()
                    .await?;
                print_response(resp).await?;
            }
            TaskCmd::Stats(args) => {
                let resp = client
                    .get(format!("{base}/api/tasks/{}/stats", args.id))
                    .send()
                    .await?;
                print_response(resp).await?;
            }
            TaskCmd::NotifyTest(args) => {
                let resp = client
                    .post(format!("{base}/api/tasks/{}/notify-test", args.id))
                    .send()
                    .await?;
                print_response(resp).await?;
            }
            TaskCmd::Export => {
                let resp = client
                    .get(format!("{base}/api/export/tasks"))
                    .send()
                    .await?;
                let envv: serde_json::Value = resp.json().await?;
                println!("{}", serde_json::to_string_pretty(&envv["data"])?);
            }
            TaskCmd::Import(args) => {
                let text = std::fs::read_to_string(&args.file).context("read import file")?;
                let resp = client
                    .post(format!("{base}/api/import/tasks"))
                    .header("Content-Type", "application/json")
                    .body(text)
                    .send()
                    .await?;
                print_response(resp).await?;
            }
        },
        Command::Cron(cmd) => match cmd {
            CronCmd::Preview(args) => {
                let resp = client
                    .post(format!("{base}/api/cron/preview"))
                    .json(&serde_json::json!({
                        "expr": args.expr,
                        "timezone": args.timezone,
                    }))
                    .send()
                    .await?;
                print_response(resp).await?;
            }
        },
        Command::Auth(cmd) => match cmd {
            AuthCmd::Login(args) => {
                let resp = client
                    .post(format!("{base}/api/auth/login"))
                    .json(&serde_json::json!({
                        "username": args.username,
                        "password": args.password,
                    }))
                    .send()
                    .await?;
                print_response(resp).await?;
                eprintln!(
                    "hint: save it via SCHEDULE_TOKEN env or --token; it is valid for 30 days"
                );
            }
            AuthCmd::Audit(args) => {
                let resp = client
                    .get(format!("{base}/api/audit?limit={}", args.limit))
                    .send()
                    .await?;
                print_response(resp).await?;
            }
        },
        Command::Token(cmd) => match cmd {
            TokenCmd::List => {
                let resp = client.get(format!("{base}/api/auth/tokens")).send().await?;
                print_response(resp).await?;
            }
            TokenCmd::Create(args) => {
                let resp = client
                    .post(format!("{base}/api/auth/tokens"))
                    .json(&serde_json::json!({ "name": args.name }))
                    .send()
                    .await?;
                print_response(resp).await?;
            }
            TokenCmd::Revoke(args) => {
                let resp = client
                    .delete(format!("{base}/api/auth/tokens/{}", args.id))
                    .send()
                    .await?;
                print_response(resp).await?;
            }
        },
        Command::Exec(cmd) => match cmd {
            ExecCmd::List(args) => {
                let mut url = format!("{base}/api/executions?limit={}", args.limit);
                if let Some(ref tid) = args.task_id {
                    url.push_str(&format!("&task_id={tid}"));
                }
                let resp = client.get(&url).send().await?;
                print_response(resp).await?;
            }
            ExecCmd::Show(args) => {
                let resp = client
                    .get(format!("{base}/api/executions/{}", args.id))
                    .send()
                    .await?;
                print_response(resp).await?;
            }
        },
    }

    Ok(())
}
