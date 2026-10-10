//! 运行日志文件的轮转：写满 [`RotationPolicy::max_bytes`] 就滚成 `path.1`（最新）…
//! `path.N`（最旧），并按 mtime 清超龄分卷。

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use crate::config::LogSection;

/// 运行日志文件的轮转策略（由 `[log]` 的后三键决定）。
#[derive(Debug, Clone, Copy)]
pub struct RotationPolicy {
    pub max_bytes: u64,
    pub keep_files: usize,
    pub max_seconds: i64,
}

impl Default for RotationPolicy {
    fn default() -> Self {
        Self {
            max_bytes: 64 * 1024 * 1024,
            keep_files: 4,
            max_seconds: 7 * 24 * 60 * 60,
        }
    }
}

impl RotationPolicy {
    /// 由 `[log]` 段解析策略：缺省回落到 [`RotationPolicy::default`]，`max_bytes` 有 8 KiB 下限。
    pub fn resolve(section: &LogSection) -> Self {
        let default = Self::default();
        Self {
            max_bytes: section.max_bytes.unwrap_or(default.max_bytes).max(8 * 1024),
            keep_files: section.keep_files.unwrap_or(default.keep_files),
            max_seconds: section.max_seconds.unwrap_or(default.max_seconds),
        }
    }
}

struct RotatingFile {
    path: PathBuf,
    retention: RotationPolicy,
    file: std::fs::File,
    written: u64,
}

impl RotatingFile {
    fn open(path: &Path, retention: RotationPolicy) -> io::Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        let written = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        let writer = Self {
            path: path.to_path_buf(),
            retention,
            file,
            written,
        };
        writer.prune_by_age();
        Ok(writer)
    }

    fn archive(&self, index: usize) -> PathBuf {
        PathBuf::from(format!("{}.{}", self.path.display(), index))
    }

    fn rotate(&mut self) -> io::Result<()> {
        let _ = self.file.flush();
        // `keep_files == 0`：不留历史，截断当前文件即可。
        if self.retention.keep_files == 0 {
            self.file = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&self.path)?;
            self.written = 0;
            return Ok(());
        }
        let keep = self.retention.keep_files;
        // 丢最旧，再逐级下移（从旧的往里挪，避免互相覆盖）。
        let _ = std::fs::remove_file(self.archive(keep));
        for index in (1..keep).rev() {
            let from = self.archive(index);
            if from.exists() {
                let _ = std::fs::rename(&from, self.archive(index + 1));
            }
        }
        let _ = std::fs::rename(&self.path, self.archive(1));
        self.file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.written = 0;
        self.prune_by_age();
        Ok(())
    }

    fn prune_by_age(&self) {
        if self.retention.max_seconds <= 0 {
            return;
        }
        let cutoff = SystemTime::now() - Duration::from_secs(self.retention.max_seconds as u64);
        for index in 1..=self.retention.keep_files.max(1) {
            let path = self.archive(index);
            let stale = std::fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .map(|modified| modified < cutoff)
                .unwrap_or(false);
            if stale {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

impl Write for RotatingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // `written > 0`：空文件不反复轮转（否则上限小于一行时会转不停）。
        if self.written > 0 && self.written + buf.len() as u64 > self.retention.max_bytes {
            self.rotate()?;
        }
        let written = self.file.write(buf)?;
        self.written += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// `Target::Pipe` 收 `Box<dyn Write + Send>`；用 `Arc<Mutex<..>>` 包一层保证跨线程安全。
#[derive(Clone)]
pub(crate) struct SharedWriter(Arc<Mutex<RotatingFile>>);

impl SharedWriter {
    fn new(file: RotatingFile) -> Self {
        Self(Arc::new(Mutex::new(file)))
    }
}

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .flush()
    }
}

/// 打开一个写文件的 [`SharedWriter`]（供 [`crate::init`] 用）。
pub(crate) fn open_rotating_writer(
    path: &Path,
    policy: RotationPolicy,
) -> io::Result<SharedWriter> {
    RotatingFile::open(path, policy).map(SharedWriter::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "wist-log-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn resolve_applies_defaults_and_8kib_floor() {
        let mut section = LogSection::default();
        let resolved = RotationPolicy::resolve(&section);
        assert_eq!(resolved.max_bytes, 64 * 1024 * 1024);
        assert_eq!(resolved.keep_files, 4);
        assert_eq!(resolved.max_seconds, 7 * 24 * 60 * 60);

        // 给了过小的 max_bytes → 抬到 8 KiB 下限。
        section.max_bytes = Some(16);
        assert_eq!(RotationPolicy::resolve(&section).max_bytes, 8 * 1024);
    }

    #[test]
    fn rotates_when_exceeding_max_bytes_and_keeps_history() {
        let dir = temp_dir("rotate");
        let path = dir.join("app.log");
        let policy = RotationPolicy {
            max_bytes: 8 * 1024,
            keep_files: 2,
            max_seconds: 0,
        };
        let mut writer = RotatingFile::open(&path, policy).expect("open");
        for _ in 0..5 {
            writer.write_all(&vec![b'x'; 4 * 1024]).expect("write");
        }
        let _ = writer.flush();
        assert!(path.exists(), "current file exists");
        assert!(dir.join("app.log.1").exists(), "one archive kept");
        assert!(
            !dir.join("app.log.3").exists(),
            "beyond keep_files not kept"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
