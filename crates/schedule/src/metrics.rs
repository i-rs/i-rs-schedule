/// 进程内指标(原子计数,零锁):/metrics 直方图与计数器用。
/// 桶为原始计数,渲染时转累计(Prometheus 语义)。
pub struct Metrics {
    /// 按 BUCKET_MS 升序的原始落入数
    buckets: [std::sync::atomic::AtomicU64; BUCKET_MS.len()],
    sum_ms: std::sync::atomic::AtomicU64,
    total: std::sync::atomic::AtomicU64,
    skipped: std::sync::atomic::AtomicU64,
}

/// 耗时桶边界(ms),升序;+Inf 隐含在 total 中。
pub const BUCKET_MS: [u64; 12] = [
    10, 50, 100, 250, 500, 1_000, 2_500, 5_000, 10_000, 30_000, 60_000, 120_000,
];

impl Default for Metrics {
    fn default() -> Self {
        Self {
            buckets: std::array::from_fn(|_| std::sync::atomic::AtomicU64::new(0)),
            sum_ms: std::sync::atomic::AtomicU64::new(0),
            total: std::sync::atomic::AtomicU64::new(0),
            skipped: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl Metrics {
    /// 记录一次执行耗时。
    pub fn record(&self, duration_ms: u64) {
        use std::sync::atomic::Ordering::Relaxed;
        self.total.fetch_add(1, Relaxed);
        self.sum_ms.fetch_add(duration_ms, Relaxed);
        for (i, bound) in BUCKET_MS.iter().enumerate() {
            if duration_ms <= *bound {
                self.buckets[i].fetch_add(1, Relaxed);
            }
        }
    }

    /// 记录一次 skipped(并发跳过 / 维护错过 / 入库失败等)。
    pub fn bump_skipped(&self) {
        self.skipped
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn skipped(&self) -> u64 {
        self.skipped.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Prometheus 文本片段(histogram,累计桶)。
    pub fn render_histogram(&self) -> String {
        use std::sync::atomic::Ordering::Relaxed;
        let mut out = String::from(
            "# HELP irs_execution_duration_seconds Task execution durations.\n\
             # TYPE irs_execution_duration_seconds histogram\n",
        );
        // record() 已按累计语义填充(值 ≤ 边界的所有桶 +1),此处直接输出
        for (i, bound) in BUCKET_MS.iter().enumerate() {
            let count = self.buckets[i].load(Relaxed);
            let le = if *bound < 1000 {
                format!("0.{bound:03}")
            } else {
                format!("{}", *bound as f64 / 1000.0)
            };
            out.push_str(&format!(
                "irs_execution_duration_seconds_bucket{{le=\"{le}\"}} {count}\n"
            ));
        }
        let total = self.total.load(Relaxed);
        let sum = self.sum_ms.load(Relaxed);
        out.push_str(&format!(
            "irs_execution_duration_seconds_bucket{{le=\"+Inf\"}} {total}\n\
             irs_execution_duration_seconds_sum {}\n\
             irs_execution_duration_seconds_count {total}\n",
            sum as f64 / 1000.0,
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_is_cumulative_and_sums() {
        let m = Metrics::default();
        m.record(5); // le=0.010
        m.record(80); // le=0.100
        m.record(1500); // le=2.5
        m.bump_skipped();
        let text = m.render_histogram();
        assert!(text.contains("irs_execution_duration_seconds_bucket{le=\"0.010\"} 1"));
        assert!(text.contains("irs_execution_duration_seconds_bucket{le=\"0.100\"} 2"));
        assert!(text.contains("irs_execution_duration_seconds_bucket{le=\"2.5\"} 3"));
        assert!(text.contains("irs_execution_duration_seconds_bucket{le=\"+Inf\"} 3"));
        assert!(text.contains("irs_execution_duration_seconds_count 3"));
        // sum = 1.585s
        assert!(text.contains("irs_execution_duration_seconds_sum 1.585"));
        assert_eq!(m.skipped(), 1);
    }
}
