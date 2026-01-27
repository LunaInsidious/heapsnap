use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use crate::error::SnapshotError;

#[derive(Clone, Debug)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

pub fn install_ctrlc_handler() -> Result<CancelToken, SnapshotError> {
    static TOKEN: OnceLock<Arc<AtomicBool>> = OnceLock::new();

    if let Some(flag) = TOKEN.get() {
        return Ok(CancelToken(flag.clone()));
    }

    let flag = Arc::new(AtomicBool::new(false));
    let handler_flag = flag.clone();
    ctrlc::set_handler(move || {
        handler_flag.store(true, Ordering::SeqCst);
    })
    .map_err(|err| SnapshotError::InvalidData {
        details: format!("failed to install Ctrl-C handler: {err}"),
    })?;

    let _ = TOKEN.set(flag.clone());
    Ok(CancelToken(flag))
}
