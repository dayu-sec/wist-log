# 更新日志

## [0.1.1] - 2026-10-11

对齐 `text`/`json` 输出契约、修轮转健壮性，并把 `LogSection` 做成可**直接内嵌**的单类型。

### 修复

- `text` 此前落到 `env_logger` 默认布局（`[ts LEVEL target] msg`），与设计文档 §2 不符；现按契约输出 `<ts RFC3339ms> <LEVEL> <target>: <msg>`。
- `json` 的 `ts` 此前是**秒**精度（`buf.timestamp()`），契约写的是毫秒；改用 `timestamp_millis()`。
- 轮转时 `remove_file` / `rename` 失败此前被静默吞掉，可能「上限失效却毫无信号」；现直写 stderr 报告（**不**走 `log::*` —— 会重入写锁死锁）。
- `level` 明显拼错时在 `init` 发告警（`env_logger` 对未知指令静默忽略，否则会「配了却无日志」）。
- 分卷路径改由 `OsString` 拼接，非 UTF-8 路径不再 lossy。

### 变更

- `LogSection` / `LogFormat` 增加 `Serialize`（`LogSection` 另加 `PartialEq` / `Eq`）：组件可直接内嵌 `wist_log::LogSection`，无需再写镜像类型。
- `RotationPolicy` 不再对外导出（仅 crate 内部使用）。
- 长时间不轮转的进程也会按 `max_seconds` **限频**（60s）清理超龄分卷。

### 其他

- dev-dep `toml` 与生态对齐到 `1.1`；README 补徽章；补 `json` 行口径 / `keep_files=0` / 按时间清 / 自动建父目录 / 单条超限记录 等测试。

## [0.1.0] - 2026-10-11

首个版本：把 center / gateway 里**各自一份**的 `logging.rs` 抽成共享 crate，统一 wist 各组件运行日志的输出。

### 新增

- `LogSection`（`[log]` 段）：`level` / `format` / `file` / `max_bytes` / `keep_files` / `max_seconds`。
  - `max_seconds` 为规范键名；旧名 `max_age_seconds` 以 `alias` 继续接受（平滑迁移）。
- `LogFormat`：`text`（人看）/ `json`（机器看，每行带 `level` 字段）。
- `init(&LogSection, &Identity)`：`RUST_LOG` > `[log].level` > `info`；落文件写满**进程内轮转**，缺省 stderr；`try_init` 只生效一次。
- `Identity`：`component` / `version`，进 `json` 每行。
- `RotationPolicy`：`max_bytes`（下限 8 KiB）/ `keep_files` / `max_seconds`。
