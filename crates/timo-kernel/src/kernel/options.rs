//! 门面装配选项（CLI/未来 desktop 的全部入口参数）。

use std::path::PathBuf;
use std::sync::Arc;

/// 门面装配选项（CLI/未来 desktop 的全部入口参数）。
#[derive(Clone, Default)]
pub struct KernelOptions {
    /// 数据目录覆盖（缺省 TIMO_AGENT_DIR > 平台目录）。
    pub data_dir: Option<PathBuf>,
    /// 工作目录（工具与项目设置解析基底）。
    pub work_dir: Option<PathBuf>,
    /// 模型覆盖（`--provider P --model M`）；provider 为空串 = 裸 `--model id`，按目录唯一性解析。
    pub model_override: Option<(String, String)>,
    /// 思考档位覆盖（CLI > 设置）。
    pub thinking: Option<timo_config::ThinkingLevel>,
    /// 显式凭证（`--api-key`）。
    pub api_key: Option<String>,
    /// 会话目录覆盖。
    pub session_dir: Option<PathBuf>,
    /// 单次 run 的模型请求轮数上限（`--max-turns`）。
    pub max_turns: Option<usize>,
    /// 显式扩展路径（`--extension`，可重复）。
    pub extension_paths: Vec<PathBuf>,
    /// 完全关闭扩展（`--no-extensions`）。
    pub no_extensions: bool,
    /// 是否交互式询问授权（None = 自动检测 stdin 是否 TTY）。
    pub extensions_interactive: Option<bool>,
    /// `--ext-flag NAME=VALUE`。
    pub ext_flags: Vec<(String, String)>,
    /// 不落会话文件（`--no-session`；子代理用，设计 08 §4.2）。
    pub no_session: bool,
    /// 工具允许列表（`--tools a,b,*`）：`None` = 按设置与默认激活集；列出的即最终允许集合
    /// （被默认集关掉的也恢复声明），未列出的置 Hidden。
    pub tool_allowlist: Option<Vec<String>>,
    /// 追加到系统提示尾部的文本（`--append-system-prompt`，可重复；设计 08 D52 走命令行文本）。
    pub append_system_prompt: Vec<String>,
    /// 显式 prompt 模板路径（`--prompt-template`，可重复；设计 09 §4.4）。
    pub prompt_template_paths: Vec<String>,
    /// 停用默认模板目录（`--no-prompt-templates`；显式路径仍加载）。
    pub no_prompt_templates: bool,
    /// 工具执行前审阅器（桌面注入；`None` = CLI/Phase 1/2 自动执行）。
    pub tool_review: Option<Arc<dyn timo_tools::ToolReview>>,
}
