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
    pub(super) fn build_prompt(
        &self,
    ) -> (String, Vec<prompt::Skill>, Vec<prompt::ResourceWarning>) {
        let (context_files, mut warnings) =
            prompt::discover_context_files(&self.work_dir, &self.data_dir);
        let roots = prompt::skill_roots(&self.data_dir, &self.work_dir, &self.skill_paths);
        let skill_set = prompt::discover(&roots);
        warnings.extend(skill_set.warnings);
        let skills = skill_set.skills;
        let custom = prompt::custom_prompt(&self.data_dir, &self.work_dir);
        let mut append =
            prompt::append_system_prompt(&self.data_dir, &self.work_dir).unwrap_or_default();
        // `--append-system-prompt` 的命令行文本接在同一节尾部（设计 08 §4.2 / D52）。
        for extra in &self.append_prompt {
            if extra.is_empty() {
                continue;
            }
            if !append.is_empty() {
                append.push_str("\n\n");
            }
            append.push_str(extra);
        }
        let declared: Vec<&ToolDefinition> = self.registry.declared();
        let docs = docs_dir();
        let inputs = prompt::PromptInputs {
            cwd: &self.work_dir,
            custom_prompt: custom.as_deref(),
            append_system_prompt: (!append.is_empty()).then_some(append.as_str()),
            tools: &declared,
            prompt_guidelines: &[],
            context_files: &context_files,
            skills: &skills,
            docs_dir: docs.as_deref(),
            extra_sections: &[],
        };
        match prompt::assemble(&inputs) {
            Ok(assembled) => (assembled.render(), skills, warnings),
            Err(error) => {
                self.bus.emit(KernelEvent::Error {
                    context: format!("prompt: {error}"),
                });
                (prompt::fallback().render(), skills, warnings)
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

    /// 重新发现上下文与技能并重建提示；内容变化时下次运行重建 agent。
    pub fn reload_resources(&mut self) {
        let (text, skills, warnings) = self.build_prompt();
        self.prompt_warnings = warnings;
        self.skills = skills;
        if text != self.prompt_text {
            self.prompt_text = text;
            self.agent = None;
        }
    }

    /// 命令清单：内建 + 技能 + 扩展（同名先到先得）。
    #[must_use]
    pub fn slash_commands(&self) -> Vec<crate::SlashCommand> {
        crate::commands::merge(
            crate::commands::builtin_commands(),
            &self.skills,
            self.skill_commands_enabled,
            &self.extension_commands(),
        )
    }

    /// `/skill:name args` 展开；其它文本原样返回。
    #[must_use]
    pub fn expand_input(&self, text: &str) -> String {
        prompt::expand_command(text, &self.skills).unwrap_or_else(|| text.to_string())
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

    fn write_skill(data: &std::path::Path, name: &str) {
        let dir = data.join("skills").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: does {name}\n---\nBody of {name}.\n"),
        )
        .unwrap();
    }

    async fn kernel_at(data: &std::path::Path) -> Kernel {
        let work = data.join("work");
        std::fs::create_dir_all(&work).unwrap();
        Kernel::new(KernelOptions {
            data_dir: Some(data.to_path_buf()),
            work_dir: Some(work),
            model_override: Some(("anthropic".into(), "claude-sonnet-4-5".into())),
            api_key: Some("test-key".into()),
            ..Default::default()
        })
        .await
        .expect("assemble")
    }

    #[tokio::test]
    async fn skills_enter_prompt_commands_and_expansion() {
        let data = std::env::temp_dir().join(format!("timo-kernel-skill-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        write_skill(&data, "pdf");
        let mut kernel = kernel_at(&data).await;
        kernel.tools().await;

        let prompt = kernel.system_prompt();
        assert!(prompt.contains("<available_skills>"));
        assert!(prompt.contains("<name>pdf</name>"));

        let commands = kernel.slash_commands();
        assert!(commands.iter().any(|command| command.name == "skill:pdf"));

        let expanded = kernel.expand_input("/skill:pdf go");
        assert!(expanded.contains("Body of pdf."));
        assert!(expanded.ends_with("\n\ngo"));

        let _ = std::fs::remove_dir_all(&data);
    }

    #[tokio::test]
    async fn disabled_skill_commands_hide_the_list_but_keep_expansion() {
        let data = std::env::temp_dir().join(format!("timo-kernel-noskill-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(
            data.join("settings.json"),
            r#"{"enable_skill_commands": false}"#,
        )
        .unwrap();
        write_skill(&data, "pdf");
        let mut kernel = kernel_at(&data).await;
        kernel.tools().await;

        assert!(
            !kernel
                .slash_commands()
                .iter()
                .any(|command| command.name == "skill:pdf")
        );
        assert!(
            kernel
                .expand_input("/skill:pdf go")
                .contains("Body of pdf.")
        );

        let _ = std::fs::remove_dir_all(&data);
    }
}
