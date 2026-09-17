/// 任务输出的有界累积器:head 固定保留前半预算,tail 滚动保留后半预算,
/// 内存占用与输出总量无关。MAX_OUTPUT_KB = 0 表示不设限。
pub struct BoundedOutput {
    /// 总字节预算(0 = 不限)
    cap: usize,
    head: Vec<u8>,
    tail: Vec<u8>,
    total: u64,
}

impl BoundedOutput {
    pub fn new(max_kb: usize) -> Self {
        let cap = max_kb.saturating_mul(1024);
        Self {
            cap,
            head: Vec::new(),
            tail: Vec::new(),
            total: 0,
        }
    }

    pub fn is_truncated(&self) -> bool {
        self.cap > 0 && self.total > self.cap as u64
    }

    /// 追加一段输出;字节块可能从多字节字符中间切开,finish 时统一 lossy 解码。
    pub fn push(&mut self, chunk: &[u8]) {
        self.total += chunk.len() as u64;
        if self.cap == 0 {
            self.head.extend_from_slice(chunk);
            return;
        }
        let half = self.cap / 2;
        if self.head.len() < half {
            let take = (half - self.head.len()).min(chunk.len());
            self.head.extend_from_slice(&chunk[..take]);
            self.tail.extend_from_slice(&chunk[take..]);
        } else {
            self.tail.extend_from_slice(chunk);
        }
        if self.tail.len() > half {
            let drop = self.tail.len() - half;
            self.tail.drain(..drop);
        }
    }

    /// 当前累积内容的快照:超限时为 head + 截断标记 + tail。
    pub fn snapshot(&self) -> String {
        if !self.is_truncated() {
            return String::from_utf8_lossy(&self.head).to_string();
        }
        let lost = self.total - (self.head.len() + self.tail.len()) as u64;
        format!(
            "{}\n…[truncated {} bytes]…\n{}",
            String::from_utf8_lossy(&self.head),
            lost,
            String::from_utf8_lossy(&self.tail)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_cap_keeps_everything() {
        let mut b = BoundedOutput::new(1); // 1024 bytes
        b.push(b"hello world");
        assert_eq!(b.snapshot(), "hello world");
        assert!(!b.is_truncated());
        assert_eq!(b.total, 11);
    }

    #[test]
    fn over_cap_keeps_head_and_tail_with_marker() {
        let mut b = BoundedOutput::new(1);
        let big = vec![b'a'; 2000];
        b.push(&big);
        let s = b.snapshot();
        assert!(b.is_truncated());
        assert!(s.starts_with(&"a".repeat(512)));
        assert!(s.ends_with(&"a".repeat(512)));
        assert!(s.contains("…[truncated 976 bytes]…"));
    }

    #[test]
    fn multiple_pushes_roll_tail() {
        let mut b = BoundedOutput::new(1);
        b.push(&vec![b'x'; 800]);
        b.push(&vec![b'y'; 800]);
        let s = b.snapshot();
        assert!(s.starts_with(&"x".repeat(512)));
        assert!(s.ends_with(&"y".repeat(512)));
        assert!(s.contains("truncated 576 bytes"));
    }

    #[test]
    fn zero_cap_is_unlimited() {
        let mut b = BoundedOutput::new(0);
        b.push(&vec![b'z'; 100_000]);
        assert_eq!(b.snapshot().len(), 100_000);
        assert!(!b.is_truncated());
    }

    #[test]
    fn multibyte_split_across_pushes_decodes_lossy() {
        let mut b = BoundedOutput::new(0);
        let s = "中文内容";
        let bytes = s.as_bytes();
        b.push(&bytes[..3]); // 切在字符中间
        b.push(&bytes[3..]);
        assert_eq!(b.snapshot(), s);
    }
}
