//! Captures the server library's `log` output in memory.
//!
//! Anything written to stdout/stderr would corrupt the alternate screen, so
//! log lines are kept in a bounded buffer and drawn in a panel instead.

use std::collections::VecDeque;
use std::sync::Mutex;

use log::{Level, LevelFilter, Log, Metadata, Record};

const CAPACITY: usize = 200;

#[derive(Clone)]
pub struct LogLine {
    pub time: String,
    pub level: Level,
    pub message: String,
}

static LINES: Mutex<VecDeque<LogLine>> = Mutex::new(VecDeque::new());
static LOGGER: BufferLogger = BufferLogger;

struct BufferLogger;

impl Log for BufferLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        // Only the server's own lines are interesting; skip HTTP stack chatter
        if !record.target().starts_with("traintime") {
            return;
        }
        let line = LogLine {
            time: chrono::Local::now().format("%H:%M:%S").to_string(),
            level: record.level(),
            message: record.args().to_string().replace('\n', " "),
        };
        if let Ok(mut lines) = LINES.lock() {
            if lines.len() == CAPACITY {
                lines.pop_front();
            }
            lines.push_back(line);
        }
    }

    fn flush(&self) {}
}

pub fn init() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(LevelFilter::Info);
    }
}

/// The most recent `n` lines, oldest first.
pub fn tail(n: usize) -> Vec<LogLine> {
    match LINES.lock() {
        Ok(lines) => lines
            .iter()
            .skip(lines.len().saturating_sub(n))
            .cloned()
            .collect(),
        Err(_) => Vec::new(),
    }
}
