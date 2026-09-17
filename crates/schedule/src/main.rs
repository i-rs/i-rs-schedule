mod api;
mod config;
mod db;
mod executor;
mod live;
mod notify;
mod output;
mod schedule;
mod scheduler;

use db::Db;
use executor::Executor;
use scheduler::{ControlCmd, Scheduler};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config = config::Config::load();
    let addr = format!("127.0.0.1:{}", config.port);

    let db = Db::new(&config.db_path)?;

    // 执行记录保留策略:RETENTION_DAYS 天(默认 30),0 = 永久保留。
    let retention_days = config.retention_days;
    if retention_days > 0 {
        run_retention(&db, retention_days).await;
        let retention_db = db.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 3600));
            loop {
                interval.tick().await;
                run_retention(&retention_db, retention_days).await;
            }
        });
    }

    let tasks = db.list_enabled_tasks().await?;
    tracing::info!("loaded {} enabled tasks from db", tasks.len());

    // 维护模式:重启后从 settings 恢复
    let maintenance_enabled = matches!(
        db.get_setting("maintenance").await.unwrap().as_deref(),
        Some("1")
    );
    if maintenance_enabled {
        tracing::info!("maintenance mode restored: enabled");
    }
    let maintenance = scheduler::Maintenance::new(maintenance_enabled);

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();

    let mut scheduler = Scheduler::new(maintenance_enabled);
    scheduler.load_tasks(tasks);

    // 管理员账号(可选):配置了即启用登录
    if let (Some(user), Some(pass)) = (config.admin_user.clone(), config.admin_password.clone()) {
        db.seed_admin(&user, &pass).await?;
        tracing::info!("admin user seeded: {user}");
    }
    let has_admin = config.admin_user.is_some() && config.admin_password.is_some();

    // 全局通知渠道(任务未配置时的回落)
    let global_notify = match (config.notify_type.clone(), config.notify_url.clone()) {
        (Some(t), Some(u)) if !u.is_empty() => Some((t, u)),
        _ => None,
    };
    let executor = Arc::new(Executor::with_global_concurrency(
        global_notify,
        config.max_output_kb,
        live::Events::new(),
        config.global_max_concurrent,
    ));
    let scheduler_db = db.clone();
    let api_executor = executor.clone();

    // scheduler 在后台跑,处理到期任务与控制命令。
    let scheduler_handle = tokio::spawn(async move {
        scheduler.run(cmd_rx, executor, scheduler_db).await;
    });

    let router = api::build_router(
        db,
        cmd_tx.clone(),
        api_executor,
        api::AuthSettings {
            static_token: config.token.clone(),
            has_admin,
        },
        maintenance,
    );

    tracing::info!("starting server on http://{addr}");

    // server.run 内部已处理 SIGINT:收到信号后停止 accept 并返回 Ok(())。
    let server = desirable::new(&addr);
    let _ = server.run(router).await;
    tracing::info!("server stopped; signaling scheduler to shut down");
    let _ = cmd_tx.send(ControlCmd::Shutdown);

    // 等 scheduler 完成 drain(在途 execution 收尾)。
    let _ = scheduler_handle.await;

    Ok(())
}

/// 删除超过保留期的执行记录并记日志。
async fn run_retention(db: &Db, retention_days: i64) {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(retention_days))
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    match db.purge_executions_older_than(cutoff).await {
        Ok(0) => {}
        Ok(n) => tracing::info!(
            deleted = n,
            "purged executions older than {retention_days} days"
        ),
        Err(e) => tracing::warn!(error = %e, "retention purge failed"),
    }
}
