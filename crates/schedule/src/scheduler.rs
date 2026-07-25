use crate::db::{Db, ScheduleConfig, Task, TaskExecution};
use crate::executor::{ExecutionResult, Executor};
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
            ScheduleConfig::Once { delay_secs } => {
                let created_utc = chrono::NaiveDateTime::parse_from_str(
                    created_at,
                    "%Y-%m-%dT%H:%M:%S%.3fZ",
                )
                .ok()
                .or_else(|| {
                    chrono::NaiveDateTime::parse_from_str(created_at, "%Y-%m-%dT%H:%M:%S%.fZ")
                        .ok()
                })
                .map(|dt| dt.and_utc());

                match created_utc {
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
                }
            }
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
            if let Some(delay) = task.schedule.remaining_delay(&task.created_at) {
                let key = self.queue.insert(task.clone(), delay);
                self.keys.insert(task.id.clone(), key);
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

                    if matches!(task.schedule, ScheduleConfig::Once { .. }) {
                        self.keys.remove(&task.id);
                    }

                    let exec = executor.clone();
                    let db = db.clone();
                    let task_clone = task.clone();

                    tokio::spawn(async move {
                        let exec_id = Uuid::new_v4().to_string();
                        let started_at = chrono::Utc::now()
                            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                            .to_string();

                        let _ = db
                            .create_execution(&TaskExecution {
                                id: exec_id.clone(),
                                task_id: task_clone.id.clone(),
                                status: "running".to_string(),
                                output: None,
                                http_status: None,
                                started_at,
                                finished_at: None,
                            })
                            .await;

                        let result = exec.execute(&task_clone).await;

                        let _ = db
                            .update_execution(
                                &exec_id,
                                &result.status,
                                &result.output,
                                result.http_status,
                            )
                            .await;
                    });
                }

                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        ControlCmd::Add(task) => self.insert(task),
                        ControlCmd::Remove(id) => self.remove(&id),
                        ControlCmd::Update(task) => self.update(task),
                    }
                }
            }
        }
    }
}
