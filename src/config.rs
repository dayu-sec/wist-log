//! `[log]` 配置段：所有 wist 组件**同名同义**。

use std::path::PathBuf;

/// 运行日志的 `[log]` 段（级别 / 格式 / 落点 / 轮转）。
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct LogSection {
    /// 过滤器指令：`info`，或 `wist_gateway=debug,hyper=warn` 这种分 target 写法。缺省 `info`。
    #[serde(default)]
    pub level: Option<String>,
    /// 输出格式（`text` | `json`）。缺省 `text`。
    #[serde(default)]
    pub format: LogFormat,
    /// 写文件（追加、自动建父目录、写满轮转）。缺省写 stderr。
    #[serde(default)]
    pub file: Option<PathBuf>,
    /// 单文件上限（字节），写满就轮转成 `file.1`/`file.2`/…。缺省 64 MiB（下限 8 KiB）。
    #[serde(default)]
    pub max_bytes: Option<u64>,
    /// 保留的历史分卷个数（`0` = 不留历史，轮转即截断）。缺省 4。
    #[serde(default)]
    pub keep_files: Option<usize>,
    /// 历史分卷保留时长（秒）；`0` = 不按时间清。缺省 7 天。
    ///
    /// 旧名 `max_age_seconds` 仍接受（`alias`），便于既有配置平滑迁移。
    #[serde(default, alias = "max_age_seconds")]
    pub max_seconds: Option<i64>,
}

/// 日志输出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// `<时间戳> <LEVEL> <target>: <message>`（缺省）。
    #[default]
    Text,
    /// 每行一个 JSON 对象：`{ts, level, target, component, version, msg}`。
    Json,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_full_section_with_max_seconds() {
        let section: LogSection = toml::from_str(
            "level = \"wist_gateway=debug\"\nformat = \"json\"\nfile = \"/tmp/g.log\"\n\
             max_bytes = 1048576\nkeep_files = 3\nmax_seconds = 86400\n",
        )
        .expect("parse");
        assert_eq!(section.level.as_deref(), Some("wist_gateway=debug"));
        assert_eq!(section.format, LogFormat::Json);
        assert_eq!(
            section.file.as_deref(),
            Some(std::path::Path::new("/tmp/g.log"))
        );
        assert_eq!(section.max_bytes, Some(1_048_576));
        assert_eq!(section.keep_files, Some(3));
        assert_eq!(section.max_seconds, Some(86_400));
    }

    #[test]
    fn accepts_the_legacy_max_age_seconds_alias() {
        // 老配置写 `max_age_seconds` 仍能解析到同一字段（平滑迁移）。
        let section: LogSection = toml::from_str("max_age_seconds = 120\n").expect("parse");
        assert_eq!(section.max_seconds, Some(120));
    }

    #[test]
    fn defaults_are_text_with_no_file_and_no_overrides() {
        let section = LogSection::default();
        assert_eq!(section.format, LogFormat::Text);
        assert!(section.file.is_none());
        assert!(section.level.is_none());
        assert!(section.max_bytes.is_none());
        assert!(section.keep_files.is_none());
        assert!(section.max_seconds.is_none());
    }
}
