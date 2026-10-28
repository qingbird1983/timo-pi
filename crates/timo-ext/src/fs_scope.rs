//! `fs.read` 路径规范化与作用域匹配（设计 phase-2/03 §2.2；phase-3.5/08 §4.1）。
//!
//! 宿主文件 API 的唯一强制点：先查硬拒路径（E404），再查声明作用域（E401/E403）。

use std::path::{Component, Path, PathBuf};

/// 单次读取上限（超出截断并追加标记；设计 08 §4.1）。
pub const READ_CAP_BYTES: usize = 1024 * 1024;

/// 展开 `~`、按工作目录解析相对路径、折叠 `.`/`..`（不跟随符号链接）。
#[must_use]
pub fn resolve(cwd: &Path, raw: &str) -> PathBuf {
    let expanded = expand_home(raw);
    let joined = if Path::new(&expanded).is_absolute() {
        PathBuf::from(&expanded)
    } else {
        cwd.join(&expanded)
    };
    fold(&joined)
}

fn expand_home(raw: &str) -> String {
    if raw == "~" || raw.starts_with("~/") || raw.starts_with("~\\") {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_default();
        if !home.is_empty() {
            return format!("{home}{}", &raw[1..]);
        }
    }
    raw.to_string()
}

fn fold(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return path.to_path_buf();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 归一化为用于匹配的正斜杠字符串（Windows 大小写折叠）。
#[must_use]
pub fn key(path: &Path) -> String {
    let text = path
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string();
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

/// 目录作用域写法 `<path>/**`（与 [`key`] 同一套归一化）。
///
/// 装机时用它把数据目录补进 `fs.read`：用户级 `agents/`、`prompts/` 不在 cwd 之下，
/// `**`（相对 cwd）覆盖不到，真机上读代理定义会被 E403 拒（09 §6 真机发现）。
#[must_use]
pub fn dir_scope(path: &Path) -> String {
    let key = key(path);
    if key.ends_with("/**") {
        key
    } else {
        format!("{key}/**")
    }
}

/// 读检查：返回错误文案（含 `E4xx` 前缀）或解析后的绝对路径。
///
/// # Errors
/// 未授予 → `E401`；硬拒路径 → `E404`；不在声明作用域 → `E403`。
pub fn check_read(cwd: &Path, scopes: &[String], raw: &str) -> Result<PathBuf, String> {
    let absolute = resolve(cwd, raw);
    let target = key(&absolute);
    if is_protected(&target) {
        return Err(format!("E404 protected path is never readable: `{raw}`"));
    }
    if scopes.is_empty() {
        return Err(format!(
            "E401 fs.read capability is not granted (call site readFile, path `{raw}`)"
        ));
    }
    let granted: Vec<String> = scopes.iter().map(|s| key(&resolve(cwd, s))).collect();
    if granted.iter().any(|scope| allowed(scope, &target)) {
        return Ok(absolute);
    }
    Err(format!(
        "E403 path `{raw}` is outside the declared fs.read scopes"
    ))
}

/// 硬拒清单：凭据与版本库内部（读/写/删全禁，设计 03 §2.2）。
fn is_protected(target: &str) -> bool {
    let segments: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();
    let last = segments.last().copied().unwrap_or_default();
    segments.iter().any(|s| *s == ".git" || *s == ".ssh")
        || last.starts_with(".env")
        || matches!(last, "id_rsa" | "id_ed25519" | "id_ecdsa" | "known_hosts")
        || last.ends_with(".pem")
        || last.ends_with(".p12")
}

/// 作用域匹配：目录前缀等价 `dir/**`；`*` 单段、`**` 任意深度、`?` 单字符。
fn allowed(scope: &str, target: &str) -> bool {
    if target == scope || target.starts_with(&format!("{scope}/")) {
        return true;
    }
    if scope.contains(['*', '?']) {
        return glob_match(scope, target);
    }
    false
}

/// 通配符匹配（`globset`；`literal_separator` 让 `*`/`?` 不跨段）。
///
/// `globset` 视尾部 `/**` 为至少一段，这里先试「`**` 吞 0 段」：
/// 去掉尾部 `/**` 后仍能匹配，即命中原目录本身。
fn glob_match(pattern: &str, target: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix("/**")
        && glob_match(prefix, target)
    {
        return true;
    }
    let literal = escape_glob_metachars(pattern);
    globset::GlobBuilder::new(&literal)
        .literal_separator(true)
        // Windows 上 `\` 默认是分隔符；这里只有转义用途（pattern 已归一化为 `/`）。
        .backslash_escape(true)
        .build()
        .is_ok_and(|glob| glob.compile_matcher().is_match(target))
}

/// 只保留 `*`/`?`/`**` 三类通配：其余 globset 元字符（字符类、交替）按字面量转义，
/// 避免 scope 意外扩大授权面。
fn escape_glob_metachars(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    for ch in pattern.chars() {
        if matches!(ch, '[' | ']' | '{' | '}' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// 截断到 [`READ_CAP_BYTES`]，并追加可读标记。
#[must_use]
pub fn cap_text(text: &str) -> String {
    if text.len() <= READ_CAP_BYTES {
        return text.to_string();
    }
    let mut cut = READ_CAP_BYTES;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n[truncated]", &text[..cut])
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn cwd() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\work\proj")
        } else {
            PathBuf::from("/work/proj")
        }
    }

    #[test]
    fn relative_paths_resolve_against_cwd_and_fold() {
        let folded = resolve(&cwd(), "./docs/../docs/a.md");
        assert_eq!(key(&folded), format!("{}/docs/a.md", key(&cwd())));
        let abs = resolve(&cwd(), "agents/scout.md");
        assert!(key(&abs).ends_with("agents/scout.md"));
    }

    #[test]
    fn dir_scope_grants_a_directory_outside_cwd() {
        // 数据目录与 cwd 无关（真机：cwd 是项目，data_dir 在用户配置目录）。
        let data = if cfg!(windows) {
            PathBuf::from(r"D:gent-data")
        } else {
            PathBuf::from("/agent-data")
        };
        let scope = dir_scope(&data);
        assert!(scope.ends_with("/**"), "{scope}");
        // 幂等：已经是 `/**` 的写法不再叠一层。
        assert_eq!(dir_scope(&PathBuf::from(&scope)), scope);
        // 数据目录在 cwd 之外：cwd 的 `**` 够不着，dir_scope 够得着（真机 E403 的根因）。
        let agent = data.join("agents").join("scout.md");
        assert!(!allowed(&key(&cwd().join("**")), &key(&agent)));
        assert!(allowed(&scope, &key(&agent)));
    }

    #[test]
    fn scope_prefix_and_wildcards() {
        assert!(allowed("/work/proj/docs", "/work/proj/docs/a/b.md"));
        assert!(allowed("/work/proj/docs", "/work/proj/docs"));
        assert!(!allowed("/work/proj/docs", "/work/proj/docx/a.md"));
        assert!(allowed("/work/proj/docs/**", "/work/proj/docs/a/b.md"));
        assert!(allowed("/work/proj/*/b.md", "/work/proj/a/b.md"));
        assert!(!allowed("/work/proj/*/b.md", "/work/proj/a/c/b.md"));
        assert!(allowed("/work/proj/?.md", "/work/proj/x.md"));
        assert!(!allowed("/work/proj/?.md", "/work/proj/xy.md"));
    }

    #[test]
    fn wildcards_follow_segment_semantics() {
        let cases = [
            ("**", "/work/proj/docs/a.md", true),
            ("/work/proj/**", "/work/proj", true),
            ("/work/proj/**", "/work/proj/docs/a.md", true),
            ("/work/proj/**/b.md", "/work/proj/b.md", true),
            ("/work/proj/**/b.md", "/work/proj/a/b.md", true),
            ("/work/proj/*/b.md", "/work/proj/a/b.md", true),
            ("/work/proj/*/b.md", "/work/proj/a/c/b.md", false),
            ("/work/proj/?.md", "/work/proj/x.md", true),
            ("/work/proj/?.md", "/work/proj/xy.md", false),
            ("/work/**/x/**", "/work/x", true),
            ("/work/**/x/**", "/work/a/b/x/c", true),
            ("/work/a*b/c", "/work/axxb/c", true),
            // 字符类/交替不是本作用域的语法：按字面量匹配，不扩大授权面。
            ("/work/[a]/x", "/work/[a]/x", true),
            ("/work/[a]/x", "/work/a/x", false),
            ("/work/{a,b}", "/work/a", false),
        ];
        for (pattern, target, expected) in cases {
            assert_eq!(
                glob_match(pattern, target),
                expected,
                "{pattern} vs {target}"
            );
        }
    }

    #[test]
    fn protected_paths_are_refused_before_scopes() {
        let err = check_read(&cwd(), &["**".to_string()], ".env.local").unwrap_err();
        assert!(err.starts_with("E404"), "{err}");
        let err = check_read(&cwd(), &["**".to_string()], ".git/config").unwrap_err();
        assert!(err.starts_with("E404"), "{err}");
        let err = check_read(&cwd(), &["**".to_string()], "secrets/id_rsa").unwrap_err();
        assert!(err.starts_with("E404"), "{err}");
    }

    #[test]
    fn missing_scope_is_e401_and_outside_is_e403() {
        let err = check_read(&cwd(), &[], "docs/a.md").unwrap_err();
        assert!(err.starts_with("E401"), "{err}");
        let err = check_read(&cwd(), &["./agents".to_string()], "docs/a.md").unwrap_err();
        assert!(err.starts_with("E403"), "{err}");
        assert!(check_read(&cwd(), &["./agents".to_string()], "agents/scout.md").is_ok());
    }

    #[test]
    fn cap_keeps_utf8_and_marks_truncation() {
        let text = "é".repeat(READ_CAP_BYTES);
        let capped = cap_text(&text);
        assert!(capped.ends_with("\n[truncated]"));
        assert!(capped.len() <= READ_CAP_BYTES + 13);
        assert_eq!(cap_text("short"), "short");
    }
}
