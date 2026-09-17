use crate::db::Db;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// 最近一次成功备份的时间(healthz 展示)。
#[derive(Clone, Default)]
pub struct BackupState(Arc<Mutex<Option<String>>>);

impl BackupState {
    pub fn set(&self, ts: String) {
        *self.0.lock().unwrap() = Some(ts);
    }

    pub fn get(&self) -> Option<String> {
        self.0.lock().unwrap().clone()
    }
}

/// 执行一次备份:VACUUM INTO 时间戳文件(在线备份,不锁库),超出保留份数删最旧。
/// 返回备份时间戳。
pub async fn run_backup(db: &Db, dir: &str, keep: usize) -> anyhow::Result<String> {
    let dir_path = Path::new(dir);
    tokio::fs::create_dir_all(dir_path).await?;
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f").to_string();
    let file = dir_path.join(format!("schedule-backup-{ts}.db"));
    db.vacuum_into(&file.to_string_lossy()).await?;
    tracing::info!(file = %file.display(), "backup created");
    prune_old_backups(dir_path, keep).await;
    Ok(ts)
}

/// 只保留最近 keep 份备份文件(按文件名 = 时间戳排序)。
async fn prune_old_backups(dir: &Path, keep: usize) {
    if keep == 0 {
        return;
    }
    let mut rd = match tokio::fs::read_dir(dir).await {
        Ok(rd) => rd,
        Err(e) => {
            tracing::warn!(error = %e, dir = %dir.display(), "read backup dir failed");
            return;
        }
    };
    let mut backups: Vec<String> = Vec::new();
    while let Some(entry) = rd.next_entry().await.ok().flatten() {
        if let Ok(name) = entry.file_name().into_string()
            && name.starts_with("schedule-backup-")
        {
            backups.push(name);
        }
    }
    backups.sort();
    while backups.len() > keep {
        let victim = backups.remove(0);
        match tokio::fs::remove_file(dir.join(&victim)).await {
            Ok(()) => tracing::info!(file = %victim, "pruned old backup"),
            Err(e) => tracing::warn!(error = %e, file = %victim, "prune backup failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> (Db, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("irs-backup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::new(dir.join("t.db").to_str().unwrap()).unwrap();
        (db, dir)
    }

    #[tokio::test]
    async fn backup_creates_file_and_reports_state() {
        let (db, dir) = temp_db();
        let backup_dir = dir.join("backups");
        let state = BackupState::default();
        assert!(state.get().is_none());

        let ts = run_backup(&db, backup_dir.to_str().unwrap(), 3)
            .await
            .unwrap();
        state.set(ts);
        assert!(state.get().is_some());
        assert_eq!(std::fs::read_dir(&backup_dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn prune_keeps_only_newest() {
        let (db, dir) = temp_db();
        let backup_dir = dir.join("backups");
        std::fs::create_dir_all(&backup_dir).unwrap();
        // 预置 3 份旧备份,keep = 2 → 再备份 1 份后应只剩 2 份
        for name in [
            "schedule-backup-20260101-000000-000.db",
            "schedule-backup-20260102-000000-000.db",
            "schedule-backup-20260103-000000-000.db",
        ] {
            std::fs::write(backup_dir.join(name), b"old").unwrap();
        }
        run_backup(&db, backup_dir.to_str().unwrap(), 2)
            .await
            .unwrap();
        let remaining: Vec<String> = std::fs::read_dir(&backup_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(remaining.len(), 2);
        assert!(remaining.iter().all(|n| !n.contains("20260101")));
        std::fs::remove_dir_all(&dir).ok();
    }
}
