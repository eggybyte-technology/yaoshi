use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::{YaoshiError, YaoshiResult};

pub fn ensure_dir(path: &Path, phase: &'static str) -> YaoshiResult<()> {
    if path.exists() {
        if path.is_dir() {
            return Ok(());
        }
        return Err(error_for_phase(
            phase,
            format!(
                "expected directory path is not a directory: {}",
                path.display()
            ),
        ));
    }
    fs::create_dir_all(path)
        .map_err(|e| error_for_phase(phase, format!("create directory {}: {e}", path.display())))
}

pub fn write_if_changed(path: &Path, bytes: &[u8]) -> YaoshiResult<bool> {
    if path.exists() {
        if !path.is_file() {
            return Err(YaoshiError::build(format!(
                "cache file exists and is not a regular file: {}",
                path.display()
            )));
        }
        let mut existing = Vec::new();
        File::open(path)
            .and_then(|mut f| f.read_to_end(&mut existing))
            .map_err(|e| YaoshiError::build(format!("read cache file {}: {e}", path.display())))?;
        if existing == bytes {
            return Ok(false);
        }
    }

    let parent = path.parent().ok_or_else(|| {
        YaoshiError::build(format!("cache path has no parent: {}", path.display()))
    })?;
    ensure_dir(parent, "build")?;
    let tmp = temp_sibling(path);
    {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)
            .map_err(|e| YaoshiError::build(format!("create temp cache file: {e}")))?;
        file.write_all(bytes)
            .map_err(|e| YaoshiError::build(format!("write temp cache file: {e}")))?;
        file.sync_all()
            .map_err(|e| YaoshiError::build(format!("fsync temp cache file: {e}")))?;
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        YaoshiError::build(format!("replace cache file {}: {e}", path.display()))
    })?;
    fsync_dir(parent)?;
    Ok(true)
}

pub fn fsync_dir(path: &Path) -> YaoshiResult<()> {
    File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| YaoshiError::build(format!("fsync directory {}: {e}", path.display())))
}

fn temp_sibling(path: &Path) -> PathBuf {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("tmp");
    path.with_file_name(format!(
        ".{name}.tmp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}

fn error_for_phase(phase: &'static str, message: String) -> YaoshiError {
    match phase {
        "environment" => YaoshiError::environment(message),
        "image" => YaoshiError::image(message),
        "publish" => YaoshiError::publish(message),
        "usage" => YaoshiError::usage(message),
        "config" => YaoshiError::config(message),
        _ => YaoshiError::build(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    #[test]
    fn write_if_changed_reports_create_reuse_and_replace() {
        let dir = tempdir("yaoshi-write-if-changed");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.bin");
        assert!(write_if_changed(&path, b"one").unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"one");
        assert!(!write_if_changed(&path, b"one").unwrap());
        assert!(write_if_changed(&path, b"two").unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"two");
        let _ = fs::remove_dir_all(dir);
    }
}
