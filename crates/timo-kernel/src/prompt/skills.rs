//! 技能提示格式（设计 phase-3.5/02 §4；发现与展开在批次 3 补齐）。

use std::path::PathBuf;

/// 一个已加载的技能（发现逻辑在批次 3）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Skill {
    /// 命令与展示名。
    pub name: String,
    /// 路由描述（模型据此判断是否加载）。
    pub description: String,
    /// `SKILL.md` 路径。
    pub file_path: PathBuf,
    /// 技能目录（`/skill:` 展开时作为相对路径基准）。
    #[allow(dead_code)]
    pub base_dir: PathBuf,
    /// 只允许显式调用，不进提示。
    pub disable_model_invocation: bool,
}

/// 提示里点名用哪个工具读取技能文件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reader {
    /// `read` 工具。
    Read,
    /// `bash` 工具。
    Bash,
    /// 不点名工具（隐藏读取器）。
    #[allow(dead_code)]
    Indirect,
}

/// 生成 `<available_skills>` 节内容；全部隐藏或为空时返回空串。
pub(crate) fn format_for_prompt(skills: &[Skill], reader: Reader) -> String {
    let visible: Vec<&Skill> = skills
        .iter()
        .filter(|skill| !skill.disable_model_invocation)
        .collect();
    if visible.is_empty() {
        return String::new();
    }

    let load = match reader {
        Reader::Read => {
            "When a task matches a description, load that skill's file with the read tool."
        }
        Reader::Bash => "When a task matches a description, load that skill's file with bash.",
        Reader::Indirect => "Load a skill's file when a task matches its description.",
    };

    let mut out = String::from("Skills listed below provide task-specific instructions.\n");
    out.push_str(load);
    out.push('\n');
    out.push_str(
        "Relative paths inside a skill resolve against its directory (next to SKILL.md); \
use that absolute path in tool commands.\n\n<available_skills>",
    );
    for skill in visible {
        out.push_str("\n  <skill>\n    <name>");
        out.push_str(&escape_xml(&skill.name));
        out.push_str("</name>\n    <description>");
        out.push_str(&escape_xml(&skill.description));
        out.push_str("</description>\n    <location>");
        out.push_str(&escape_xml(&skill.file_path.display().to_string()));
        out.push_str("</location>\n  </skill>");
    }
    out.push_str("\n</available_skills>");
    out
}

fn escape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn skill(name: &str, hidden: bool) -> Skill {
        Skill {
            name: name.into(),
            description: format!("{name} <desc> & more"),
            file_path: PathBuf::from(format!("/s/{name}/SKILL.md")),
            base_dir: PathBuf::from(format!("/s/{name}")),
            disable_model_invocation: hidden,
        }
    }

    #[test]
    fn hidden_skills_are_excluded() {
        let skills = vec![skill("a", false), skill("b", true)];
        let text = format_for_prompt(&skills, Reader::Read);
        assert!(text.contains("<name>a</name>"));
        assert!(!text.contains("<name>b</name>"));
    }

    #[test]
    fn all_hidden_yields_empty() {
        assert_eq!(
            format_for_prompt(&[skill("a", true)], Reader::Read),
            String::new()
        );
        assert_eq!(format_for_prompt(&[], Reader::Read), String::new());
    }

    #[test]
    fn xml_is_escaped() {
        let text = format_for_prompt(&[skill("a", false)], Reader::Read);
        assert!(text.contains("<description>a &lt;desc&gt; &amp; more</description>"));
    }

    #[test]
    fn reader_wording_follows_tool() {
        let skills = [skill("a", false)];
        assert!(format_for_prompt(&skills, Reader::Read).contains("with the read tool"));
        assert!(format_for_prompt(&skills, Reader::Bash).contains("with bash"));
        assert!(format_for_prompt(&skills, Reader::Indirect).contains("Load a skill's file"));
    }
}
