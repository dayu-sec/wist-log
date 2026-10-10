# wist-log

wist 统一日志输出：`log` 门面 + `env_logger` 后端，级别 / 格式 / 落点 / 轮转由配置的 `[log]` 段决定。

**目标**：让 gateway / gwlinkd / center / agentd / exec / upgrader 用**同一套**日志配置键、同一套输出格式、同一套级别语义 —— 想不一致都难。

## 契约

`[log]` 段（所有组件同名同义）：

| 键 | 值 | 缺省 |
|---|---|---|
| `level` | `info` / `wist_gateway=debug,hyper=warn` | `info` |
| `format` | `text` / `json` | `text` |
| `file` | 路径（空 = stderr） | 空 → stderr |
| `max_bytes` | 字节（写满轮转，下限 8 KiB） | 64 MiB |
| `keep_files` | 保留分卷数（0 = 不留） | 4 |
| `max_seconds` | 分卷保留秒数（0 = 不按时间清） | 7 天 |

- **优先级**：`RUST_LOG` > `[log].level` > `info`。
- **格式**：`text` = `<ts> <LEVEL> <target>: <msg>`；`json` = 每行 `{ts, level, target, component, version, msg}`（采集端按 `level` 筛）。
- **落点**：给了 `file` 才写文件（追加、自动建父目录、**进程内轮转**，别再挂 logrotate）；否则写 stderr。
- **容器里不要给 `file`**：让容器 runtime 收 stderr 并轮转（`docker logging` / journald）。

## 用法

```toml
# 配置里的 [log] 段
[log]
level  = "info"
format = "json"
# file 留空 → stderr
```

```rust
let section = /* 从配置反序列化 */ wist_log::LogSection::default();
wist_log::init(
    &section,
    &wist_log::Identity::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
);
```

之后业务代码只 `log::info!` / `log::warn!`；**生命周期日志**（upgrade / enroll / 起停等低频操作）用
orion-error 的 `OperationContext` + `with_auto_log()`（`AutoLogGuard`，默认失败、成功 `mark_success()`）。

## 设计文档

`docs/design/logging-output.md`（随本 crate）。
