//! 桌面侧宿主 API v1 接线（设计 03 §3、04）。
//!
//! handler 在引擎线程运行，只把调用入队；查询、存储与 `runTool` 在独立宿主
//! 线程完成，结果经 `HostReply`（任意线程可答）回给插件。UI 副作用转成
//! [`HostUiEvent`]，由运行期在渲染帧统一应用。
//!
//! 这里不链接内核实现：白名单、参数与挡位判定全用
//! `timo_kernel::plugins::host` 的结论；模型与工具经桥接通道回内核。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nsh_engine::{HostCall, HostChannel, HostReply};
use serde_json::{Map, Value, json};
use timo_kernel::plugins::PermissionTier;
use timo_kernel::plugins::host::{self as kernel_host, Caller, PluginStorage};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio_util::sync::CancellationToken;

use crate::bridge::PluginRequest;

use super::host_worker::{WorkerContext, worker_loop};

/// 插件调用产生的宿主副作用；引擎线程只入队，UI 渲染帧再应用。
#[derive(Debug, Clone)]
pub(crate) enum HostUiEvent {
    /// 应用内通知。
    Notify {
        /// 插件 id（诊断与去重）。
        plugin: String,
        /// 级别。
        level: kernel_host::NotifyLevel,
        /// 文本。
        message: String,
    },
    /// 切换到已有会话。
    OpenSession(String),
    /// 用系统默认应用打开路径。
    OpenPath(String),
    /// 插入/提交提示词。
    SendPrompt {
        /// 文本。
        text: String,
        /// 是否直接提交。
        submit: bool,
    },
    /// 打开插件自己的面板。
    OpenPanel {
        /// 插件 id。
        plugin: String,
    },
    /// 写剪贴板。
    ClipboardWrite(String),
    /// 已消毒的主题 token 补丁。
    Theme {
        /// 插件 id。
        plugin: String,
        /// `token → value`。
        tokens: std::collections::BTreeMap<String, String>,
    },
    /// 进入插件详情的诊断卡（E743 等警告）。
    Diagnostic {
        /// 插件 id。
        plugin: String,
        /// 诊断码。
        code: String,
        /// 人读原因。
        message: String,
    },
}

/// 启动一个实例所需的一切（运行期从清单与目录拼出）。
pub(crate) struct HostConfig {
    /// 插件 id。
    pub(crate) plugin_id: String,
    /// 插件版本。
    pub(crate) version: String,
    /// 实际挡位。
    pub(crate) tier: PermissionTier,
    /// 清单声明的查询。
    pub(crate) queries: Vec<String>,
    /// 清单声明的动作。
    pub(crate) actions: Vec<String>,
    /// 清单声明的工具。
    pub(crate) tools: Vec<String>,
    /// 清单声明的 slot 类型。
    pub(crate) slots: Vec<String>,
    /// 内核数据根：插件存储落在 `<root>/plugins/<id>/storage.json`。
    pub(crate) storage_root: PathBuf,
    /// 会话目录（只读快照）。
    pub(crate) sessions_dir: PathBuf,
    /// 相对路径解析基准（`openPath` 用）。
    pub(crate) work_dir: PathBuf,
    /// 生效的 `fs.read` 作用域；`openPath` 只允许打开其中的路径。
    pub(crate) read_scopes: Vec<String>,
    /// 初始主题快照 `{appearance, tokens}`。
    pub(crate) theme: Value,
    /// UI 副作用通道。
    pub(crate) ui: UnboundedSender<HostUiEvent>,
    /// 内核请求通道。
    pub(crate) kernel: UnboundedSender<PluginRequest>,
    /// 实例取消令牌（禁用/卸载）。
    pub(crate) cancel: CancellationToken,
}

/// 一个实例的宿主面：channel 注册、主题快照与看门狗计数。
pub(crate) struct HostApi {
    plugin_id: String,
    version: String,
    tier: PermissionTier,
    queries: Vec<String>,
    actions: Vec<String>,
    tools: Vec<String>,
    theme: Arc<Mutex<Value>>,
    composer: Arc<Mutex<String>>,
    pongs: Arc<AtomicU64>,
    jobs: UnboundedSender<HostJob>,
}

impl HostApi {
    /// 建立宿主线程并准备注册；插件 id 非法时返回原因。
    ///
    /// # Errors
    /// 插件 id 不符合清单规则（存储无法隔离）→ E720 文本。
    pub(crate) fn start(config: HostConfig) -> Result<Self, String> {
        let storage = PluginStorage::new(&config.storage_root, &config.plugin_id)
            .map_err(|error| error.to_string())?;
        let theme = Arc::new(Mutex::new(config.theme.clone()));
        let composer = Arc::new(Mutex::new(String::new()));
        let pongs = Arc::new(AtomicU64::new(0));
        let (jobs, receiver) = unbounded_channel();
        let worker_ctx = WorkerContext {
            plugin_id: config.plugin_id.clone(),
            version: config.version.clone(),
            tier: config.tier,
            queries: config.queries.clone(),
            actions: config.actions.clone(),
            tools: config.tools.clone(),
            slots: config.slots.clone(),
            storage,
            sessions_dir: config.sessions_dir,
            work_dir: config.work_dir,
            read_scopes: config.read_scopes,
            theme: theme.clone(),
            composer: composer.clone(),
            ui: config.ui,
            kernel: config.kernel,
            cancel: config.cancel,
        };
        // 宿主线程随 jobs 通道关闭自行退出（View 释放后 handler 全部丢弃）。
        let _ = std::thread::Builder::new()
            .name(format!("plugin-host-{}", config.plugin_id))
            .spawn(move || worker_loop(receiver, worker_ctx));
        Ok(Self {
            plugin_id: config.plugin_id,
            version: config.version,
            tier: config.tier,
            queries: config.queries,
            actions: config.actions,
            tools: config.tools,
            theme,
            composer,
            pongs,
            jobs,
        })
    }

    /// 注册查询/动作/`theme.update`/`__pong`。
    pub(crate) fn register(&self, channel: &HostChannel) {
        let caller = CallerOwned {
            tier: self.tier,
            queries: self.queries.clone(),
            actions: self.actions.clone(),
            tools: self.tools.clone(),
        };
        for name in kernel_host::QUERIES
            .iter()
            .chain(kernel_host::ACTIONS.iter())
            .copied()
            .chain(["theme.update"])
        {
            let jobs = self.jobs.clone();
            let caller = caller.clone();
            channel.handle(name, move |call: HostCall| {
                enqueue(&jobs, &caller, call);
            });
        }
        let pongs = self.pongs.clone();
        channel.handle("__pong", move |_call: HostCall| {
            pongs.fetch_add(1, Ordering::Relaxed);
        });
    }

    /// 启动参数（`host.params`；设计 04 §1）。
    pub(crate) fn params(
        &self,
        slot_kind: &str,
        instance_id: &str,
        slot_data: Value,
        locale: &str,
    ) -> Map<String, Value> {
        let theme = self
            .theme
            .lock()
            .map(|theme| theme.clone())
            .unwrap_or(Value::Null);
        let mut params = Map::new();
        params.insert("api".into(), json!(kernel_host::API_VERSION));
        params.insert(
            "plugin".into(),
            json!({ "id": self.plugin_id, "version": self.version }),
        );
        params.insert(
            "slot".into(),
            json!({ "kind": slot_kind, "instanceId": instance_id, "data": slot_data }),
        );
        params.insert("theme".into(), theme);
        params.insert("locale".into(), json!(locale));
        params
    }

    /// 更新共享主题快照（主应用切明暗/主题时）。
    pub(crate) fn set_theme(&self, theme: Value) {
        if let Ok(mut slot) = self.theme.lock() {
            *slot = theme;
        }
    }

    /// 更新输入框快照（`composer.text` 与 `composerTrigger` 数据源）。
    pub(crate) fn set_composer_text(&self, text: &str) {
        if let Ok(mut slot) = self.composer.lock()
            && slot.as_str() != text
        {
            *slot = text.to_string();
        }
    }

    /// 记录已收到的 pong 数（看门狗比较快照）。
    pub(crate) fn pong_count(&self) -> u64 {
        self.pongs.load(Ordering::Relaxed)
    }

    /// 发 `__ping`；插件 SDK 立刻回 `__pong`。
    pub(crate) fn ping(&self, channel: &HostChannel, tick: u64) {
        let _ = channel.send("__ping", json!({ "t": tick }));
    }
}

/// 入队一次调用；post 没有回执，动作照样执行。
fn enqueue(jobs: &UnboundedSender<HostJob>, caller: &CallerOwned, call: HostCall) {
    let job = HostJob {
        caller: caller.clone(),
        name: call.name().to_string(),
        data: call.data().clone(),
        reply: call.into_reply(),
    };
    if let Err(error) = jobs.send(job)
        && let Some(reply) = error.0.reply
    {
        reply.reject("E742: host worker is unavailable");
    }
}

/// handler 侧持有的声明面副本（进入宿主线程后不再借用清单）。
#[derive(Clone)]
pub(super) struct CallerOwned {
    tier: PermissionTier,
    queries: Vec<String>,
    actions: Vec<String>,
    tools: Vec<String>,
}

impl CallerOwned {
    pub(super) fn caller(&self) -> Caller<'_> {
        Caller {
            tier: self.tier,
            queries: &self.queries,
            actions: &self.actions,
            tools: &self.tools,
        }
    }
}

/// 一次待执行的调用。
pub(super) struct HostJob {
    pub(super) caller: CallerOwned,
    pub(super) name: String,
    pub(super) data: Value,
    pub(super) reply: Option<HostReply>,
}
