use crate::db::{Db, ScheduleConfig, Task};
use crate::executor::Executor;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::time::delay_queue::{self, DelayQueue};

pub enum ControlCmd {
    Add(Task),
    Remove(String),
    Update(Task),
    Shutdown,
}

pub struct Scheduler {
    queue: DelayQueue<Task>,
    keys: HashMap<String, delay_queue::Key>,
    join_set: tokio::task::JoinSet<()>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            queue: DelayQueue::new(),
            keys: HashMap::new(),
            join_set: tokio::task::JoinSet::new(),
        }
    }

    pub fn load_tasks(&mut self, tasks: Vec<Task>) {
        for task in tasks {
            match task.schedule.remaining_delay(&task.created_at) {
                Some(delay) => {
                    let key = self.queue.insert(task.clone(), delay);
                    self.keys.insert(task.id.clone(), key);
                }
                None => {
                    tracing::info!(
                        task_id = %task.id,
                        name = %task.name,
                        "skipping expired once-task on startup"
                    );
                }
            }
        }
    }

    pub fn insert(&mut self, task: Task) {
        let delay = task.schedule.next_delay();
        let key = self.queue.insert(task.clone(), delay);
        self.keys.insert(task.id.clone(), key);
    }

    pub fn remove(&mut self, id: &str) {
        if let Some(key) = self.keys.remove(id) {
            self.queue.remove(&key);
        }
    }

    pub fn update(&mut self, task: Task) {
        self.remove(&task.id);
        if task.enabled {
            self.insert(task);
        }
    }

    pub async fn run(
        mut self,
        mut cmd_rx: mpsc::UnboundedReceiver<ControlCmd>,
        executor: Arc<Executor>,
        db: Db,
    ) {
        loop {
            tokio::select! {
                Some(expired) = self.queue.next() => {
                    let task = expired.into_inner();

                    match &task.schedule {
                        ScheduleConfig::Once { .. } => {
                            self.keys.remove(&task.id);
                        }
                        ScheduleConfig::Cron { .. } => {
                            let next = task.schedule.next_delay();
                            let key = self.queue.insert(task.clone(), next);
                            self.keys.insert(task.id.clone(), key);
                        }
                    }

                    tracing::debug!(task_id = %task.id, name = %task.name, "task due");

                    let exec = executor.clone();
                    let db = db.clone();
                    let task_clone = task.clone();

                    self.join_set.spawn(async move {
                        let _ = exec.execute_and_record(&db, &task_clone).await;
                    });
                }

                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        ControlCmd::Add(task) => {
                            tracing::debug!(task_id = %task.id, "scheduler add");
                            self.insert(task);
                        }
                        ControlCmd::Remove(id) => {
                            tracing::debug!(task_id = %id, "scheduler remove");
                            self.remove(&id);
                        }
                        ControlCmd::Update(task) => {
                            tracing::debug!(task_id = %task.id, "scheduler update");
                            self.update(task);
                        }
                        ControlCmd::Shutdown => {
                            tracing::info!(
                                "scheduler shutdown requested; draining in-flight executions"
                            );
                            break;
                        }
                    }
                }
            }
        }

        // 收到 Shutdown 后,等待在途 execution 完成(HTTP/shell 各自有超时,
        // 这里再加一个总 timeout 兜底,避免某个卡死的 task 拖住退出)。
        let drain = async { while self.join_set.join_next().await.is_some() {} };
        if tokio::time::timeout(Duration::from_secs(35), drain)
            .await
            .is_err()
        {
            tracing::warn!("shutdown drain timed out after 35s; aborting remaining tasks");
            self.join_set.abort_all();
        }
        tracing::info!("scheduler stopped");
    }
}
