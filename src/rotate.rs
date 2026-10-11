//! 运行日志文件的轮转：写满 [`RotationPolicy::max_bytes`] 就滚成 `path.1`（最新）…
//! `path.N`（最旧），并按 mtime 清超龄分卷。

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use crate::config::LogSection;

/// 两次「按时间清理」之间的最小间隔：避免每条日志都去 `stat` 各分卷。
const PRUNE_INTERVAL: Duration = Duration::from_secs(60);

/// 运行日志文件的轮转策略（由 `[log]` 的后三键决定）。
///
/// **仅 crate 内部使用**：使用者通过 `[log]` 段配置，无需也不应直接构造它。
#[derive(Debug, Clone, Copy)]
pub(crate) struct RotationPolicy {
    pub(crate) max_bytes: u64,
    pub(crate) keep_files: usize,
    pub(crate) max_seconds: i64,
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
    pub(crate) fn resolve(section: &LogSection) -> Self {
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
    last_prune: Instant,
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
            last_prune: Instant::now(),
        };
        writer.prune_by_age();
        Ok(writer)
    }

    /// 分卷路径：`<path>.<index>`。用 `OsString` 拼接，**不**走 `display()`
    /// （后者对非 UTF-8 路径是 lossy 的，会把分卷落到错的路径）。
    fn archive(&self, index: usize) -> PathBuf {
        let mut name = self.path.as_os_str().to_os_string();
        name.push(format!(".{index}"));
        PathBuf::from(name)
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
        let oldest = self.archive(keep);
        if let Err(err) = std::fs::remove_file(&oldest)
            && err.kind() != io::ErrorKind::NotFound
        {
            warn_rotate("删除最旧分卷", &oldest, &err);
        }
        for index in (1..keep).rev() {
            let from = self.archive(index);
            if from.exists() {
                let to = self.archive(index + 1);
                if let Err(err) = std::fs::rename(&from, &to) {
                    warn_rotate("下移分卷", &from, &err);
                }
            }
        }
        let current_archive = self.archive(1);
        if let Err(err) = std::fs::rename(&self.path, &current_archive) {
            warn_rotate("归档当前文件", &self.path, &err);
        }
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

    /// 长时间不轮转的进程也要清超龄分卷：在写入路径上**限频**地按时间清理。
    fn prune_if_due(&mut self) {
        if self.retention.max_seconds > 0 && self.last_prune.elapsed() >= PRUNE_INTERVAL {
            self.prune_by_age();
            self.last_prune = Instant::now();
        }
    }
}

/// 轮转中的非致命失败。
///
/// **必须直写 stderr，不能走 `log::*`**：这里是从 [`SharedWriter::write`] 里被调用的，
/// 此刻已持有写锁；再经 `env_logger` 回调进来会重入同一把 `std::sync::Mutex`（不可重入）→ 死锁。
fn warn_rotate(action: &str, path: &Path, err: &io::Error) {
    eprintln!(
        "[warn] 日志轮转：{action} {} 失败：{err}（继续写当前文件，请检查磁盘/权限）",
        path.display()
    );
}

impl Write for RotatingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // 单条记录**不拆分**：一条比 `max_bytes` 还大的记录会独占一个（因而略超上限的）文件
        // —— 拆行会破坏日志语义。`written > 0` 保证空文件不反复轮转（否则上限小于一条记录时会转不停）。
        if self.written > 0 && self.written + buf.len() as u64 > self.retention.max_bytes {
            self.rotate()?;
        }
        let written = self.file.write(buf)?;
        self.written += written as u64;
        self.prune_if_due();
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
    fn open_creates_missing_parent_directories() {
        let dir = temp_dir("mkdir");
        let path = dir.join("nested/deeper/app.log");
        let policy = RotationPolicy {
            max_bytes: 64 * 1024,
            keep_files: 1,
            max_seconds: 0,
        };
        let mut writer = RotatingFile::open(&path, policy).expect("open");
        writer.write_all(b"line\n").expect("write");
        let _ = writer.flush();

        assert!(path.exists(), "父目录应当被自动创建并落文件");
        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn keep_files_zero_truncates_instead_of_archiving() {
        let dir = temp_dir("keep0");
        let path = dir.join("app.log");
        let policy = RotationPolicy {
            max_bytes: 8 * 1024,
            keep_files: 0,
            max_seconds: 0,
        };
        let mut writer = RotatingFile::open(&path, policy).expect("open");
        for _ in 0..4 {
            writer.write_all(&vec![b'x'; 4 * 1024]).expect("write");
        }
        let _ = writer.flush();
        // 不留历史：没有 .1，当前文件被截断到最后一轮之内。
        assert!(!dir.join("app.log.1").exists(), "keep_files=0 不留分卷");
        let size = std::fs::metadata(&path).expect("meta").len();
        assert!(size <= 8 * 1024, "轮转即截断，当前文件不应累积");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_single_oversized_record_is_not_split_but_costs_its_own_file() {
        let dir = temp_dir("oversized");
        let path = dir.join("app.log");
        let policy = RotationPolicy {
            max_bytes: 8,
            keep_files: 2,
            max_seconds: 0,
        };
        let mut writer = RotatingFile::open(&path, policy).expect("open");
        // 空文件上的第一条就超限：`written == 0` 守卫使其不轮转（否则会转不停）。
        writer.write_all(b"0123456789ab").expect("write");
        assert!(!dir.join("app.log.1").exists(), "空文件不轮转");
        assert_eq!(std::fs::metadata(&path).expect("meta").len(), 12);
        // 再来一条：此时已有内容，轮转后新文件仍是“单条超限”。
        writer.write_all(b"0123456789ab").expect("write");
        let _ = writer.flush();
        assert!(dir.join("app.log.1").exists(), "非空文件超限会轮转");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prunes_archives_older_than_max_seconds_on_open() {
        let dir = temp_dir("prune");
        let path = dir.join("app.log");
        // 造一个很旧的分卷。
        let stale = dir.join("app.log.1");
        std::fs::write(&stale, b"old\n").expect("write stale");
        let old = SystemTime::now() - Duration::from_secs(10 * 24 * 60 * 60);
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .expect("open stale")
            .set_modified(old)
            .expect("set mtime");

        let policy = RotationPolicy {
            max_bytes: 64 * 1024,
            keep_files: 4,
            max_seconds: 7 * 24 * 60 * 60,
        };
        let _writer = RotatingFile::open(&path, policy).expect("open");

        assert!(!stale.exists(), "超龄分卷应在 open 时被清掉");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
