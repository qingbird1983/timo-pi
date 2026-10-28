//! 桥接事件与市场快照（从 `bridge.rs` 拆出，守住单文件行数预算）。

use timo_kernel::plugins::PluginEntry;
use timo_kernel::plugins::market::{MarketIndex, MarketState};
use timo_kernel::{
    KernelEvent, Message, ReviewDecision, SessionInfo, SlashCommand, ToolReviewRequest,
    UsageSummary,
};
use tokio::sync::oneshot;

use super::ProviderInfo;

/// 市场页快照（索引 + 本地状态 + 最近错误）。
#[derive(Debug, Clone)]
pub struct MarketSnapshot {
    /// 缓存或新拉取的索引。
    pub index: Option<MarketIndex>,
    /// 索引解析警告（已格式化）。
    pub warnings: Vec<String>,
    /// 离线原因（有值表示本次刷新失败）。
    pub offline: Option<String>,
    /// 收藏/屏蔽/已安装记录。
    pub state: MarketState,
    /// 当前索引地址。
    pub index_url: Option<String>,
}

/// 内核发给 UI 的事件。
#[derive(Debug)]
pub enum BridgeEvent {
    /// 内核装配完成。
    Ready {
        /// 当前 provider/model。
        model: String,
        /// 会话列表。
        sessions: Vec<SessionInfo>,
    },
    /// 会话列表刷新。
    Sessions(Vec<SessionInfo>),
    /// 会话文件已删除。
    Deleted(String),
    /// 会话已切换（`id = None` 表示草稿）。
    Opened {
        /// 会话 id。
        id: Option<String>,
        /// 投影历史。
        history: Vec<Message>,
    },
    /// 一次 run 开始。
    Running,
    /// 一次 run 结束。
    Finished,
    /// 命令清单（内建 + 技能 + 扩展）。
    Commands(Vec<SlashCommand>),
    /// 累计用量（会话级）。
    Usage(UsageSummary),
    /// 内核事件快照。
    Kernel(KernelEvent),
    /// 工具执行前审阅请求；UI 决定后经 `respond` 回传。
    Review {
        /// 待审副作用。
        request: ToolReviewRequest,
        /// 决定通道；UI 关闭窗口即 drop → 拒绝。
        respond: oneshot::Sender<ReviewDecision>,
    },
    /// 供应商列表。
    Providers(Vec<ProviderInfo>),
    /// 某供应商密钥已保存。
    ProviderSaved(String),
    /// 某供应商密钥已移除。
    ProviderRemoved(String),
    /// 钥匙串/配置操作失败。
    ProviderError(String),
    /// 统一插件清单。
    Plugins(Vec<PluginEntry>),
    /// 插件操作失败（E7xx）。
    PluginError(String),
    /// 市场快照。
    Market(MarketSnapshot),
    /// 市场操作失败。
    MarketError(String),
    /// 打开会话失败。
    OpenFailed(String),
    /// 删除会话失败。
    DeleteFailed(String),
    /// run 失败（循环级错误）。
    RunFailed(String),
    /// 运行中收到会话切换命令。
    Busy,
    /// 事件通道落后。
    Lagged(u64),
    /// 启动失败（凭证/配置），内核不可用。
    Fatal(String),
}
