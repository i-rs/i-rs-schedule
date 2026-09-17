use serde::Deserialize;

/// `config.toml` 可选配置。优先级:环境变量 > 配置文件 > 默认值。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FileConfig {
    pub db_path: Option<String>,
    pub port: Option<u16>,
    pub token: Option<String>,
    pub retention_days: Option<i64>,
    pub admin_user: Option<String>,
    pub admin_password: Option<String>,
    pub notify_type: Option<String>,
    pub notify_url: Option<String>,
    pub max_output_kb: Option<usize>,
    pub global_max_concurrent: Option<usize>,
    pub backup_dir: Option<String>,
    pub backup_keep: Option<usize>,
}

impl FileConfig {
    /// 从指定路径加载;文件不存在返回默认值,解析失败打 warn 并忽略。
    pub fn load(path: &str) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        match toml::from_str::<FileConfig>(&text) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(path = %path, error = %e, "invalid config file; ignoring");
                Self::default()
            }
        }
    }
}

/// 运行时生效配置:环境变量覆盖配置文件。
#[derive(Debug, Clone)]
pub struct Config {
    pub db_path: String,
    pub port: u16,
    pub token: Option<String>,
    pub retention_days: i64,
    pub admin_user: Option<String>,
    pub admin_password: Option<String>,
    pub notify_type: Option<String>,
    pub notify_url: Option<String>,
    /// 任务输出持久化上限(KB,0 = 不限)
    pub max_output_kb: usize,
    /// 全局并发执行上限(超限等待)
    pub global_max_concurrent: usize,
    /// 自动备份目录(未配置 = 关闭)
    pub backup_dir: Option<String>,
    /// 备份保留份数
    pub backup_keep: usize,
}

impl Config {
    pub fn load() -> Self {
        let file = Self::load_file("config.toml");
        let env = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());

        let db_path = env("SCHEDULE_DB")
            .or(file.db_path)
            .unwrap_or_else(|| "./data/schedule.db".to_string());
        let port = env("SCHEDULE_PORT")
            .and_then(|v| v.parse().ok())
            .or(file.port)
            .unwrap_or(3000);
        let token = env("SCHEDULE_TOKEN").or(file.token);
        let retention_days = env("RETENTION_DAYS")
            .and_then(|v| v.parse().ok())
            .or(file.retention_days)
            .unwrap_or(30);
        let admin_user = env("ADMIN_USER").or(file.admin_user);
        let admin_password = env("ADMIN_PASSWORD").or(file.admin_password);
        let notify_type = env("NOTIFY_TYPE")
            .or(file.notify_type)
            .filter(|v| v != "none");
        let notify_url = env("NOTIFY_URL").or(file.notify_url);
        let max_output_kb = env("MAX_OUTPUT_KB")
            .and_then(|v| v.parse().ok())
            .or(file.max_output_kb)
            .unwrap_or(64);
        let global_max_concurrent = env("GLOBAL_MAX_CONCURRENCY")
            .and_then(|v| v.parse().ok())
            .or(file.global_max_concurrent)
            .unwrap_or(32);
        let backup_dir = env("BACKUP_DIR")
            .or(file.backup_dir)
            .filter(|v| !v.is_empty());
        let backup_keep = env("BACKUP_KEEP")
            .and_then(|v| v.parse().ok())
            .or(file.backup_keep)
            .unwrap_or(7);

        Self {
            db_path,
            port,
            token,
            retention_days,
            admin_user,
            admin_password,
            notify_type,
            notify_url,
            max_output_kb,
            global_max_concurrent,
            backup_dir,
            backup_keep,
        }
    }

    fn load_file(path: &str) -> FileConfig {
        FileConfig::load(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_toml_file() {
        std::fs::write(
            "/tmp/irs-config-test.toml",
            "db_path = \"/tmp/x.db\"\nport = 8080\ntoken = \"abc\"\nretention_days = 7\n",
        )
        .unwrap();
        let c = FileConfig::load("/tmp/irs-config-test.toml");
        assert_eq!(c.db_path.as_deref(), Some("/tmp/x.db"));
        assert_eq!(c.port, Some(8080));
        assert_eq!(c.token.as_deref(), Some("abc"));
        assert_eq!(c.retention_days, Some(7));
        std::fs::remove_file("/tmp/irs-config-test.toml").unwrap();
    }

    #[test]
    fn missing_file_yields_default() {
        let c = FileConfig::load("/tmp/nonexistent-config.toml");
        assert_eq!(c.port, None);
        assert_eq!(c.retention_days, None);
    }

    #[test]
    fn invalid_toml_ignored() {
        std::fs::write("/tmp/irs-config-bad.toml", "not [valid toml {{{{").unwrap();
        let c = FileConfig::load("/tmp/irs-config-bad.toml");
        assert_eq!(c.port, None);
        std::fs::remove_file("/tmp/irs-config-bad.toml").unwrap();
    }
}
