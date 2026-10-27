//! 统一工具注册表（接缝 #4 检查点：来源不可区分，仅名字前缀不同）。

use std::collections::BTreeMap;

use crate::Result;
use crate::definition::{Exposure, ToolDefinition, ToolOutput};
use crate::error::ToolError;

/// `--tools` 列表项匹配：整名相等，或含一个 `*` 的前后缀模式（`mcp__radius__*`）。
pub fn pattern_matches(pattern: &str, name: &str) -> bool {
    if pattern == "*" || pattern == name {
        return true;
    }
    match pattern.split_once('*') {
        Some((head, tail)) => name.starts_with(head) && name.ends_with(tail),
        None => false,
    }
}

/// 工具注册表：Phase 1 持内置 + MCP 工具；Phase 2 扩展工具同样入表。
///
/// 可克隆：工具定义共享执行闭包（`Arc`），克隆用于给 agent 一个装配期快照。
#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: BTreeMap<String, ToolDefinition>,
}

impl ToolRegistry {
    /// 空注册表。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个工具（同名覆盖）。
    pub fn register(&mut self, def: ToolDefinition) {
        self.tools.insert(def.name.clone(), def);
    }

    /// 批量注册。
    pub fn register_all(&mut self, defs: impl IntoIterator<Item = ToolDefinition>) {
        for d in defs {
            self.register(d);
        }
    }

    /// 查找。
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ToolDefinition> {
        self.tools.get(name)
    }

    /// 对模型声明的工具清单（prompt 组装用；设计 03 §1 默认激活集由内核过滤）。
    #[must_use]
    pub fn declared(&self) -> Vec<&ToolDefinition> {
        self.tools
            .values()
            .filter(|d| d.declared_to_model())
            .collect()
    }

    /// 全部工具（诊断/列表用）。
    #[must_use]
    pub fn all(&self) -> Vec<&ToolDefinition> {
        self.tools.values().collect()
    }

    /// 按 `--tools` 允许列表收敛声明面（设计 08 §4.2）：列出的就是**最终允许集合**，
    /// 命中即对模型声明（原本 Hidden/Deferred 的恢复为 [`Exposure::Direct`]），
    /// 未命中的置 [`Exposure::Hidden`]；注册表本身不变，只改声明面。
    ///
    /// 返回一个工具都没命中的列表项，由调用方逐条告警。
    pub fn apply_allowlist(&mut self, allowlist: &[String]) -> Vec<String> {
        let mut unmatched = allowlist.to_vec();
        for def in self.tools.values_mut() {
            let before = unmatched.len();
            unmatched.retain(|pattern| !pattern_matches(pattern, &def.name));
            if unmatched.len() == before {
                def.exposure = Exposure::Hidden;
            } else if !def.declared_to_model() {
                def.exposure = Exposure::Direct;
            }
        }
        unmatched
    }

    /// 统一执行入口：校验参数 → 执行（MCP/内置同一条管线，architecture/02 §4）。
    ///
    /// # Errors
    /// 未找到 → [`ToolError::ToolNotFound`]；参数校验失败 → [`ToolError::InvalidParams`]；
    /// 执行错误原样透传（循环转错误 result）。
    pub async fn execute(
        &self,
        name: &str,
        ctx: crate::definition::ToolCallCtx,
    ) -> Result<ToolOutput> {
        let def = self
            .tools
            .get(name)
            .ok_or_else(|| ToolError::ToolNotFound {
                name: name.to_string(),
            })?;
        crate::definition::validate_params(&def.parameters, &ctx.params)?;
        def.run(ctx).await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::definition::{ToolCallCtx, ToolDefinition};
    use serde_json::json;
    use std::sync::Arc;

    fn dummy(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.into(),
            label: name.into(),
            description: "d".into(),
            prompt_snippet: None,
            prompt_guidelines: vec![],
            parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
            output_schema: None,
            exposure: crate::definition::Exposure::Direct,
            namespace: None,
            annotations: crate::definition::Annotations::default(),
            execution_mode: crate::definition::ExecutionMode::Parallel,
            execute: Arc::new(|ctx| {
                Box::pin(async move {
                    let p = ctx.params["path"].as_str().unwrap_or_default();
                    Ok(ToolOutput::text(format!("ran {p}")))
                })
            }),
        }
    }

    #[test]
    fn allowlist_enables_listed_tools_and_hides_the_rest() {
        let mut reg = ToolRegistry::new();
        for name in ["read", "mcp__radius__search", "mcp__other__run"] {
            reg.register(dummy(name));
        }
        // grep 模拟"注册但被 default_tools 关掉"：列出来就必须恢复声明
        let mut off = dummy("grep");
        off.exposure = Exposure::Hidden;
        reg.register(off);

        let stray = reg.apply_allowlist(&["read".into(), "grep".into(), "mcp__radius__*".into()]);
        let declared: Vec<String> = reg.declared().iter().map(|d| d.name.clone()).collect();
        assert_eq!(declared, ["grep", "mcp__radius__search", "read"]);
        assert_eq!(reg.all().len(), 4, "注册表不变，只改声明面");
        assert!(stray.is_empty(), "每个列表项都命中过就不该报未知");
        assert_eq!(
            reg.apply_allowlist(&["nope".into()]),
            ["nope".to_string()],
            "一个都没命中的列表项要报回去"
        );
    }

    #[test]
    fn pattern_matches_covers_exact_star_and_affixes() {
        assert!(pattern_matches("read", "read"));
        assert!(pattern_matches("*", "anything"));
        assert!(pattern_matches("mcp__radius__*", "mcp__radius__search"));
        assert!(!pattern_matches("mcp__radius__*", "mcp__other__search"));
        assert!(!pattern_matches("re", "read"));
    }

    #[tokio::test]
    async fn uniform_pipeline_builtin_and_namespaced() {
        let mut reg = ToolRegistry::new();
        reg.register(dummy("read"));
        let mut mcp = dummy("mcp__docs__search");
        mcp.namespace = Some(crate::definition::Namespace {
            name: "mcp__docs".into(),
            description: Some("docs".into()),
        });
        reg.register(mcp);
        // 同一注册表、同一管线（接缝 #4 断言）
        for name in ["read", "mcp__docs__search"] {
            let ctx = ToolCallCtx {
                tool_call_id: "t1".into(),
                params: json!({"path": "x"}),
                cancel: tokio_util::sync::CancellationToken::new(),
                updates: tokio::sync::mpsc::channel(4).0,
                cwd: Arc::from(std::path::Path::new(".")),
                review: None,
            };
            let out = reg.execute(name, ctx).await.expect("ok");
            assert_eq!(
                out.content[0],
                timo_ai::Content::Text {
                    text: "ran x".into()
                }
            );
        }
        let ctx = ToolCallCtx {
            tool_call_id: "t2".into(),
            params: json!({}),
            cancel: tokio_util::sync::CancellationToken::new(),
            updates: tokio::sync::mpsc::channel(4).0,
            cwd: Arc::from(std::path::Path::new(".")),
            review: None,
        };
        let err = reg.execute("read", ctx).await.expect_err("schema fail");
        assert!(matches!(err, ToolError::InvalidParams { .. }));
    }
}
