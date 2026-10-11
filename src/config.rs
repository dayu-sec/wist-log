//! `[log]` 配置段：所有 wist 组件**同名同义**。

use std::path::PathBuf;

/// 运行日志的 `[log]` 段（级别 / 格式 / 落点 / 轮转）。
///
/// 这是**唯一事实来源**：各组件把它**直接内嵌**进自己的配置结构（如 `wist-gateway`
/// `AdminConfig.log`、`wist-center` `CenterConfig.log`、`wist-agentd` `AgentConfig.log`）。
/// 因此 derive 了 `Serialize` / `PartialEq` / `Eq`，以便内嵌方也能 derive 这些。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// `<时间戳> <LEVEL> <target>: <message>`（缺省）。
    #[default]
    Text,
    /// 每行一个 JSON 对象：`{ts, level, target, component, version, msg}`。
    Json,
}

impl LogSection {
    /// `level` 是否像合法的过滤器指令。
    ///
    /// 只用于在 `init` 时对**明显拼错**的 `level` 说一声：`env_logger` 对无法识别的指令
    /// 会静默忽略，后果是“配了却一条日志都没有”，极难排查。合法形如 `info`，
    /// 或分 target 的 `wist_gateway=debug,hyper=warn`（逗号分隔）。
    pub(crate) fn level_looks_valid(&self) -> bool {
        let Some(level) = self.level.as_deref() else {
            return true;
        };
        let mut any = false;
        for directive in level.split(',') {
            let directive = directive.trim();
            if directive.is_empty() {
                continue;
            }
            any = true;
            let (target, threshold) = match directive.rsplit_once('=') {
                Some((target, threshold)) => (Some(target.trim()), threshold.trim()),
                None => (None, directive),
            };
            if let Some(target) = target
                && target.is_empty()
            {
                return false;
            }
            if !matches!(
                threshold.to_ascii_lowercase().as_str(),
                "off" | "error" | "warn" | "info" | "debug" | "trace"
            ) {
                return false;
            }
        }
        any
    }
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

    #[test]
    fn level_validity_flags_typos_and_accepts_the_documented_forms() {
        let valid = [
            None,
            Some("info"),
            Some("OFF"),
            Some("wist_gateway=debug,hyper=warn"),
        ];
        for level in valid {
            let section = LogSection {
                level: level.map(str::to_string),
                ..Default::default()
            };
            assert!(section.level_looks_valid(), "{level:?} 应当合法");
        }

        let invalid = [
            Some(""),
            Some("   "),
            Some("inof"),
            Some("=debug"),
            Some("app="),
        ];
        for level in invalid {
            let section = LogSection {
                level: level.map(str::to_string),
                ..Default::default()
            };
            assert!(!section.level_looks_valid(), "{level:?} 应当被判为可疑");
        }
    }

    #[test]
    fn the_section_round_trips_through_serde() {
        // 内嵌方（gateway / center / agentd）需要能序列化它：钉住这个能力。
        let section = LogSection {
            level: Some("info".to_string()),
            format: LogFormat::Json,
            file: Some(std::path::PathBuf::from("/var/log/app.log")),
            max_bytes: Some(1024),
            keep_files: Some(2),
            max_seconds: Some(3600),
        };
        let text = serde_json::to_string(&section).expect("serialize");
        let back: LogSection = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, section);
    }
}
