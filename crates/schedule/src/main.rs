mod api;
mod db;
mod executor;
mod schedule;
mod scheduler;

use db::Db;
use executor::Executor;
use scheduler::Scheduler;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let db_path = std::env::var("SCHEDULE_DB").unwrap_or_else(|_| "./data/schedule.db".to_string());
    let port = std::env::var("SCHEDULE_PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("127.0.0.1:{port}");

    let db = Db::new(&db_path)?;

    let tasks = db.list_enabled_tasks().await?;
    tracing::info!("loaded {} enabled tasks from db", tasks.len());

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();

    let mut scheduler = Scheduler::new();
    scheduler.load_tasks(tasks);

    let executor = Arc::new(Executor::new());
    let scheduler_db = db.clone();
    let api_executor = executor.clone();

    tokio::spawn(async move {
        scheduler.run(cmd_rx, executor, scheduler_db).await;
    });

    let router = api::build_router(db, cmd_tx, api_executor);

    tracing::info!("starting server on http://{addr}");
    desirable::new(&addr).run(router).await?;

    Ok(())
}
