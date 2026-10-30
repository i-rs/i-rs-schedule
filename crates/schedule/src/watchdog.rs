use crate::db::{Db, ScheduleConfig, Task, TaskExecution};
use crate::executor::Executor;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 宽限期:期望时间过后 5 分钟仍无任何执行尝试才判定漏跑。
const GRACE_SECS: i64 = 300;

/// 每个任务已告警的期望时间(ISO),防止每个扫描周期重复告警。
type Suppress = Arc<Mutex<HashMap<String, String>>>;

/// 时区下 now 之前最近的一次 cron 触发点(UTC);disabled / once / 48h 内无触发点返回 None。
pub fn last_due_utc(task: &Task) -> Option<chrono::DateTime<chrono::Utc>> {
    if !task.enabled {
        return None;
    }
    let ScheduleConfig::Cron { expr } = &task.schedule else {
        return None;
    };
    let schedule = expr.parse::<cron::Schedule>().ok()?;
    let tz = crate::schedule::parse_timezone(&task.timezone);
    let now = chrono::Utc::now();
    let horizon = now - chrono::Duration::hours(48);
    let mut last = None;
    for t in schedule.after(&horizon.with_timezone(&tz)).take(10_000) {
        let t_utc = t.with_timezone(&chrono::Utc);
        if t_utc > now {
            break;
        }
        last = Some(t_utc);
    }
    last
}

/// 共享判定:任务是否存在"已到期但此后无执行"的期望时间。
/// 返回期望时间 ISO;查询失败/未到期/已有执行一律 None(宁可漏报不误报)。
pub async fn missed_expected(db: &Db, task: &Task) -> Option<String> {
    let expected = last_due_utc(task)?;
    let now = chrono::Utc::now();
    if now - expected < chrono::Duration::seconds(GRACE_SECS) {
        return None;
    }
    let expected_iso = expected.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    match db.has_execution_since(&task.id, &expected_iso).await {
        Ok(false) => Some(expected_iso),
        _ => None,
    }
}

/// 单轮扫描:漏跑 → 推送告警(同一期望时间只告警一次)。
async fn scan(db: &Db, ex: &Executor, suppress: &Suppress) {
    let tasks = match db.list_enabled_tasks().await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(error = %e, "watchdog scan list tasks failed");
            return;
        }
    };
    for task in tasks {
        if !task.missed_alert {
            continue;
        }
        let Some(expected_iso) = missed_expected(db, &task).await else {
            continue;
        };
        {
            let mut map = suppress.lock().unwrap();
            if map.get(&task.id).is_some_and(|prev| prev == &expected_iso) {
                continue;
            }
            map.insert(task.id.clone(), expected_iso.clone());
        }
        let (notify_type, notify_url) = ex.resolve_notify_channel(&task);
        let synthetic = TaskExecution {
            id: "-".to_string(),
            task_id: task.id.clone(),
            attempt: 0,
            status: "missed".to_string(),
            output: Some(format!("no execution since {expected_iso}")),
            http_status: None,
            started_at: crate::db::now_iso(),
            finished_at: Some(crate::db::now_iso()),
        };
        ex.notifier().send_channel(
            &notify_type,
            &notify_url,
            &task,
            "task_missed",
            "任务漏跑",
            &synthetic,
            0,
        );
        tracing::warn!(task_id = %task.id, expected = %expected_iso, "missed run detected; alert sent");
    }
}

/// 漏跑检测循环:每 60 秒扫描一轮。
pub async fn run(db: Db, ex: Executor) {
    let suppress: Suppress = Arc::default();
    loop {
        scan(&db, &ex, &suppress).await;
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TaskType;

    fn cron_task(expr: &str, tz: &str) -> Task {
        let mut t = Task::new(
            "wd".into(),
            TaskType::Shell { cmd: "echo".into() },
            ScheduleConfig::Cron { expr: expr.into() },
        );
        t.timezone = tz.into();
        t
    }

    #[test]
    fn last_due_finds_recent_occurrence() {
        // 每小时整点(UTC);now 之前最近一个整点必存在
        let t = cron_task("0 0 * * * *", "UTC");
        let due = last_due_utc(&t).unwrap();
        let now = chrono::Utc::now();
        assert!(due <= now);
        assert!(now - due < chrono::Duration::hours(2));
    }

    #[test]
    fn last_due_respects_timezone() {
        // 上海时区每天 09:00 = UTC 01:00;若现在刚过 UTC 01:00,期望就是它
        let t = cron_task("0 0 9 * * *", "Asia/Shanghai");
        let due = last_due_utc(&t).unwrap();
        assert_eq!(due.format("%H").to_string(), "01");
    }

    #[test]
    fn last_due_none_for_disabled_or_once() {
        let mut t = cron_task("0 0 * * * *", "UTC");
        t.enabled = false;
        assert!(last_due_utc(&t).is_none());
        let once = Task::new(
            "o".into(),
            TaskType::Shell { cmd: "echo".into() },
            ScheduleConfig::Once { delay_secs: 60 },
        );
        assert!(last_due_utc(&once).is_none());
    }
}
