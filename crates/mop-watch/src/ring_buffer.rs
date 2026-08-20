use crate::traits::LogLine;
use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

#[derive(Clone)]
pub struct ResourceLogBuffer {
    max_lines: usize,
    max_line_bytes: usize,
    buffer: Arc<RwLock<VecDeque<LogLine>>>,
    tx: broadcast::Sender<LogLine>,
}

impl ResourceLogBuffer {
    pub fn new(max_lines: usize, max_line_bytes: usize) -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            max_lines,
            max_line_bytes,
            buffer: Arc::new(RwLock::new(VecDeque::with_capacity(max_lines.min(1000)))),
            tx,
        }
    }

    pub async fn push(&self, mut log: LogLine) {
        if log.line.len() > self.max_line_bytes {
            log.line.truncate(self.max_line_bytes);
            log.line.push_str("... [truncated]");
        }

        {
            let mut write = self.buffer.write().await;
            if write.len() >= self.max_lines {
                write.pop_front();
            }
            write.push_back(log.clone());
        }

        // Broadcast to live stream subscribers (ignore send error if no active receivers)
        let _ = self.tx.send(log);
    }

    pub async fn get_snapshot(&self, tail: usize, since: Option<DateTime<Utc>>) -> Vec<LogLine> {
        let read = self.buffer.read().await;
        let mut filtered: Vec<LogLine> = if let Some(s) = since {
            read.iter().filter(|l| l.ts > s).cloned().collect()
        } else {
            read.iter().cloned().collect()
        };

        if filtered.len() > tail {
            let start = filtered.len() - tail;
            filtered.drain(0..start);
        }

        filtered
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LogLine> {
        self.tx.subscribe()
    }
}
