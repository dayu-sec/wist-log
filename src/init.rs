//! 初始化：按 `[log]` 段装配 `env_logger`（级别 / 格式 / 落点）。

use std::io::Write;

use env_logger::{Builder, Target};

use crate::config::{LogFormat, LogSection};
use crate::rotate::{RotationPolicy, open_rotating_writer};

/// 日志里标注的组件身份（`format = "json"` 时进每行）。
#[derive(Debug, Clone, Copy)]
pub struct Identity {
    pub component: &'static str,
    pub version: &'static str,
}

impl Identity {
    pub const fn new(component: &'static str, version: &'static str) -> Self {
        Self { component, version }
    }
}

/// 按 `[log]` 段初始化日志。**只生效一次**（`try_init`）；已初始化过不 panic。
pub fn init(section: &LogSection, identity: &Identity) {
    let mut builder = Builder::new();
    builder.parse_filters(&resolve_filter(section));
    if !rust_log_is_set() && !section.level_looks_valid() {
        // env_logger 对无法识别的指令**静默忽略**，结果是"配了却一条日志都没有"。
        // 只提醒一声，不改变行为（RUST_LOG 仍可覆盖）。
        eprintln!(
            "[warn] [log].level = {:?} 看起来不是合法的过滤器指令（形如 `info` 或 \
             `wist_gateway=debug,hyper=warn`）；env_logger 会忽略它，可能一条日志都不出",
            section.level
        );
    }

    match section.file.as_deref() {
        Some(path) => match open_rotating_writer(path, RotationPolicy::resolve(section)) {
            Ok(writer) => {
                builder.target(Target::Pipe(Box::new(writer)));
            }
            Err(err) => {
                // 落文件失败不该让组件起不来：说清一句、回落 stderr（那里通常有 journald 接着）。
                eprintln!(
                    "[warn] 打开日志文件 {} 失败：{err}（回落 stderr）",
                    path.display()
                );
                builder.target(Target::Stderr);
            }
        },
        None => {
            builder.target(Target::Stderr);
        }
    }

    match section.format {
        LogFormat::Text => {
            // 契约（设计文档 §2）：`<ts RFC3339ms> <LEVEL> <target>: <message>`。
            builder.format(|buf, record| {
                writeln!(
                    buf,
                    "{} {} {}: {}",
                    buf.timestamp_millis(),
                    record.level(),
                    record.target(),
                    record.args()
                )
            });
        }
        LogFormat::Json => {
            let component: &'static str = identity.component;
            let version: &'static str = identity.version;
            builder.format(move |buf, record| {
                let line = json_line(
                    buf.timestamp_millis().to_string(),
                    record.level().as_str(),
                    record.target(),
                    component,
                    version,
                    &record.args().to_string(),
                );
                writeln!(buf, "{line}")
            });
        }
    }

    if let Err(err) = builder.try_init() {
        // 只可能失败于「已有 logger 初始化过」（如测试 harness 或宿主先装了全局 logger）。
        // 说出来比静默好：否则使用者会以为 `[log]` 生效了，却看不到日志去了文件。
        eprintln!("[warn] 日志已被其它组件初始化，[log] 配置未生效（原因：{err}）");
    }
}

/// 单行 JSON 日志（`format = "json"` 的字段口径，见设计文档 §2）。
///
/// `level` 统一小写；`ts` 为 RFC3339 **毫秒**；`component` / `version` 由 [`Identity`] 提供。
fn json_line(
    ts: String,
    level: &str,
    target: &str,
    component: &str,
    version: &str,
    msg: &str,
) -> serde_json::Value {
    serde_json::json!({
        "ts": ts,
        "level": level.to_ascii_lowercase(),
        "target": target,
        "component": component,
        "version": version,
        "msg": msg,
    })
}

/// 解析生效的过滤器：`RUST_LOG` > `[log] level` > `info`。
fn resolve_filter(section: &LogSection) -> String {
    resolve_filter_from(std::env::var("RUST_LOG").ok().as_deref(), section)
}

fn resolve_filter_from(rust_log: Option<&str>, section: &LogSection) -> String {
    rust_log
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            section
                .level
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "info".to_string())
}

fn rust_log_is_set() -> bool {
    std::env::var("RUST_LOG")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_prefers_rust_log_then_section_then_info() {
        let section = LogSection {
            level: Some("warn".to_string()),
            ..Default::default()
        };
        // RUST_LOG 优先。
        assert_eq!(resolve_filter_from(Some("debug"), &section), "debug");
        // 环境变量为空（或只有空白）时回落到 [log].level。
        assert_eq!(resolve_filter_from(Some("  "), &section), "warn");
        assert_eq!(resolve_filter_from(None, &section), "warn");
        // 两者都没有 → info。
        assert_eq!(resolve_filter_from(None, &LogSection::default()), "info");
    }

    #[test]
    fn identity_keeps_component_and_version() {
        let identity = Identity::new("wist-gateway", "0.7.0");
        assert_eq!(identity.component, "wist-gateway");
        assert_eq!(identity.version, "0.7.0");
    }

    #[test]
    fn json_line_carries_the_documented_fields_with_a_lowercase_level() {
        let value = json_line(
            "2026-10-11T12:34:56.789Z".to_string(),
            "INFO",
            "wist_agentd::runtime",
            "wist-agentd",
            "0.3.0",
            "started",
        );
        assert_eq!(value["ts"], "2026-10-11T12:34:56.789Z");
        assert_eq!(value["level"], "info");
        assert_eq!(value["target"], "wist_agentd::runtime");
        assert_eq!(value["component"], "wist-agentd");
        assert_eq!(value["version"], "0.3.0");
        assert_eq!(value["msg"], "started");
    }
}
