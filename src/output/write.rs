use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::error::SnapshotError;

pub fn write_atomic(path: &Path, content: &str) -> Result<(), SnapshotError> {
    let temp_path = temp_path(path);
    let result = write_file(&temp_path, content);
    if let Err(err) = result {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }
    fs::rename(&temp_path, path).map_err(SnapshotError::Io)?;
    Ok(())
}

fn write_file(path: &Path, content: &str) -> Result<(), SnapshotError> {
    let mut file = File::create(path).map_err(SnapshotError::Io)?;
    file.write_all(content.as_bytes())
        .map_err(SnapshotError::Io)?;
    file.sync_all().map_err(SnapshotError::Io)?;
    Ok(())
}

fn temp_path(path: &Path) -> PathBuf {
    let mut temp = path.to_path_buf();
    let suffix = format!(".tmp-{}", std::process::id());
    let mut filename = path.file_name().unwrap_or_default().to_os_string();
    filename.push(&suffix);
    temp.set_file_name(filename);
    temp
}

pub fn write_or_stdout(path: Option<&Path>, content: &str) -> Result<(), SnapshotError> {
    match path {
        Some(path) => write_atomic(path, content),
        None => {
            let mut stdout = io::stdout();
            stdout
                .write_all(content.as_bytes())
                .map_err(SnapshotError::Io)?;
            Ok(())
        }
    }
}
