//! 技能发现、校验、提示注入与 `/skill:` 展开（设计 phase-3.5/02 §2~§5）。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::resources::{canonical_key, parse_frontmatter};
use super::{ResourceWarning, WarningKind};

/// 一个已加载的技能。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Skill {
    /// 命令与展示名。
    pub name: String,
    /// 路由描述（模型据此判断是否加载）。
    pub description: String,
    /// `SKILL.md` 路径。
    pub file_path: PathBuf,
    /// 技能目录（`/skill:` 展开时作为相对路径基准）。
    pub base_dir: PathBuf,
    /// 只允许显式调用，不进提示。
    pub disable_model_invocation: bool,
}

/// 一次发现的结果。
#[derive(Debug, Default)]
pub(crate) struct SkillSet {
    /// 按发现顺序（先到先得）。
    pub skills: Vec<Skill>,
    /// 逐条告警。
    pub warnings: Vec<ResourceWarning>,
}

/// 提示里点名用哪个工具读取技能文件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reader {
    /// `read` 工具。
    Read,
    /// `bash` 工具。
    Bash,
}

/// 技能根，按发现顺序；重复路径只保留第一次（约定见 `prompt::resources`）。
pub(crate) fn skill_roots(data_dir: &Path, cwd: &Path, extra: &[String]) -> Vec<PathBuf> {
    super::resources::resource_roots(data_dir, cwd, extra, "skills")
}

/// 发现技能：目录含 `SKILL.md` 即技能根、不再下钻；根级 `*.md` 视为独立技能。
pub(crate) fn discover(roots: &[PathBuf]) -> SkillSet {
    let mut set = SkillSet::default();
    let mut seen_files: HashSet<String> = HashSet::new();
    let mut by_name: HashMap<String, PathBuf> = HashMap::new();

    for root in roots {
        if root.is_file() {
            if let Some(skill) = load_skill(root, &mut set.warnings) {
                accept(skill, &mut set, &mut seen_files, &mut by_name);
            }
            continue;
        }
        if !root.is_dir() {
            continue;
        }

        let declared = root.join("SKILL.md");
        if declared.is_file() {
            if let Some(skill) = load_skill(&declared, &mut set.warnings) {
                accept(skill, &mut set, &mut seen_files, &mut by_name);
            }
            continue;
        }

        let mut candidates = Vec::new();
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') || name == "node_modules" || !path.is_file() {
                    continue;
                }
                if path.extension().is_some_and(|ext| ext == "md") {
                    candidates.push(path);
                }
            }
        }
        candidates.sort();
        for path in candidates {
            if let Some(skill) = load_skill(&path, &mut set.warnings) {
                accept(skill, &mut set, &mut seen_files, &mut by_name);
            }
        }

        for path in walk_skill_files(root) {
            if has_skill_root_between(root, &path) {
                continue;
            }
            if let Some(skill) = load_skill(&path, &mut set.warnings) {
                accept(skill, &mut set, &mut seen_files, &mut by_name);
            }
        }
    }

    set
}

fn accept(
    skill: Skill,
    set: &mut SkillSet,
    seen_files: &mut HashSet<String>,
    by_name: &mut HashMap<String, PathBuf>,
) {
    if !seen_files.insert(canonical_key(&skill.file_path)) {
        return;
    }
    if let Some(winner) = by_name.get(&skill.name) {
        set.warnings.push(ResourceWarning {
            kind: WarningKind::Collision,
            message: format!(
                "skill name `{}` already loaded from {}",
                skill.name,
                winner.display()
            ),
            path: skill.file_path,
        });
        return;
    }
    by_name.insert(skill.name.clone(), skill.file_path.clone());
    set.skills.push(skill);
}

fn walk_skill_files(root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .ignore(true)
        .require_git(false)
        .follow_links(true)
        .build();
    for entry in walker.flatten() {
        if entry.file_name() != "SKILL.md" {
            continue;
        }
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path
            .components()
            .any(|component| component.as_os_str() == "node_modules")
        {
            continue;
        }
        paths.push(path.to_path_buf());
    }
    paths.sort();
    paths
}

fn has_skill_root_between(root: &Path, file: &Path) -> bool {
    // 从技能的上一级目录开始：技能自身目录含 SKILL.md 是正常的。
    let mut current = file.parent().and_then(Path::parent);
    while let Some(dir) = current {
        if dir == root {
            break;
        }
        if dir.join("SKILL.md").is_file() {
            return true;
        }
        current = dir.parent();
    }
    false
}

fn load_skill(path: &Path, warnings: &mut Vec<ResourceWarning>) -> Option<Skill> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw.strip_prefix('\u{feff}').unwrap_or(&raw).to_string(),
        Err(error) => {
            warnings.push(ResourceWarning {
                kind: WarningKind::Io,
                message: format!("read skill: {error}"),
                path: path.to_path_buf(),
            });
            return None;
        }
    };

    let fields = parse_frontmatter(&raw);
    if fields.unterminated {
        warnings.push(ResourceWarning {
            kind: WarningKind::Parse,
            message: "frontmatter is not terminated".into(),
            path: path.to_path_buf(),
        });
        return None;
    }
    for line in &fields.unsupported {
        warnings.push(ResourceWarning {
            kind: WarningKind::Parse,
            message: format!("unsupported frontmatter: {line}"),
            path: path.to_path_buf(),
        });
    }

    let parent_name = path
        .parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = fields
        .values
        .get("name")
        .cloned()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(parent_name);

    let description = fields
        .values
        .get("description")
        .cloned()
        .unwrap_or_default();
    let has_description = !description.trim().is_empty();

    if let Some(problem) = name_problem(&name) {
        warnings.push(ResourceWarning {
            kind: WarningKind::InvalidName,
            message: problem,
            path: path.to_path_buf(),
        });
    }
    if !has_description {
        warnings.push(ResourceWarning {
            kind: WarningKind::InvalidDescription,
            message: "description is required".into(),
            path: path.to_path_buf(),
        });
        return None;
    }
    if description.chars().count() > 1024 {
        warnings.push(ResourceWarning {
            kind: WarningKind::InvalidDescription,
            message: "description exceeds 1024 characters".into(),
            path: path.to_path_buf(),
        });
    }

    let base_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    Some(Skill {
        name,
        description,
        file_path: path.to_path_buf(),
        base_dir,
        disable_model_invocation: fields
            .values
            .get("disable-model-invocation")
            .is_some_and(|value| value.trim() == "true"),
    })
}

fn name_problem(name: &str) -> Option<String> {
    if name.chars().count() > 64 {
        return Some("name exceeds 64 characters".into());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Some("name must use lowercase letters, digits and hyphens".into());
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Some("name must not start or end with a hyphen".into());
    }
    if name.contains("--") {
        return Some("name must not contain consecutive hyphens".into());
    }
    None
}

/// 展开 `/skill:name args`；未知或读取失败返回 None（原样透传）。
pub(crate) fn expand_command(text: &str, skills: &[Skill]) -> Option<String> {
    let rest = text.strip_prefix("/skill:")?;
    let (name, args) = match rest.split_once(char::is_whitespace) {
        Some((name, args)) => (name, args.trim()),
        None => (rest, ""),
    };
    let skill = skills.iter().find(|skill| skill.name == name)?;
    let raw = std::fs::read_to_string(&skill.file_path).ok()?;
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    let fields = parse_frontmatter(raw);
    let body = if fields.unterminated {
        raw
    } else {
        fields.body
    }
    .trim();

    let mut out = format!(
        "<skill name=\"{}\" location=\"{}\">\nRelative paths resolve against {}.\n\n{}\n</skill>",
        skill.name,
        skill.file_path.display().to_string().replace('\\', "/"),
        skill.base_dir.display().to_string().replace('\\', "/"),
        body
    );
    if !args.is_empty() {
        out.push_str("\n\n");
        out.push_str(args);
    }
    Some(out)
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
mod tests;
