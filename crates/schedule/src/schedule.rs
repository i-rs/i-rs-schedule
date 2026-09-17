use crate::db::Task;
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScheduleConfig {
    Cron { expr: String },
    Once { delay_secs: u64 },
}

/// 解析 IANA 时区;无效值回退 UTC 并告警(防御绕过校验的写入)。
pub fn parse_timezone(s: &str) -> Tz {
    s.parse::<Tz>().unwrap_or_else(|_| {
        tracing::warn!(timezone = %s, "invalid timezone; falling back to UTC");
        chrono_tz::UTC
    })
}

/// 创建/更新时校验时区字符串。
pub fn validate_timezone(s: &str) -> Result<(), String> {
    s.parse::<Tz>()
        .map(|_| ())
        .map_err(|e| format!("invalid timezone {s:?}: {e}"))
}

impl ScheduleConfig {
    /// 计算到下一次触发的时间间隔(按任务时区)。
    ///
    /// cron 解析失败时回退到 1 小时并打 warn(防御 db 被直接写入绕过创建校验)。
    pub fn next_delay(&self, tz: Tz) -> Duration {
        match self {
            ScheduleConfig::Cron { expr } => match expr.parse::<cron::Schedule>() {
                Ok(schedule) => {
                    let now = chrono::Utc::now();
                    match schedule.upcoming(tz).next() {
                        Some(next) => {
                            let delta = (next.with_timezone(&chrono::Utc) - now)
                                .num_milliseconds()
                                .max(0);
                            Duration::from_millis(delta as u64)
                        }
                        None => {
                            tracing::warn!(
                                expr = %expr,
                                "cron yielded no upcoming fire; retrying in 1h"
                            );
                            Duration::from_secs(3600)
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, expr = %expr, "cron parse failed; retrying in 1h");
                    Duration::from_secs(3600)
                }
            },
            ScheduleConfig::Once { delay_secs } => Duration::from_secs(*delay_secs),
        }
    }

    /// 启动加载时,根据任务创建时间计算剩余延迟;已过期则返回 None。
    pub fn remaining_delay(&self, created_at: &str, tz: Tz) -> Option<Duration> {
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
            ScheduleConfig::Cron { .. } => Some(self.next_delay(tz)),
        }
    }

    /// 校验调度配置是否合法。cron 表达式无效时返回错误信息。
    pub fn validate(&self) -> Result<(), String> {
        match self {
            ScheduleConfig::Cron { expr } => match expr.parse::<cron::Schedule>() {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("invalid cron expr {expr:?}: {e}")),
            },
            ScheduleConfig::Once { .. } => Ok(()),
        }
    }
}

/// 计算任务的下一次执行时间(UTC ISO 字符串);disabled / 已过期 once 返回 None。
pub fn next_run_at(task: &Task) -> Option<String> {
    if !task.enabled {
        return None;
    }
    let tz = parse_timezone(&task.timezone);
    match &task.schedule {
        ScheduleConfig::Cron { expr } => {
            let schedule = expr.parse::<cron::Schedule>().ok()?;
            schedule.upcoming(tz).next().map(|t| {
                t.with_timezone(&chrono::Utc)
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            })
        }
        ScheduleConfig::Once { delay_secs } => {
            let created = crate::db::parse_iso(&task.created_at)?;
            let fire_at = created + chrono::Duration::seconds(*delay_secs as i64);
            if fire_at > chrono::Utc::now() {
                Some(fire_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ScheduleConfig, TaskType};

    fn task(schedule: ScheduleConfig, timezone: &str, enabled: bool) -> Task {
        Task {
            id: "t1".into(),
            name: "test".into(),
            task_type: TaskType::Shell { cmd: "echo".into() },
            enabled,
            schedule,
            timezone: timezone.into(),
            timeout_secs: 30,
            max_retries: 0,
            max_concurrent: 1,
            trigger_task_ids: Vec::new(),
            tags: Vec::new(),
            trigger_on: "success".into(),
            notify_type: "none".into(),
            notify_url: String::new(),
            created_at: crate::db::now_iso(),
            updated_at: crate::db::now_iso(),
        }
    }

    #[test]
    fn shanghai_9am_is_1am_utc() {
        // 上海时区的"每天 9 点"对应的下次触发应为 UTC 01:00
        let t = task(
            ScheduleConfig::Cron {
                expr: "0 0 9 * * *".into(),
            },
            "Asia/Shanghai",
            true,
        );
        let next = next_run_at(&t).unwrap();
        assert_eq!(next.get(11..13).unwrap(), "01");
    }

    #[test]
    fn utc_9am_is_9am_utc() {
        let t = task(
            ScheduleConfig::Cron {
                expr: "0 0 9 * * *".into(),
            },
            "UTC",
            true,
        );
        let next = next_run_at(&t).unwrap();
        assert_eq!(next.get(11..13).unwrap(), "09");
    }

    #[test]
    fn invalid_timezone_falls_back_to_utc() {
        assert_eq!(parse_timezone("Mars/Olympus"), chrono_tz::UTC);
    }

    #[test]
    fn validate_timezone_rejects_garbage() {
        assert!(validate_timezone("Asia/Shanghai").is_ok());
        assert!(validate_timezone("not-a-zone").is_err());
    }

    #[test]
    fn cron_validate_rejects_bad_expr() {
        assert!(
            ScheduleConfig::Cron {
                expr: "0 0 9 * * *".into()
            }
            .validate()
            .is_ok()
        );
        assert!(
            ScheduleConfig::Cron {
                expr: "garbage".into()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn next_run_at_respects_disabled_and_expired_once() {
        let mut t = task(ScheduleConfig::Once { delay_secs: 60 }, "UTC", true);
        // created_at 刚生成,delay 60s → 未来
        assert!(next_run_at(&t).is_some());
        // disabled → None
        t.enabled = false;
        assert!(next_run_at(&t).is_none());
        // once 已过期 → None
        t.enabled = true;
        t.created_at = "2020-01-01T00:00:00.000Z".into();
        assert!(next_run_at(&t).is_none());
    }
}
