//! 命令注册面（设计 phase-3.5/02 §6）：内建 + 技能 + 扩展合并为只读清单。

use crate::prompt::Skill;

/// 命令来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSource {
    /// 内建命令。
    Builtin,
    /// 技能命令（`skill:<name>`）。
    Skill,
    /// 扩展注册的命令（Phase 2，`plugin_<id>_*`）。
    Extension,
}

/// 命令可用状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStatus {
    /// 可执行。
    Ready,
    /// 有明确批次，尚未实现。
    Planned(&'static str),
    /// 本阶段范围外。
    Unsupported,
}

/// 一条命令（CLI 补全与桌面命令面板共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCommand {
    /// 命令名（不含 `/`）。
    pub name: String,
    /// 一行说明。
    pub description: String,
    /// 参数提示。
    pub argument_hint: Option<String>,
    /// 来源。
    pub source: CommandSource,
    /// 状态。
    pub status: CommandStatus,
}

const BUILTIN: [(&str, &str, Option<&str>, CommandStatus); 25] = [
    ("new", "Start a new session", None, CommandStatus::Ready),
    (
        "resume",
        "Resume a session, or list sessions",
        Some("[id]"),
        CommandStatus::Ready,
    ),
    (
        "session",
        "Show session id, model and message count",
        None,
        CommandStatus::Ready,
    ),
    (
        "reload",
        "Reload context files and skills",
        None,
        CommandStatus::Ready,
    ),
    ("quit", "Quit the CLI", None, CommandStatus::Ready),
    (
        "abort",
        "Cancel the current run",
        None,
        CommandStatus::Ready,
    ),
    (
        "ext",
        "Show extension diagnostics",
        None,
        CommandStatus::Ready,
    ),
    (
        "compact",
        "Compact the session context",
        Some("[instructions]"),
        CommandStatus::Planned("P1"),
    ),
    (
        "export",
        "Export the session",
        Some("[path]"),
        CommandStatus::Planned("P1"),
    ),
    (
        "model",
        "Select a model",
        Some("<provider/model>"),
        CommandStatus::Planned("later batch"),
    ),
    (
        "thinking",
        "Set the thinking level",
        Some("<level>"),
        CommandStatus::Planned("later batch"),
    ),
    (
        "settings",
        "Open settings",
        None,
        CommandStatus::Planned("later batch"),
    ),
    (
        "login",
        "Configure provider authentication",
        Some("<provider>"),
        CommandStatus::Planned("later batch"),
    ),
    (
        "logout",
        "Remove provider authentication",
        None,
        CommandStatus::Planned("later batch"),
    ),
    (
        "tree",
        "Navigate the session tree",
        None,
        CommandStatus::Unsupported,
    ),
    (
        "fork",
        "Fork from an earlier message",
        None,
        CommandStatus::Unsupported,
    ),
    (
        "clone",
        "Clone the current session",
        None,
        CommandStatus::Unsupported,
    ),
    (
        "import",
        "Import a session",
        Some("<path>"),
        CommandStatus::Unsupported,
    ),
    (
        "share",
        "Share the session",
        None,
        CommandStatus::Unsupported,
    ),
    (
        "bug",
        "Report a bug",
        Some("<description>"),
        CommandStatus::Unsupported,
    ),
    (
        "copy",
        "Copy the last response",
        None,
        CommandStatus::Unsupported,
    ),
    (
        "name",
        "Set the session name",
        Some("[name]"),
        CommandStatus::Unsupported,
    ),
    (
        "changelog",
        "Show changelog",
        None,
        CommandStatus::Unsupported,
    ),
    (
        "hotkeys",
        "Show shortcuts",
        None,
        CommandStatus::Unsupported,
    ),
    (
        "trust",
        "Save project trust",
        None,
        CommandStatus::Unsupported,
    ),
];

/// 内建命令（固定表序）。
#[must_use]
pub fn builtin_commands() -> Vec<SlashCommand> {
    BUILTIN
        .iter()
        .map(|(name, description, hint, status)| SlashCommand {
            name: (*name).to_string(),
            description: (*description).to_string(),
            argument_hint: hint.map(str::to_string),
            source: CommandSource::Builtin,
            status: *status,
        })
        .collect()
}

/// 合并清单：内建 → 技能 → 扩展；同名先到先得。
#[must_use]
pub fn merge(
    builtin: Vec<SlashCommand>,
    skills: &[Skill],
    skill_commands: bool,
    extensions: &[String],
) -> Vec<SlashCommand> {
    let mut out = builtin;
    if skill_commands {
        for skill in skills {
            out.push(SlashCommand {
                name: format!("skill:{}", skill.name),
                description: skill.description.clone(),
                argument_hint: None,
                source: CommandSource::Skill,
                status: CommandStatus::Ready,
            });
        }
    }
    for name in extensions {
        out.push(SlashCommand {
            name: name.clone(),
            description: "Extension command".to_string(),
            argument_hint: None,
            source: CommandSource::Extension,
            status: CommandStatus::Ready,
        });
    }

    let mut seen = std::collections::HashSet::new();
    out.retain(|command| seen.insert(command.name.clone()));
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn skill(name: &str) -> Skill {
        Skill {
            name: name.into(),
            description: format!("does {name}"),
            file_path: PathBuf::from(format!("/s/{name}/SKILL.md")),
            base_dir: PathBuf::from(format!("/s/{name}")),
            disable_model_invocation: false,
        }
    }

    #[test]
    fn builtin_table_has_unique_names_and_statuses() {
        let commands = builtin_commands();
        let mut names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len());
        assert!(
            commands
                .iter()
                .any(|c| c.name == "reload" && c.status == CommandStatus::Ready)
        );
        assert!(
            commands
                .iter()
                .any(|c| c.name == "tree" && c.status == CommandStatus::Unsupported)
        );
        assert!(
            commands
                .iter()
                .any(|c| matches!(c.status, CommandStatus::Planned(_)))
        );
    }

    #[test]
    fn merge_orders_builtin_then_skills_then_extensions() {
        let commands = merge(
            builtin_commands(),
            &[skill("pdf")],
            true,
            &["plugin_x_echo".into()],
        );
        assert_eq!(commands[0].name, "new");
        let pdf = commands
            .iter()
            .find(|c| c.name == "skill:pdf")
            .expect("skill");
        assert_eq!(pdf.source, CommandSource::Skill);
        assert_eq!(pdf.description, "does pdf");
        assert!(commands.iter().any(|c| c.name == "plugin_x_echo"));
    }

    #[test]
    fn disabled_skill_commands_are_hidden() {
        let commands = merge(builtin_commands(), &[skill("pdf")], false, &[]);
        assert!(!commands.iter().any(|c| c.name == "skill:pdf"));
    }

    #[test]
    fn duplicate_names_keep_the_first() {
        let commands = merge(builtin_commands(), &[], true, &["reload".into()]);
        let reloads: Vec<&SlashCommand> = commands.iter().filter(|c| c.name == "reload").collect();
        assert_eq!(reloads.len(), 1);
        assert_eq!(reloads[0].source, CommandSource::Builtin);
    }
}
