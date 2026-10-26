//! 系统提示组装（设计 03 §4：身份行 + 环境节 + Available tools + Guidelines）。

use timo_tools::ToolRegistry;

/// 组装系统提示（文案结构对齐 pi；Phase 1 无 sections 机制——设计 03 §4）。
#[must_use]
pub fn build_system_prompt(cwd: &std::path::Path, registry: &ToolRegistry) -> String {
    let mut out = String::new();
    out.push_str("You are Timo-pi, a coding agent that reads files, edits files, runs commands, and reports results.\n\n");
    out.push_str(&format!(
        "# Environment\n- Working directory: {}\n- Platform: {}\n\n",
        cwd.display(),
        std::env::consts::OS
    ));
    let declared = registry.declared();
    if !declared.is_empty() {
        out.push_str("# Available tools\n");
        for t in &declared {
            if let Some(snippet) = &t.prompt_snippet {
                out.push_str(&format!("- {snippet}\n"));
            } else {
                out.push_str(&format!("- {} ({})\n", t.name, t.label));
            }
        }
        out.push('\n');
        let guidelines: Vec<&String> = declared
            .iter()
            .flat_map(|t| t.prompt_guidelines.iter())
            .collect();
        if !guidelines.is_empty() {
            out.push_str("# Guidelines\n");
            for g in guidelines {
                out.push_str(&format!("- {g}\n"));
            }
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn prompt_lists_active_tools() {
        let mut reg = timo_tools::ToolRegistry::new();
        reg.register(timo_tools::ToolDefinition {
            name: "read".into(),
            label: "Read".into(),
            description: "d".into(),
            prompt_snippet: Some("read(path): x".into()),
            prompt_guidelines: vec!["Page through big files.".into()],
            parameters: serde_json::json!({}),
            output_schema: None,
            exposure: timo_tools::Exposure::Direct,
            namespace: None,
            annotations: timo_tools::Annotations::default(),
            execution_mode: timo_tools::ExecutionMode::Parallel,
            execute: std::sync::Arc::new(|_| Box::pin(async { unreachable!("not called") })),
        });
        let prompt = build_system_prompt(std::path::Path::new("D:/x"), &reg);
        assert!(prompt.contains("Timo-pi"));
        assert!(prompt.contains("read(path): x"));
        assert!(prompt.contains("Page through big files."));
        assert!(prompt.contains("D:/x"));
    }
}
