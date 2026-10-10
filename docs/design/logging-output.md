# 统一日志输出设计（wist）

本文件是 **wist 各组件运行日志的输出契约**。实现落在 crate `wist-log`（本仓），
gateway / gwlinkd / center / agentd / exec / upgrader 都走它 —— 让"想不一致都难"。

---

## 0. 原则

- **事件流**：程序只往 `log` 门面写；**级别 / 格式 / 落点由单一 `init` 按配置决定**。
- **一致**：所有组件用**同一套** `[log]` 键、同一套字段名、同一套级别语义。
- **稀疏**：只记**生命周期 + 边界投影**，不逐层 `error!` / `info!`。
- **默认交给上层**：默认写 **stderr**，由 systemd/journald 或容器 runtime 收集；组件**不自己 daemonize、不自己管文件**。

---

## 1. 配置契约：`[log]`（所有组件**同名同义**）

| 键 | 值 | 缺省 | 说明 |
|---|---|---|---|
| `level` | `info` / `wist_gateway=debug,hyper=warn` | `info` | 阈值，可分 target |
| `format` | `text` / `json` | `text` | |
| `file` | 路径 | **空 = stderr** | 给了才写文件 |
| `max_bytes` | 字节（下限 8 KiB） | 64 MiB | 写满轮转成 `.1`/`.2`/… |
| `keep_files` | 个数（0 = 不留） | 4 | 保留分卷数 |
| `max_seconds` | 秒（0 = 不按时间清） | 7 天 | 分卷保留时长 |

- **优先级（全栈一致）：`RUST_LOG` > `[log].level` > `info`。**
- 键名规范为 `max_seconds`；旧名 `max_age_seconds` 以 `alias` 继续接受（平滑迁移）。

---

## 2. 输出契约

- **text**（人看）：`<ts RFC3339ms> <LEVEL> <target>: <message>`
- **json**（机器看）：每行一个对象，固定字段：

| 字段 | 必有 | 说明 |
|---|---|---|
| `ts` | ✓ | RFC3339 毫秒 |
| `level` | ✓ | 小写 `info` / `warn` / …（**采集端按它筛**）|
| `target` | ✓ | Rust 模块路径 |
| `component` | ✓ | `wist-gateway` / `wist-center` / `wist-agentd` … |
| `version` | ✓ | 组件版本 |
| `msg` | ✓ | 人读信息 |
| `event` / 业务字段 | 选 | 稳定事件名 + 扁平 key（`gateway_id`…）|

> **plain text 到 stderr，收集端看不懂你的级别**。要"落地后还能按级别/字段筛"，就用 `format = "json"`。

---

## 3. 级别语义（统一）

| 级别 | 何时用 |
|---|---|
| `error` | 操作失败、需人处理 |
| `warn` | 降级 / 意外但已处理（含鉴权失败 401/429）|
| `info` | **生命周期**（启动 / 就绪 / 停止 / 配置加载）+ 低频操作成功 |
| `debug` | 排查细节 |
| `trace` | 极细 |

---

## 4. 落点契约

| 运行形态 | `file` | 轮转 |
|---|---|---|
| **容器**（gateway / center 发布态） | **留空 → stderr** | 交 runtime（docker `log-opts max-size`/`max-file`、journald）|
| **宿主 / 边缘**（dev、agentd 离线） | 可给 | **进程内**（`max_bytes` / `keep_files` / `max_seconds`）+ **别叠 logrotate** |
| 通用 | 日志**一律 stderr**；stdout 只留给程序输出（如 `version`）| |

> 容器里给了 `file` ⇒ `docker logs` 看不到该组件自身日志。**要落盘留存再给**，且路径必须在**挂载卷**上。
> 二者互斥，**不要按级别把 stdout/stderr 分流**（会把一个日志流裂成两条）。

---

## 5. 生命周期日志契约（orion-error 0.9）

低频操作（upgrade / enroll / credential renew / daemon 起停）用：

```rust
let mut guard = OperationContext::doing("renew").with_mod_path(module_path!()).with_auto_log();
let mut scope = guard.scope();      // 默认失败
let value = fetch().await?;         // `?` 早退 → 自动记 fail!
scope.mark_success();               // 成功才 → suc!
```

- **默认失败、成功 `mark_success()`**；**用 `scope()`，不要 `scoped_success()`**（后者默认成功，`?` 早退会误记 `suc!`）。
- 把操作附到错误用 **`err.with_context(&guard)`**（借用，只拷数据、不再记一条）。
- **只包低频操作**；周期 tick 不包（会刷屏）。

---

## 6. 安全

- **脱敏**：token / 口令 / 证书 / URL 查询串一律不落地。
- 不靠 plain text 让收集端猜级别 —— 要筛就 `format = "json"` 带 `level`。

---

## 7. 落地

1. 共享实现 = 本 crate `wist-log`（`LogSection` + `init` + json 格式 + 轮转）。
2. center / gateway 的**各自一份** `logging.rs` → 改用 `wist-log`（迁配置键 `max_age_seconds` → `max_seconds`，旧名有 alias）。
3. agentd / gwlinkd 接入；exec / upgrader 的 CLI 输出（`version` 等）**保留 stdout**，不进日志。
