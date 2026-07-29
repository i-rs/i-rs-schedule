use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScheduleConfig {
    Cron { expr: String },
    Once { delay_secs: u64 },
}

impl ScheduleConfig {
    /// 计算到下一次触发的时间间隔。
    ///
    /// cron 解析失败时回退到 1 小时并打 warn(防御 db 被直接写入绕过创建校验)。
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
