//! 项目资源清单与触发检测（设计 phase-3.5/12 §3.3）。
//!
//! 清单是**唯一出处**：触发检测与逐面门控都从这里派生。新增一类项目资源时
//! 只改本文件，不要在各加载器里各写一份判据。

use std::path::{Path, PathBuf};

use crate::prompt::resources::canonical_key;

/// 一类受门控的项目资源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    /// `<cwd>/.timo/settings.json`。
    Settings,
    /// `<cwd>/.timo/mcp.json`。
    Mcp,
    /// `<cwd>/.timo/extensions` 与 `<cwd>/.pi/extensions`。
    Extensions,
    /// 技能根（`.timo`/`.pi`/祖先 `.agents`）。
    Skills,
    /// 模板根（`.timo`/`.pi`/祖先 `.agents`）。
    Prompts,
    /// `<cwd>/.timo/SYSTEM.md`。
    SystemPrompt,
    /// `<cwd>/.timo/APPEND_SYSTEM.md`。
    AppendSystemPrompt,
    /// `<cwd>/.timo/agents`（子代理定义）。
    Agents,
}

impl ResourceKind {
    /// 面向用户的短名。
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::Mcp => "mcp",
            Self::Extensions => "extensions",
            Self::Skills => "skills",
            Self::Prompts => "prompts",
            Self::SystemPrompt => "SYSTEM.md",
            Self::AppendSystemPrompt => "APPEND_SYSTEM.md",
            Self::Agents => "agents",
        }
    }
}

/// 一条存在于磁盘上的项目资源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectResource {
    /// 资源种类。
    pub kind: ResourceKind,
    /// 命中路径。
    pub path: PathBuf,
}

/// `<cwd>/.timo` 下的固定条目（顺序即展示顺序）。
const TIMO_ENTRIES: [(ResourceKind, &str); 8] = [
    (ResourceKind::Settings, "settings.json"),
    (ResourceKind::Mcp, "mcp.json"),
    (ResourceKind::Extensions, "extensions"),
    (ResourceKind::Skills, "skills"),
    (ResourceKind::Prompts, "prompts"),
    (ResourceKind::SystemPrompt, "SYSTEM.md"),
    (ResourceKind::AppendSystemPrompt, "APPEND_SYSTEM.md"),
    (ResourceKind::Agents, "agents"),
];

/// 祖先目录里的项目资源（pi 只扫 `.agents/skills`；本仓多一类 `.agents/prompts`，D86）。
const ANCESTOR_ENTRIES: [(ResourceKind, &str); 2] = [
    (ResourceKind::Skills, "skills"),
    (ResourceKind::Prompts, "prompts"),
];

/// 扫描需要信任决定的项目资源；空列表 = 无需询问。
///
/// `.timo` 只看 cwd；`.agents/<dir>` 从 cwd 逐级到根。用户主目录下的
/// `.agents/<dir>` 视为用户级资源，不算触发（与 pi 的 `$HOME/.agents/skills` 同义）。
#[must_use]
pub fn scan(cwd: &Path) -> Vec<ProjectResource> {
    scan_with_home(cwd, home_dir().as_deref())
}

/// 同 [`scan`]，主目录显式传入（测试用；生产走进程环境）。
#[must_use]
fn scan_with_home(cwd: &Path, home: Option<&Path>) -> Vec<ProjectResource> {
    let mut out = Vec::new();
    let timo = cwd.join(".timo");
    for (kind, name) in TIMO_ENTRIES {
        let path = timo.join(name);
        if path.exists() {
            out.push(ProjectResource { kind, path });
        }
    }
    if cwd.join(".pi").join("extensions").exists() {
        out.push(ProjectResource {
            kind: ResourceKind::Extensions,
            path: cwd.join(".pi").join("extensions"),
        });
    }
    if cwd.join(".pi").join("skills").exists() {
        out.push(ProjectResource {
            kind: ResourceKind::Skills,
            path: cwd.join(".pi").join("skills"),
        });
    }
    if cwd.join(".pi").join("prompts").exists() {
        out.push(ProjectResource {
            kind: ResourceKind::Prompts,
            path: cwd.join(".pi").join("prompts"),
        });
    }

    let start = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let mut current = Some(start.as_path());
    while let Some(dir) = current {
        if !is_user_home(dir, home) {
            for (kind, name) in ANCESTOR_ENTRIES {
                let path = dir.join(".agents").join(name);
                if path.exists() {
                    out.push(ProjectResource { kind, path });
                }
            }
        }
        current = dir.parent();
    }
    out
}

/// 当前祖先目录是否就是用户主目录：`$HOME/.agents/<dir>` 是用户级资源。
fn is_user_home(dir: &Path, home: Option<&Path>) -> bool {
    home.is_some_and(|home| same_path(dir, home))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|raw| !raw.is_empty())
        .map(PathBuf::from)
}

fn same_path(left: &Path, right: &Path) -> bool {
    canonical_key(left) == canonical_key(right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("timo-trust-gate-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn empty_timo_dir_is_not_a_trigger() {
        let work = temp("empty");
        fs::create_dir_all(work.join(".timo")).unwrap();
        assert!(scan(&work).is_empty());
        let _ = fs::remove_dir_all(&work);
    }

    #[test]
    fn every_timo_entry_is_a_trigger() {
        let work = temp("entries");
        for (_, name) in TIMO_ENTRIES {
            let path = work.join(".timo").join(name);
            if name.ends_with(".md") || name.ends_with(".json") {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, "x").unwrap();
            } else {
                fs::create_dir_all(&path).unwrap();
            }
        }
        let found = scan(&work);
        assert_eq!(found.len(), TIMO_ENTRIES.len());
        assert_eq!(found[0].kind, ResourceKind::Settings);
        let _ = fs::remove_dir_all(&work);
    }

    #[test]
    fn ancestor_agents_skills_are_a_trigger() {
        let root = temp("ancestor");
        let work = root.join("repo").join("crate");
        fs::create_dir_all(&work).unwrap();
        fs::create_dir_all(root.join("repo").join(".agents").join("skills")).unwrap();
        fs::create_dir_all(root.join("repo").join(".git")).unwrap();
        let found = scan(&work);
        assert!(found.iter().any(|r| r.kind == ResourceKind::Skills
            && (r.path.ends_with(".agents/skills") || r.path.ends_with(".agents\\skills"))));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn home_agents_skills_is_a_user_resource() {
        // 主目录下的 `.agents/*` 只在当前机器存在时才可观测；不存在则跳过。
        let Some(home) = home_dir() else {
            return;
        };
        if !home.join(".agents").join("skills").exists() {
            return;
        }
        let work = home.join(format!(".timo-trust-home-{}", std::process::id()));
        let _ = fs::remove_dir_all(&work);
        fs::create_dir_all(&work).unwrap();

        let trusted_view = scan_with_home(&work, Some(&home));
        assert!(
            !trusted_view.iter().any(|r| r.kind == ResourceKind::Skills),
            "{trusted_view:?}"
        );
        // 同一份目录在"主目录未知"时是普通项目资源（反向对照）。
        let plain = scan_with_home(&work, None);
        assert!(
            plain.iter().any(|r| r.kind == ResourceKind::Skills),
            "{plain:?}"
        );
        let _ = fs::remove_dir_all(&work);
    }
}
