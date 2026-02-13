use std::io::{self, Read};
use std::time::{Duration, Instant};

use crate::cancel::CancelToken;

pub struct ProgressReader<R> {
    inner: R,
    enabled: bool,
    total_bytes: Option<u64>,
    read_bytes: u64,
    last_report: Instant,
    cancel: CancelToken,
}

impl<R> ProgressReader<R> {
    pub fn new(inner: R, enabled: bool, total_bytes: Option<u64>, cancel: CancelToken) -> Self {
        Self {
            inner,
            enabled,
            total_bytes,
            read_bytes: 0,
            last_report: Instant::now(),
            cancel,
        }
    }

    pub fn finish(&self) {
        if self.enabled {
            eprintln!("progress: 100%");
        }
    }
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.cancel.is_cancelled() {
            return Err(io::Error::new(io::ErrorKind::Other, "cancelled"));
        }

        let bytes = self.inner.read(buf)?;
        self.read_bytes += bytes as u64;

        if self.enabled && bytes > 0 && self.last_report.elapsed() >= Duration::from_secs(1) {
            if let Some(total) = self.total_bytes {
                let percent = (self.read_bytes * 100) / total.max(1);
                eprintln!(
                    "progress: {} / {} ({}%)",
                    format_bytes(self.read_bytes),
                    format_bytes(total),
                    percent
                );
            } else {
                eprintln!("progress: {}", format_bytes(self.read_bytes));
            }
            self.last_report = Instant::now();
        }

        Ok(bytes)
    }
}

fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;

    if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.2} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.2} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}
