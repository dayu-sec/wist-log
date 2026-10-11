# 更新日志

## [0.1.0] - 2026-10-11

首个版本：把 center / gateway 里**各自一份**的 `logging.rs` 抽成共享 crate，统一 wist 各组件运行日志的输出。

### 新增

- `LogSection`（`[log]` 段）：`level` / `format` / `file` / `max_bytes` / `keep_files` / `max_seconds`。
  - `max_seconds` 为规范键名；旧名 `max_age_seconds` 以 `alias` 继续接受（平滑迁移）。
- `LogFormat`：`text`（人看）/ `json`（机器看，每行带 `level` 字段）。
- `init(&LogSection, &Identity)`：`RUST_LOG` > `[log].level` > `info`；落文件写满**进程内轮转**，缺省 stderr；`try_init` 只生效一次。
- `Identity`：`component` / `version`，进 `json` 每行。
- `RotationPolicy`：`max_bytes`（下限 8 KiB）/ `keep_files` / `max_seconds`。
