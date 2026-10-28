//! prompt 模板：`.md` 文件即 `/命令`，提交时展开成提示文本（设计 phase-3.5/09）。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::ResourceWarning;
use super::WarningKind;
use super::resources::{Frontmatter, canonical_key, parse_frontmatter, resource_roots};

/// 一个 prompt 模板。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// `/名字` 的名字 = 文件名去掉 `.md`。
    pub name: String,
    /// frontmatter `description`，缺失则取正文首个非空行截断。
    pub description: String,
    /// frontmatter `argument-hint`，仅用于命令展示。
    pub argument_hint: Option<String>,
    /// 去掉 frontmatter 的正文。
    pub content: String,
    /// 模板文件绝对/给定路径。
    pub file_path: PathBuf,
}

/// 一批模板与装载告警。
#[derive(Debug, Default)]
pub(crate) struct TemplateSet {
    pub templates: Vec<Template>,
    pub warnings: Vec<ResourceWarning>,
}

/// 模板发现根（与技能同一套目录约定，09 §4.1 / D59）。
/// `include_defaults` 为 `false` 时只保留显式路径（`--no-prompt-templates`，09 §2）。
pub(crate) fn template_roots(
    data_dir: &Path,
    cwd: &Path,
    extra: &[String],
    include_defaults: bool,
    project: bool,
) -> Vec<PathBuf> {
    if include_defaults {
        return resource_roots(data_dir, cwd, extra, "prompts", project);
    }
    extra
        .iter()
        .map(|raw| {
            let path = PathBuf::from(raw);
            if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            }
        })
        .collect()
}

/// 装载：目录只取非递归的 `.md`，按路径排序保证稳定；同名先到先得（后来者出冲突告警）。
pub(crate) fn load(roots: &[PathBuf]) -> TemplateSet {
    let mut set = TemplateSet::default();
    let mut seen_files: HashSet<String> = HashSet::new();
    let mut by_name: HashMap<String, PathBuf> = HashMap::new();

    for root in roots {
        let candidates = if root.is_file() {
            vec![root.clone()]
        } else if root.is_dir() {
            let mut files: Vec<PathBuf> = std::fs::read_dir(root)
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|entry| entry.path())
                        .filter(|path| {
                            path.is_file()
                                && !path.to_string_lossy().starts_with('.')
                                && path.extension().is_some_and(|ext| ext == "md")
                        })
                        .collect()
                })
                .unwrap_or_default();
            files.sort();
            files
        } else {
            continue;
        };

        for path in candidates {
            let Some(template) = load_one(&path, &mut set.warnings) else {
                continue;
            };
            if !seen_files.insert(canonical_key(&template.file_path)) {
                continue;
            }
            if let Some(winner) = by_name.get(&template.name) {
                set.warnings.push(ResourceWarning {
                    kind: WarningKind::Collision,
                    message: format!(
                        "prompt template `{}` already loaded from {}",
                        template.name,
                        winner.display()
                    ),
                    path: template.file_path,
                });
                continue;
            }
            by_name.insert(template.name.clone(), template.file_path.clone());
            set.templates.push(template);
        }
    }

    set
}

fn load_one(path: &Path, warnings: &mut Vec<ResourceWarning>) -> Option<Template> {
    if path.extension().is_some_and(|ext| ext != "md") {
        return None;
    }
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw.strip_prefix('\u{feff}').unwrap_or(&raw).to_string(),
        Err(error) => {
            warnings.push(ResourceWarning {
                kind: WarningKind::Io,
                message: format!("read prompt template: {error}"),
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

    let name = path.file_stem()?.to_string_lossy().to_string();
    let content = fields.body.to_string();
    let description = match fields.values.get("description") {
        Some(description) if !description.trim().is_empty() => description.clone(),
        _ => first_line_description(&fields),
    };
    let argument_hint = fields
        .values
        .get("argument-hint")
        .cloned()
        .filter(|hint| !hint.trim().is_empty());

    Some(Template {
        name,
        description,
        argument_hint,
        content,
        file_path: path.to_path_buf(),
    })
}

/// `description` 缺省：正文首个非空行，超过 60 字符截断并加省略号（按字符计，D61）。
fn first_line_description(fields: &Frontmatter<'_>) -> String {
    let Some(line) = fields.body.lines().find(|line| !line.trim().is_empty()) else {
        return String::new();
    };
    if line.chars().count() > 60 {
        let truncated: String = line.chars().take(60).collect();
        format!("{truncated}...")
    } else {
        line.to_string()
    }
}

/// `/name args` 命中模板则返回展开文本，否则 `None`（09 §4.3）。
#[must_use]
pub(crate) fn expand(text: &str, templates: &[Template]) -> Option<String> {
    let rest = text.strip_prefix('/')?;
    let (name, args) = match rest.split_once(char::is_whitespace) {
        Some((name, args)) => (name, args),
        None => (rest, ""),
    };
    let template = templates.iter().find(|template| template.name == name)?;
    Some(substitute(&template.content, &parse_args(args)))
}

/// bash 风格分词：引号内不切分，`""` 保留为一个空参数，未闭合引号走到文末。
fn parse_args(input: &str) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut has_token = false;
    let mut quote: Option<char> = None;

    for char in input.chars() {
        match quote {
            Some(open) => {
                if char == open {
                    quote = None;
                } else {
                    current.push(char);
                }
            }
            None if char == '"' || char == '\'' => {
                quote = Some(char);
                has_token = true;
            }
            None if char.is_whitespace() => {
                if has_token {
                    args.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            None => {
                current.push(char);
                has_token = true;
            }
        }
    }
    if has_token {
        args.push(current);
    }
    args
}

/// 只替换模板文本；参数值与默认值里的占位符不递归展开（09 §2）。
fn substitute(content: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(content.len());
    let chars: Vec<char> = content.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        if chars[index] != '$' {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        match placeholder(&chars, index, args) {
            Some((length, value)) => {
                out.push_str(&value);
                index += length;
            }
            None => {
                out.push('$');
                index += 1;
            }
        }
    }
    out
}

/// 从 `$` 处解析一个占位符，返回（消耗字符数，替换值）；不是占位符则 `None`。
fn placeholder(chars: &[char], start: usize, args: &[String]) -> Option<(usize, String)> {
    debug_assert_eq!(chars[start], '$');
    let tail = &chars[start + 1..];
    if tail.first() == Some(&'{') {
        let close = tail.iter().position(|c| *c == '}')?;
        let inner: String = tail[1..close].iter().collect();
        // 消耗 = `$` + `{` + 内容 + `}`。
        return Some((close + 2, braced(args, &inner)?));
    }

    if starts_with(tail, "ARGUMENTS") {
        return Some(("ARGUMENTS".chars().count() + 1, args.join(" ")));
    }
    if tail.first() == Some(&'@') {
        return Some((2, args.join(" ")));
    }
    let digits = tail.iter().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let value = positional(args, &tail[..digits].iter().collect::<String>()).unwrap_or_default();
    Some((digits + 1, value))
}

/// `${...}` 三种形态：`X:-默认`、`@:N`、`@:N:L`（09 §2）。
/// 目标名只认 `@`、`ARGUMENTS` 与数字；其它写法整段原样保留（蓝本正则不匹配）。
fn braced(args: &[String], inner: &str) -> Option<String> {
    if let Some((target, default)) = inner.split_once(":-") {
        let value = match target {
            "@" | "ARGUMENTS" => args.join(" "),
            digits if is_digits(digits) => positional(args, digits).unwrap_or_default(),
            _ => return None,
        };
        return Some(if value.is_empty() {
            default.to_string()
        } else {
            value
        });
    }

    let (target, spec) = inner.split_once(':')?;
    if target != "@" {
        return None;
    }
    let (from, length) = match spec.split_once(':') {
        Some((from, length)) => (from, Some(length)),
        None => (spec, None),
    };
    // 蓝本约定：1 起始，`0` 视同 `1`；越界只取到有的部分。
    let start = from
        .parse::<usize>()
        .ok()?
        .saturating_sub(1)
        .min(args.len());
    let slice = match length {
        Some(length) => {
            let count = length.parse::<usize>().ok()?;
            &args[start..(start + count).min(args.len())]
        }
        None => &args[start..],
    };
    Some(slice.join(" "))
}

fn is_digits(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_digit())
}

/// `$N` 与 `${N:-…}` 共用：1 起始；`0`、非法数字与越界都算"没有值"（蓝本 `args[N-1] ?? ""`）。
fn positional(args: &[String], digits: &str) -> Option<String> {
    let index: usize = digits.parse().ok()?;
    args.get(index.checked_sub(1)?)
        .filter(|value| !value.is_empty())
        .cloned()
}

fn starts_with(chars: &[char], word: &str) -> bool {
    word.chars().zip(chars).all(|(left, right)| left == *right)
}

#[cfg(test)]
mod tests;
