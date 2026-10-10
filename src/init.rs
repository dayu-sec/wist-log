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
            builder.format_timestamp_millis();
        }
        LogFormat::Json => {
            let component: &'static str = identity.component;
            let version: &'static str = identity.version;
            builder.format(move |buf, record| {
                let line = serde_json::json!({
                    "ts": buf.timestamp().to_string(),
                    "level": record.level().as_str().to_ascii_lowercase(),
                    "target": record.target(),
                    "component": component,
                    "version": version,
                    "msg": record.args().to_string(),
                });
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

/// 解析生效的过滤器：`RUST_LOG` > `[log] level` > `info`。
fn resolve_filter(section: &LogSection) -> String {
    std::env::var("RUST_LOG")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_prefers_rust_log_then_section_then_info() {
        // 不碰进程级 RUST_LOG（测试并行），只测「无环境变量时的回落」。
        if std::env::var("RUST_LOG").is_err() {
            let level = LogSection {
                level: Some("warn".to_string()),
                ..Default::default()
            };
            assert_eq!(resolve_filter(&level), "warn");

            let empty = LogSection {
                level: Some("  ".to_string()),
                ..Default::default()
            };
            assert_eq!(resolve_filter(&empty), "info");

            assert_eq!(resolve_filter(&LogSection::default()), "info");
        }
    }

    #[test]
    fn identity_keeps_component_and_version() {
        let identity = Identity::new("wist-gateway", "0.7.0");
        assert_eq!(identity.component, "wist-gateway");
        assert_eq!(identity.version, "0.7.0");
    }
}
