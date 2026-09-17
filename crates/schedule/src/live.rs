use crate::output::BoundedOutput;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

/// 单个运行中 execution 的实时输出快照。
#[derive(Debug, Clone, serde::Serialize)]
pub struct LiveSnapshot {
    pub version: u64,
    pub output: String,
    pub total: u64,
    pub done: bool,
}

struct LiveInner {
    buf: BoundedOutput,
    version: u64,
    done: bool,
}

/// 单个运行中 execution 的实时输出:有界累积 + 版本号变更通知(watch)。
pub struct LiveOutput {
    inner: Mutex<LiveInner>,
    version_tx: watch::Sender<u64>,
}

impl LiveOutput {
    pub(crate) fn new(max_output_kb: usize) -> Self {
        let (version_tx, _) = watch::channel(0);
        Self {
            inner: Mutex::new(LiveInner {
                buf: BoundedOutput::new(max_output_kb),
                version: 0,
                done: false,
            }),
            version_tx,
        }
    }

    /// 追加一块输出并递增版本;空块忽略(不触发通知)。
    pub fn push(&self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        inner.buf.push(chunk);
        inner.version += 1;
        let _ = self.version_tx.send(inner.version);
    }

    /// 标记输出结束(幂等);此后 snapshot 的 done 恒为 true。
    pub fn finish(&self) {
        let mut inner = self.inner.lock().unwrap();
        if inner.done {
            return;
        }
        inner.done = true;
        inner.version += 1;
        let _ = self.version_tx.send(inner.version);
    }

    /// 当前快照(不等待)。
    pub fn snapshot(&self) -> LiveSnapshot {
        let inner = self.inner.lock().unwrap();
        LiveSnapshot {
            version: inner.version,
            output: inner.buf.snapshot(),
            total: inner.buf.total(),
            done: inner.done,
        }
    }

    /// 长轮询语义:version 已超过 cursor 或已结束 → 立即返回;
    /// 否则等变更,最长 max_wait 超时后返回当前快照(客户端续轮)。
    pub async fn wait_snapshot(&self, cursor: u64, max_wait: std::time::Duration) -> LiveSnapshot {
        let mut rx = self.version_tx.subscribe();
        loop {
            let snap = self.snapshot();
            if snap.version > cursor || snap.done {
                return snap;
            }
            match tokio::time::timeout(max_wait, rx.changed()).await {
                Ok(Ok(())) => continue,
                // 发送端消亡或超时:返回当前快照,由客户端决定是否续轮
                _ => return snap,
            }
        }
    }
}

/// 运行中 execution 的实时输出注册表:exec_id → LiveOutput。
/// 执行结束后由 executor 移除条目,后续查询回落到 DB。
#[derive(Clone, Default)]
pub struct LiveRegistry {
    map: Arc<Mutex<HashMap<String, Arc<LiveOutput>>>>,
}

impl LiveRegistry {
    pub fn create(&self, exec_id: &str, max_output_kb: usize) -> Arc<LiveOutput> {
        let live = Arc::new(LiveOutput::new(max_output_kb));
        self.map
            .lock()
            .unwrap()
            .insert(exec_id.to_string(), live.clone());
        live
    }

    pub fn get(&self, exec_id: &str) -> Option<Arc<LiveOutput>> {
        self.map.lock().unwrap().get(exec_id).cloned()
    }

    pub fn remove(&self, exec_id: &str) {
        self.map.lock().unwrap().remove(exec_id);
    }
}

/// 全局变更事件计数器:任何执行/任务变化 +1,前端长轮询此游标驱动刷新。
#[derive(Clone)]
pub struct Events {
    tx: Arc<watch::Sender<u64>>,
}

impl Events {
    pub fn new() -> Self {
        let (tx, _) = watch::channel(0);
        Self { tx: Arc::new(tx) }
    }

    pub fn bump(&self) {
        let next = self.tx.borrow().wrapping_add(1);
        // send 在无接收者时不存值,事件会在两次长轮询的间隙丢失;
        // send_replace 无条件存储,保证后续订阅者能看到游标前进
        self.tx.send_replace(next);
    }

    /// 长轮询语义:游标已前进 → 立即返回;否则等变更,最长 max_wait。
    pub async fn wait_for(&self, cursor: u64, max_wait: std::time::Duration) -> u64 {
        let mut rx = self.tx.subscribe();
        if *rx.borrow_and_update() > cursor {
            return *rx.borrow();
        }
        let _ = tokio::time::timeout(max_wait, rx.changed()).await;
        *rx.borrow()
    }
}

impl Default for Events {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn push_increments_version_and_snapshot_reflects_content() {
        let live = LiveOutput::new(64);
        assert_eq!(live.snapshot().version, 0);
        live.push(b"hello ");
        live.push(b"world");
        let snap = live.snapshot();
        assert_eq!(snap.version, 2);
        assert_eq!(snap.output, "hello world");
        assert_eq!(snap.total, 11);
        assert!(!snap.done);
    }

    #[tokio::test]
    async fn wait_snapshot_returns_immediately_when_ahead() {
        let live = LiveOutput::new(64);
        live.push(b"data");
        let started = std::time::Instant::now();
        let snap = live
            .wait_snapshot(0, std::time::Duration::from_secs(10))
            .await;
        assert_eq!(snap.version, 1);
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    #[tokio::test]
    async fn wait_snapshot_times_out_without_change() {
        let live = LiveOutput::new(64);
        let started = std::time::Instant::now();
        let snap = live
            .wait_snapshot(0, std::time::Duration::from_millis(80))
            .await;
        assert_eq!(snap.version, 0);
        assert!(started.elapsed() >= std::time::Duration::from_millis(80));
    }

    #[tokio::test]
    async fn wait_snapshot_wakes_on_change() {
        let live = Arc::new(LiveOutput::new(64));
        let bg = live.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            bg.push(b"late");
        });
        let snap = live
            .wait_snapshot(0, std::time::Duration::from_secs(5))
            .await;
        assert_eq!(snap.version, 1);
        assert_eq!(snap.output, "late");
    }

    #[tokio::test]
    async fn finish_marks_done_and_wait_returns() {
        let live = LiveOutput::new(64);
        live.finish();
        let snap = live
            .wait_snapshot(0, std::time::Duration::from_secs(5))
            .await;
        assert!(snap.done);
    }

    #[test]
    fn registry_create_get_remove() {
        let reg = LiveRegistry::default();
        assert!(reg.get("e1").is_none());
        let live = reg.create("e1", 64);
        live.push(b"x");
        assert_eq!(reg.get("e1").unwrap().snapshot().output, "x");
        reg.remove("e1");
        assert!(reg.get("e1").is_none());
    }

    #[tokio::test]
    async fn events_wait_for_immediate_and_timeout() {
        let ev = Events::new();
        ev.bump();
        // 已前进:立即返回
        let started = std::time::Instant::now();
        assert_eq!(ev.wait_for(0, std::time::Duration::from_secs(5)).await, 1);
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        // 无变化:超时返回原值
        let started = std::time::Instant::now();
        assert_eq!(
            ev.wait_for(1, std::time::Duration::from_millis(60)).await,
            1
        );
        assert!(started.elapsed() >= std::time::Duration::from_millis(60));
    }
}
