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

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(subcommand)]
    Task(TaskCmd),

    #[command(subcommand)]
    Exec(ExecCmd),
}

#[derive(Subcommand)]
enum TaskCmd {
    Add(AddArgs),
    List(ListArgs),
    Show(ShowArgs),
    Rm(RmArgs),
    Enable(IdArgs),
    Disable(IdArgs),
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
}

#[derive(Args)]
struct ListArgs {
    #[arg(long)]
    enabled: Option<bool>,
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
    let client = reqwest::Client::new();

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
                };

                let resp = client
                    .post(format!("{base}/api/tasks"))
                    .json(&body)
                    .send()
                    .await
                    .context("failed to create task")?;

                print_response(resp).await?;
            }
            TaskCmd::List(args) => {
                let url = format!("{base}/api/tasks");
                let resp = client.get(&url).send().await?;
                let tasks: Vec<serde_json::Value> = resp.json().await?;
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
