//! 系统提示分节装配（设计 phase-3.5/01 §2~§4）。

use std::path::{Path, PathBuf};

use timo_tools::ToolDefinition;

mod context;
mod skills;

pub(crate) use context::{
    ContextFile, append_system_prompt, custom_prompt, discover_context_files,
};
use skills::Reader;
pub(crate) use skills::{Skill, discover, expand_command, skill_roots};

/// 无标签的首节名；其余节用同名 XML 标签包裹。
pub(crate) const PREAMBLE: &str = "preamble";

/// 默认身份行（本项目自写，语义对齐 pi）。
const DEFAULT_PREAMBLE: &str = "You are Timo-pi, a coding agent operating inside a harness. \
You help the user by reading files, running commands, editing code, and writing new files.";

/// 内置节名；自定义节不得占用。
const BUILTIN_SECTIONS: [&str; 7] = [
    "tools",
    "rules",
    "docs",
    "addendum",
    "project_context",
    "skills",
    "cwd",
];

/// 资源装载告警。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningKind {
    /// 文件读取失败。
    Io,
    /// 技能名不合规（仍加载）。
    InvalidName,
    /// 描述缺失或超长。
    InvalidDescription,
    /// frontmatter 解析问题。
    Parse,
    /// 同名技能冲突（先到先得）。
    Collision,
    /// `--tools` 列表项没有命中任何已注册工具。
    UnknownTool,
}

/// 一条资源告警：装载不中止，逐条上报（`Kernel::resource_warnings`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceWarning {
    /// 告警种类。
    pub kind: WarningKind,
    /// 人读说明。
    pub message: String,
    /// 相关路径。
    pub path: PathBuf,
}

/// 一个提示节。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Section {
    /// 节名（`preamble` 或标签名）。
    pub name: String,
    /// 节内容（渲染时原样放入）。
    pub content: String,
}

/// 装配后的系统提示。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct SystemPrompt {
    sections: Vec<Section>,
}

impl SystemPrompt {
    /// 渲染为发给模型的整段文本（节间空一行，首尾无多余空白）。
    #[must_use]
    pub(crate) fn render(&self) -> String {
        let mut out = String::new();
        for (index, section) in self.sections().iter().enumerate() {
            if index > 0 {
                out.push_str("\n\n");
            }
            if section.name == PREAMBLE {
                out.push_str(&section.content);
            } else {
                out.push('<');
                out.push_str(&section.name);
                out.push_str(">\n");
                out.push_str(&section.content);
                out.push_str("\n</");
                out.push_str(&section.name);
                out.push('>');
            }
        }
        out
    }

    /// 按构建顺序返回各节（诊断与测试）。
    #[must_use]
    pub(crate) fn sections(&self) -> &[Section] {
        &self.sections
    }
}

/// 装配输入（设计 01 §2）。
pub(crate) struct PromptInputs<'a> {
    /// 工作目录。
    pub cwd: &'a Path,
    /// 自定义提示词（替换 preamble，并跳过 tools/rules/docs）。
    pub custom_prompt: Option<&'a str>,
    /// 追加系统提示（addendum 节）。
    pub append_system_prompt: Option<&'a str>,
    /// 已对模型声明的工具（注册顺序）。
    pub tools: &'a [&'a ToolDefinition],
    /// 扩展追加的规则（P0 为空）。
    pub prompt_guidelines: &'a [String],
    /// 已发现的项目上下文文件（全局在前、根到 cwd）。
    pub context_files: &'a [ContextFile],
    /// 已发现的技能。
    pub skills: &'a [Skill],
    /// 文档根目录；None 时整个 docs 节省略。
    pub docs_dir: Option<&'a Path>,
    /// 扩展注册的自定义节（名字 → 内容）。
    pub extra_sections: &'a [(String, String)],
}

/// 装配失败（节名非法等）。调用方降级为身份行并外显错误。
pub(crate) fn assemble(input: &PromptInputs<'_>) -> Result<SystemPrompt, String> {
    for (name, _) in input.extra_sections {
        if !valid_section_name(name) {
            return Err(format!("invalid system prompt section name: {name}"));
        }
        if name == PREAMBLE || BUILTIN_SECTIONS.contains(&name.as_str()) {
            return Err(format!("system prompt section name is reserved: {name}"));
        }
    }

    let custom = input
        .custom_prompt
        .map(str::trim)
        .filter(|text| !text.is_empty());

    let mut sections = Vec::new();
    match custom {
        Some(custom) => sections.push(Section {
            name: PREAMBLE.into(),
            content: custom.to_string(),
        }),
        None => {
            sections.push(Section {
                name: PREAMBLE.into(),
                content: DEFAULT_PREAMBLE.into(),
            });
            sections.push(Section {
                name: "tools".into(),
                content: render_tools(input.tools),
            });
            sections.push(Section {
                name: "rules".into(),
                content: render_rules(input.tools, input.prompt_guidelines),
            });
            if let Some(dir) = input.docs_dir {
                sections.push(Section {
                    name: "docs".into(),
                    content: render_docs(dir),
                });
            }
        }
    }

    if let Some(append) = input
        .append_system_prompt
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        sections.push(Section {
            name: "addendum".into(),
            content: append.to_string(),
        });
    }

    if !input.context_files.is_empty() {
        sections.push(Section {
            name: "project_context".into(),
            content: render_project_context(input.context_files),
        });
    }

    if let Some(reader) = reader_for(input.tools) {
        let body = skills::format_for_prompt(input.skills, reader);
        if !body.trim().is_empty() {
            sections.push(Section {
                name: "skills".into(),
                content: body,
            });
        }
    }

    sections.push(Section {
        name: "cwd".into(),
        content: input.cwd.display().to_string().replace('\\', "/"),
    });

    for (name, content) in input.extra_sections {
        if !content.trim().is_empty() {
            sections.push(Section {
                name: name.clone(),
                content: content.clone(),
            });
        }
    }

    Ok(SystemPrompt { sections })
}

/// 装配失败时的最后防线：只有默认身份行。
#[must_use]
pub(crate) fn fallback() -> SystemPrompt {
    SystemPrompt {
        sections: vec![Section {
            name: PREAMBLE.into(),
            content: DEFAULT_PREAMBLE.into(),
        }],
    }
}

fn valid_section_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn render_tools(tools: &[&ToolDefinition]) -> String {
    let mut lines = Vec::new();
    for tool in tools {
        let Some(snippet) = tool.prompt_snippet.as_deref().map(str::trim) else {
            continue;
        };
        if snippet.is_empty() {
            continue;
        }
        if snippet.starts_with(&tool.name) {
            lines.push(format!("- {snippet}"));
        } else {
            lines.push(format!("- {}: {snippet}", tool.name));
        }
    }
    if lines.is_empty() {
        lines.push("(none)".into());
    }
    lines.push(String::new());
    lines.push("Other tools may be available through the project or extensions.".into());
    lines.join("\n")
}

fn render_rules(tools: &[&ToolDefinition], extra: &[String]) -> String {
    let mut rules: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut add = |rule: &str| {
        let rule = rule.trim();
        if rule.is_empty() || !seen.insert(rule.to_string()) {
            return;
        }
        rules.push(rule.to_string());
    };

    let has = |name: &str| tools.iter().any(|tool| tool.name == name);
    let has_shell = has("bash") || has("powershell");
    if has_shell && !has("grep") && !has("find") && !has("ls") {
        if has("bash") && has("powershell") {
            add("Use bash or PowerShell to list, search, and find files");
        } else if has("powershell") {
            add("Use PowerShell to list, search, and find files");
        } else {
            add("Use bash (ls, rg, find) to inspect files");
        }
    }
    for tool in tools {
        for guideline in &tool.prompt_guidelines {
            add(guideline);
        }
    }
    for guideline in extra {
        add(guideline);
    }
    add("Keep responses concise.");
    add("Name file paths explicitly when you touch them.");
    rules
        .into_iter()
        .map(|rule| format!("- {rule}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_docs(dir: &Path) -> String {
    let dir = dir.display().to_string().replace('\\', "/");
    format!(
        "Timo-pi documentation (read only when the user asks about timo-pi itself, its CLI, extensions, skills, or desktop app):\n\
- Main documentation: {dir}/README.md\n\
- Architecture: {dir}/architecture/\n\
- Standards: {dir}/standards/\n\
- Design notes: {dir}/design/\n\
- Decision records: {dir}/adr/\n\
- Read the docs fully and follow cross-references before changing behavior."
    )
}

fn render_project_context(files: &[ContextFile]) -> String {
    let mut out = String::from("Project instructions found for this workspace:");
    for file in files {
        out.push_str("\n\n<project_instructions path=\"");
        out.push_str(&file.path.display().to_string().replace('\\', "/"));
        out.push_str("\">\n");
        out.push_str(file.content.trim_end());
        out.push_str("\n</project_instructions>");
    }
    out
}

fn reader_for(tools: &[&ToolDefinition]) -> Option<Reader> {
    if tools.iter().any(|tool| tool.name == "read") {
        Some(Reader::Read)
    } else if tools.iter().any(|tool| tool.name == "bash") {
        Some(Reader::Bash)
    } else {
        None
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use timo_tools::{Exposure, ToolRegistry};

    fn tool(name: &str, snippet: Option<&str>, guidelines: &[&str]) -> ToolDefinition {
        ToolDefinition {
            name: name.into(),
            label: name.into(),
            description: "d".into(),
            prompt_snippet: snippet.map(str::to_string),
            prompt_guidelines: guidelines.iter().map(|g| (*g).to_string()).collect(),
            parameters: serde_json::json!({"type": "object"}),
            output_schema: None,
            exposure: Exposure::Direct,
            namespace: None,
            annotations: timo_tools::Annotations::default(),
            execution_mode: timo_tools::ExecutionMode::Parallel,
            execute: std::sync::Arc::new(|_| Box::pin(async { unreachable!("not called") })),
        }
    }

    fn registry_tools(names: &[&str]) -> Vec<ToolDefinition> {
        let mut registry = ToolRegistry::new();
        for name in names {
            registry.register(tool(name, Some(&format!("{name}(arg): do {name}")), &[]));
        }
        registry.declared().into_iter().cloned().collect()
    }

    #[test]
    fn golden_prompt_is_stable() {
        let tools = registry_tools(&["read", "bash"]);
        let refs: Vec<&ToolDefinition> = tools.iter().collect();
        let files = vec![ContextFile {
            path: PathBuf::from("D:/work/AGENTS.md"),
            content: "Be careful.\n".into(),
        }];
        let inputs = PromptInputs {
            cwd: Path::new("D:/work"),
            custom_prompt: None,
            append_system_prompt: Some("Extra rule."),
            tools: &refs,
            prompt_guidelines: &[],
            context_files: &files,
            skills: &[],
            docs_dir: None,
            extra_sections: &[],
        };
        let prompt = assemble(&inputs).expect("assemble");
        assert_eq!(
            prompt.render(),
            concat!(
                "You are Timo-pi, a coding agent operating inside a harness. ",
                "You help the user by reading files, running commands, editing code, and writing new files.\n\n",
                "<tools>\n",
                "- bash(arg): do bash\n- read(arg): do read\n\n",
                "Other tools may be available through the project or extensions.\n",
                "</tools>\n\n",
                "<rules>\n",
                "- Use bash (ls, rg, find) to inspect files\n",
                "- Keep responses concise.\n",
                "- Name file paths explicitly when you touch them.\n",
                "</rules>\n\n",
                "<addendum>\nExtra rule.\n</addendum>\n\n",
                "<project_context>\n",
                "Project instructions found for this workspace:\n\n",
                "<project_instructions path=\"D:/work/AGENTS.md\">\nBe careful.\n</project_instructions>\n",
                "</project_context>\n\n",
                "<cwd>\nD:/work\n</cwd>"
            )
        );
    }

    #[test]
    fn custom_prompt_replaces_preamble_and_skips_default_sections() {
        let tools = registry_tools(&["read"]);
        let refs: Vec<&ToolDefinition> = tools.iter().collect();
        let inputs = PromptInputs {
            cwd: Path::new("/w"),
            custom_prompt: Some("  custom  "),
            append_system_prompt: None,
            tools: &refs,
            prompt_guidelines: &[],
            context_files: &[],
            skills: &[],
            docs_dir: Some(Path::new("/docs")),
            extra_sections: &[],
        };
        let prompt = assemble(&inputs).expect("assemble");
        assert_eq!(prompt.render(), "custom\n\n<cwd>\n/w\n</cwd>");
    }

    #[test]
    fn invalid_section_names_are_rejected() {
        let extra = vec![("Bad".to_string(), "x".to_string())];
        let inputs = PromptInputs {
            cwd: Path::new("/w"),
            custom_prompt: None,
            append_system_prompt: None,
            tools: &[],
            prompt_guidelines: &[],
            context_files: &[],
            skills: &[],
            docs_dir: None,
            extra_sections: &extra,
        };
        assert!(assemble(&inputs).is_err());

        let extra = vec![("rules".to_string(), "x".to_string())];
        let inputs = PromptInputs {
            extra_sections: &extra,
            ..inputs
        };
        assert!(assemble(&inputs).is_err());
    }

    #[test]
    fn empty_extra_sections_are_skipped() {
        let extra = vec![("notes".to_string(), "   ".to_string())];
        let inputs = PromptInputs {
            cwd: Path::new("/w"),
            custom_prompt: None,
            append_system_prompt: None,
            tools: &[],
            prompt_guidelines: &[],
            context_files: &[],
            skills: &[],
            docs_dir: None,
            extra_sections: &extra,
        };
        let prompt = assemble(&inputs).expect("assemble");
        assert!(
            !prompt
                .sections()
                .iter()
                .any(|section| section.name == "notes")
        );
    }

    #[test]
    fn tools_without_snippets_render_none() {
        let mut registry = ToolRegistry::new();
        registry.register(tool("read", None, &[]));
        let tools: Vec<ToolDefinition> = registry.declared().into_iter().cloned().collect();
        let refs: Vec<&ToolDefinition> = tools.iter().collect();
        let inputs = PromptInputs {
            cwd: Path::new("/w"),
            custom_prompt: None,
            append_system_prompt: None,
            tools: &refs,
            prompt_guidelines: &[],
            context_files: &[],
            skills: &[],
            docs_dir: None,
            extra_sections: &[],
        };
        let prompt = assemble(&inputs).expect("assemble");
        assert!(prompt.render().contains("<tools>\n(none)\n"));
    }
}
