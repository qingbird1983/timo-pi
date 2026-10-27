//! 系统提示装配与重载（设计 phase-3.5/01 §7）。

use std::path::PathBuf;

use timo_agent::KernelEvent;
use timo_tools::ToolDefinition;

use super::Kernel;
use crate::prompt;

impl Kernel {
    /// 装配系统提示：上下文文件 + SYSTEM/APPEND + 工具 loadout（设计 01）。
    ///
    /// 节名非法不中止装配：发 `Error` 事件并降级为身份行。
    pub(super) fn build_prompt(&self) -> (String, Vec<prompt::ResourceWarning>) {
        let (context_files, warnings) =
            prompt::discover_context_files(&self.work_dir, &self.data_dir);
        let custom = prompt::custom_prompt(&self.data_dir, &self.work_dir);
        let append = prompt::append_system_prompt(&self.data_dir, &self.work_dir);
        let declared: Vec<&ToolDefinition> = self.registry.declared();
        let skills: Vec<prompt::Skill> = Vec::new();
        let docs = docs_dir();
        let inputs = prompt::PromptInputs {
            cwd: &self.work_dir,
            custom_prompt: custom.as_deref(),
            append_system_prompt: append.as_deref(),
            tools: &declared,
            prompt_guidelines: &[],
            context_files: &context_files,
            skills: &skills,
            docs_dir: docs.as_deref(),
            extra_sections: &[],
        };
        match prompt::assemble(&inputs) {
            Ok(assembled) => (assembled.render(), warnings),
            Err(error) => {
                self.bus.emit(KernelEvent::Error {
                    context: format!("prompt: {error}"),
                });
                (prompt::fallback().render(), warnings)
            }
        }
    }

    /// 当前系统提示（装配后有效；此前为空串）。
    #[must_use]
    pub fn system_prompt(&self) -> &str {
        &self.prompt_text
    }

    /// 上下文装载告警（`/reload` 后刷新）。
    #[must_use]
    pub fn resource_warnings(&self) -> &[crate::ResourceWarning] {
        &self.prompt_warnings
    }

    /// 重新发现上下文并重建提示；内容变化时下次运行重建 agent。
    pub fn reload_resources(&mut self) {
        let (text, warnings) = self.build_prompt();
        self.prompt_warnings = warnings;
        if text != self.prompt_text {
            self.prompt_text = text;
            self.agent = None;
        }
    }
}

/// 文档索引目录：`TIMO_DOCS_DIR` 优先，其次可执行文件旁的 `docs/`；都没有则不生成该节。
fn docs_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("TIMO_DOCS_DIR") {
        let path = PathBuf::from(dir);
        if path.is_dir() {
            return Some(path);
        }
    }
    let candidate = std::env::current_exe().ok()?.parent()?.join("docs");
    candidate.is_dir().then_some(candidate)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::kernel::KernelOptions;

    #[tokio::test]
    async fn reload_picks_up_a_new_context_file() {
        let data = std::env::temp_dir().join(format!("timo-kernel-prompt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let work = data.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let mut kernel = Kernel::new(KernelOptions {
            data_dir: Some(data.clone()),
            work_dir: Some(work.clone()),
            model_override: Some(("anthropic".into(), "claude-sonnet-4-5".into())),
            api_key: Some("test-key".into()),
            ..Default::default()
        })
        .await
        .expect("assemble");

        kernel.tools().await;
        assert!(kernel.system_prompt().contains("Timo-pi"));
        assert!(!kernel.system_prompt().contains("reload-marker"));

        std::fs::write(work.join("AGENTS.md"), "reload-marker").unwrap();
        kernel.reload_resources();
        assert!(kernel.system_prompt().contains("reload-marker"));
        assert!(kernel.resource_warnings().is_empty());

        let _ = std::fs::remove_dir_all(&data);
    }
}
