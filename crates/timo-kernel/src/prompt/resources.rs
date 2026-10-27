//! 资源装载共享层：目录约定、路径去重键与 frontmatter 解析。
//!
//! 技能（设计 phase-3.5/02）与 prompt 模板（09）共用同一套发现根与同层标量解析，
//! 避免"目录约定"和"frontmatter 语义"各留一个出处。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 一类资源的发现根，按优先级：用户级 `<data_dir>/<dir_name>` → 项目级
/// `.timo/<dir_name>` 与 `.pi/<dir_name>` → 自 cwd 向上每层 `.agents/<dir_name>`
/// （遇 `.git` 停止）→ 显式路径（相对 cwd）。重复路径只保留第一次。
pub(crate) fn resource_roots(
    data_dir: &Path,
    cwd: &Path,
    extra: &[String],
    dir_name: &str,
) -> Vec<PathBuf> {
    let mut roots = vec![
        data_dir.join(dir_name),
        cwd.join(".timo").join(dir_name),
        cwd.join(".pi").join(dir_name),
    ];

    let start = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let mut current = Some(start.as_path());
    while let Some(dir) = current {
        roots.push(dir.join(".agents").join(dir_name));
        if dir.join(".git").exists() {
            break;
        }
        current = dir.parent();
    }

    for raw in extra {
        let path = PathBuf::from(raw);
        roots.push(if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        });
    }

    let mut seen = HashSet::new();
    roots
        .into_iter()
        .filter(|root| seen.insert(canonical_key(root)))
        .collect()
}

/// 路径去重键：能 canonical 就 canonical，大小写不敏感（Windows 友好）。
pub(crate) fn canonical_key(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    canonical.display().to_string().to_lowercase()
}

/// 一段 frontmatter 的解析结果。
pub(crate) struct Frontmatter<'a> {
    /// 单层 `key: value` 标量。
    pub values: HashMap<String, String>,
    /// 认不出的行（嵌套、列表、无冒号行）。
    pub unsupported: Vec<String>,
    /// 起了 `---` 却没收尾。
    pub unterminated: bool,
    /// 正文（未闭合时退回整段原文）。
    pub body: &'a str,
}

/// 只解析单层标量（偏离 D38）；BOM 由调用方剥除。
pub(crate) fn parse_frontmatter(raw: &str) -> Frontmatter<'_> {
    let mut values = HashMap::new();
    let mut unsupported = Vec::new();
    if !raw.starts_with("---") {
        return Frontmatter {
            values,
            unsupported,
            unterminated: false,
            body: raw,
        };
    }

    let mut offset = 0;
    let mut lines = raw.split_inclusive('\n');
    let first = lines.next().unwrap_or_default();
    if first.trim_end() != "---" {
        return Frontmatter {
            values,
            unsupported,
            unterminated: false,
            body: raw,
        };
    }
    offset += first.len();

    let mut closed = false;
    for line in lines {
        offset += line.len();
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        let trimmed = line.trim_end();
        if line.starts_with(' ') || line.starts_with('\t') || trimmed.trim_start().starts_with('-')
        {
            unsupported.push(trimmed.to_string());
            continue;
        }
        match trimmed.split_once(':') {
            Some((key, value)) => {
                values.insert(key.trim().to_string(), unquote(value.trim()).to_string());
            }
            None if !trimmed.trim().is_empty() => unsupported.push(trimmed.to_string()),
            None => {}
        }
    }

    let body = if closed && offset <= raw.len() {
        &raw[offset..]
    } else {
        raw
    };
    Frontmatter {
        values,
        unsupported,
        unterminated: !closed,
        body,
    }
}

fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_run_user_then_project_then_upwards_then_explicit() {
        let cwd = std::env::temp_dir();
        let roots = resource_roots(
            Path::new("/agent"),
            &cwd,
            &["extra/prompts".into()],
            "prompts",
        );
        assert_eq!(roots[0], PathBuf::from("/agent/prompts"));
        assert!(
            roots
                .iter()
                .any(|root| root == &cwd.join(".timo").join("prompts"))
        );
        assert!(
            roots
                .iter()
                .any(|root| root == &cwd.join(".pi").join("prompts"))
        );
        assert_eq!(roots.last().unwrap(), &cwd.join("extra/prompts"));
    }

    #[test]
    fn single_layer_scalars_are_taken_and_blocks_are_reported() {
        let raw = "---\ndescription: says \"hi\"\n  nested: x\n- item\nbroken\n---\nbody here\n";
        let fields = parse_frontmatter(raw);
        assert_eq!(fields.values["description"], "says \"hi\"");
        assert_eq!(fields.unsupported, vec!["  nested: x", "- item", "broken"]);
        assert!(!fields.unterminated);
        assert_eq!(fields.body, "body here\n");
    }

    #[test]
    fn unterminated_frontmatter_keeps_the_whole_file_as_body() {
        let fields = parse_frontmatter("---\ndescription: open\nstill body\n");
        assert!(fields.unterminated);
        assert_eq!(fields.body, "---\ndescription: open\nstill body\n");
    }

    #[test]
    fn a_file_without_frontmatter_keeps_its_body() {
        let fields = parse_frontmatter("plain body\n");
        assert!(fields.values.is_empty());
        assert!(!fields.unterminated);
        assert_eq!(fields.body, "plain body\n");
    }
}
