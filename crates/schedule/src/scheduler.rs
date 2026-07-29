use crate::db::{Db, ScheduleConfig, Task, TaskExecution};
use crate::executor::Executor;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::time::delay_queue::{self, DelayQueue};
use uuid::Uuid;

pub enum ControlCmd {
    Add(Task),
    Remove(String),
    Update(Task),
}

pub struct Scheduler {
    queue: DelayQueue<Task>,
    keys: HashMap<String, delay_queue::Key>,
}

impl ScheduleConfig {
    pub fn next_delay(&self) -> Duration {
        match self {
            ScheduleConfig::Cron { expr } => match expr.parse::<cron::Schedule>() {
                Ok(schedule) => {
                    let now = chrono::Utc::now();
                    match schedule.upcoming(chrono::Utc).next() {
                        Some(next) => {
                            let delta = (next - now).num_milliseconds().max(0);
                            Duration::from_millis(delta as u64)
                        }
                        None => Duration::from_secs(3600),
                    }
                }
                Err(_) => Duration::from_secs(3600),
            },
            ScheduleConfig::Once { delay_secs } => Duration::from_secs(*delay_secs),
        }
    }

    pub fn remaining_delay(&self, created_at: &str) -> Option<Duration> {
        match self {
            ScheduleConfig::Once { delay_secs } => match crate::db::parse_iso(created_at) {
                Some(created) => {
                    let now = chrono::Utc::now();
                    let elapsed = (now - created).num_seconds().max(0) as u64;
                    if elapsed >= *delay_secs {
                        None
                    } else {
                        Some(Duration::from_secs(*delay_secs - elapsed))
                    }
                }
                None => Some(Duration::from_secs(*delay_secs)),
            },
            ScheduleConfig::Cron { .. } => Some(self.next_delay()),
        }
    }
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            queue: DelayQueue::new(),
            keys: HashMap::new(),
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

                    tokio::spawn(async move {
                        let exec_id = Uuid::new_v4().to_string();
                        let started_at = crate::db::now_iso();
                        let start = std::time::Instant::now();

                        if let Err(e) = db
                            .create_execution(&TaskExecution {
                                id: exec_id.clone(),
                                task_id: task_clone.id.clone(),
                                status: "running".to_string(),
                                output: None,
                                http_status: None,
                                started_at,
                                finished_at: None,
                            })
                            .await
                        {
                            tracing::warn!(
                                error = %e,
                                task_id = %task_clone.id,
                                "create execution failed; skipping run"
                            );
                            return;
                        }

                        tracing::info!(
                            task_id = %task_clone.id,
                            exec_id = %exec_id,
                            "execution started"
                        );

                        let result = exec.execute(&task_clone).await;

                        if let Err(e) = db
                            .update_execution(
                                &exec_id,
                                &result.status,
                                &result.output,
                                result.http_status,
                            )
                            .await
                        {
                            tracing::warn!(
                                error = %e,
                                task_id = %task_clone.id,
                                exec_id = %exec_id,
                                "update execution failed"
                            );
                        }

                        tracing::info!(
                            task_id = %task_clone.id,
                            exec_id = %exec_id,
                            status = %result.status,
                            duration_ms = start.elapsed().as_millis() as u64,
                            "execution finished"
                        );
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
                    }
                }
            }
        }
    }
}
