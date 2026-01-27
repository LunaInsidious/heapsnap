use std::fmt;

#[derive(Debug)]
pub enum SnapshotError {
    Io(std::io::Error),
    Json(serde_json::Error),
    MetaMismatch { details: String },
    InvalidData { details: String },
    Cancelled,
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::Io(err) => write!(f, "I/O error: {err}"),
            SnapshotError::Json(err) => write!(f, "JSON parse error: {err}"),
            SnapshotError::MetaMismatch { details } => write!(f, "meta mismatch: {details}"),
            SnapshotError::InvalidData { details } => write!(f, "invalid data: {details}"),
            SnapshotError::Cancelled => write!(f, "cancelled by user"),
        }
    }
}

impl std::error::Error for SnapshotError {}

impl From<std::io::Error> for SnapshotError {
    fn from(value: std::io::Error) -> Self {
        SnapshotError::Io(value)
    }
}

impl From<serde_json::Error> for SnapshotError {
    fn from(value: serde_json::Error) -> Self {
        SnapshotError::Json(value)
    }
}
