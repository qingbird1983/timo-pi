//! 统一工具注册表（接缝 #4 检查点：来源不可区分，仅名字前缀不同）。

use std::collections::BTreeMap;

use crate::Result;
use crate::definition::{ToolDefinition, ToolOutput};
use crate::error::ToolError;

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
        };
        let err = reg.execute("read", ctx).await.expect_err("schema fail");
        assert!(matches!(err, ToolError::InvalidParams { .. }));
    }
}
