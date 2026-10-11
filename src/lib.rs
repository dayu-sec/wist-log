//! wist 统一日志输出。
//!
//! 所有 wist 组件的运行日志走**同一套**口径：程序只往 `log` 门面写，级别 / 格式 / 落点由
//! [`LogSection`]（配置的 `[log]` 段）经 [`init`] 一次性决定。契约见
//! `docs/design/logging-output.md`（设计文档随本 crate）。
//!
//! - **级别**（`level`）：`RUST_LOG` 环境变量 > `[log].level` > `info`；支持分 target
//!   （`wist_gateway=debug,hyper=warn`）。
//! - **格式**（`format`）：`text`（人看，`<ts> <LEVEL> <target>: <msg>`）/ `json`（机器看，每行
//!   `{ts, level, target, component, version, msg}`，`ts` 为 RFC3339 毫秒，`level` 小写供采集端筛）。
//! - **落点**（`file`）：给了写文件（追加、自动建父目录、**写满轮转**），缺省写 stderr —— 容器里**不要**
//!   给 `file`，让容器 runtime 收 stderr 并轮转。
//! - **轮转**：进程内做（`max_bytes` / `keep_files` / `max_seconds`）；自己轮转就**别再挂 logrotate**。
//!
//! ```no_run
//! let section = wist_log::LogSection::default(); // 实际来自配置的 [log] 段
//! wist_log::init(
//!     &section,
//!     &wist_log::Identity::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
//! );
//! ```

mod config;
mod init;
mod rotate;

pub use config::{LogFormat, LogSection};
pub use init::{Identity, init};
