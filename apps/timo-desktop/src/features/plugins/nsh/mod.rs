//! NSH 嵌入 PoC 的隔离层（设计 03 §3~§4）。
//!
//! 只有这个模块 `use nsh_platform_gpui`；对外只暴露普通 Rust 数据与错误文本。
//! 默认构建不编译：整个模块由 `nsh-poc` feature 门控。

pub(crate) mod install;
mod page;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, IntoElement as _, ParentElement as _,
    Styled as _, Window, div,
};
use nsh_platform_gpui::package::{PublicKey, TrustStore};
use nsh_platform_gpui::{
    AppOptions, AppView, Appearance, LoadedPackage, accept_package, verify_package,
};

use crate::app::DesktopApp;

/// 健康检查窗口：1 秒内没有应用错误才接受新包（设计 03 §4.3）。
const HEALTH_DELAY: Duration = Duration::from_secs(1);

/// 已校验包的公开元数据。
#[derive(Debug, Clone)]
pub(crate) struct NshPackage {
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) build: u64,
    pub(crate) sha256: String,
}

/// PoC 页面上的四个加载入口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NshSource {
    /// 签名包 v1。
    V1,
    /// 签名包 v2（热更）。
    V2,
    /// 原始 TSX 字节：不是包，应被拒绝。
    Unsigned,
    /// 改了一字节的 v2：签名校验应失败。
    Tampered,
    /// 启动即抛错的签名包：健康检查应回退，旧面板保持可用。
    Crash,
}

impl NshSource {
    fn file(self) -> &'static str {
        match self {
            Self::V1 => "poc-v1.nshpkg",
            Self::V2 => "poc-v2.nshpkg",
            Self::Unsigned => "poc-unsigned.bin",
            Self::Tampered => "poc-tampered.nshpkg",
            Self::Crash => "poc-crash.nshpkg",
        }
    }

    fn signed(self) -> bool {
        matches!(self, Self::V1 | Self::V2 | Self::Crash)
    }
}

/// 一个 View 的应用错误槽：`(致命, 文本)`。
type ErrorSlot = Arc<Mutex<Option<(bool, String)>>>;

/// 已在显示的面板：View、包元数据与运行期错误槽。
type CurrentPanel = (Entity<AppView>, NshPackage, ErrorSlot);

/// 等待健康检查的新 View。
struct Pending {
    view: Entity<AppView>,
    loaded: LoadedPackage,
    package: NshPackage,
    error: ErrorSlot,
}

/// NSH 预览：当前面板、待接受面板、信任根与状态文本。
pub(crate) struct NshPreview {
    root: PathBuf,
    trust: TrustStore,
    current: Option<CurrentPanel>,
    pending: Option<Pending>,
    status: Option<(bool, String)>,
    dark: bool,
}

impl NshPreview {
    /// 建立隔离层；没有信任根或目录不可用时仍可打开页面并显示原因。
    pub(crate) fn new(dark: bool) -> Self {
        let root = poc_dir().unwrap_or_else(|| PathBuf::from("nsh-poc"));
        let mut status = None;
        let trust = match open_trust(&[root.join("trust")]) {
            Ok(trust) => trust,
            Err(error) => {
                status = Some((false, error));
                TrustStore::new()
            }
        };
        Self {
            root,
            trust,
            current: None,
            pending: None,
            status,
            dark,
        }
    }

    /// 包文件路径（数据目录下的 `packages/`）。
    pub(crate) fn package_path(&self, source: NshSource) -> PathBuf {
        self.root.join("packages").join(source.file())
    }

    /// 当前状态文本与是否成功。
    pub(crate) fn status(&self) -> Option<&(bool, String)> {
        self.status.as_ref()
    }

    /// 当前已接受的包。
    pub(crate) fn current(&self) -> Option<&NshPackage> {
        self.current.as_ref().map(|(_, package, _)| package)
    }

    fn assets_root(&self) -> PathBuf {
        self.root.join("assets")
    }

    fn set_status(&mut self, ok: bool, text: String) {
        self.status = Some((ok, text));
    }

    /// 校验签名包并暂存新 View；1 秒健康检查后再交换（[`Self::finish_pending`]）。
    pub(crate) fn load_signed(
        &mut self,
        bytes: &[u8],
        window: &mut Window,
        cx: &mut Context<DesktopApp>,
    ) -> Result<NshPackage, String> {
        if self.trust.roots().is_empty() {
            return Err(t!("nsh.no_root").to_string());
        }
        let loaded = verify_package(&self.trust, bytes, &self.assets_root())
            .map_err(|error| t!("nsh.verify_failed", error = error).to_string())?;
        let package = NshPackage {
            id: loaded.app.id.clone(),
            version: loaded.app.version.clone(),
            build: loaded.app.build,
            sha256: loaded.receipt.sha256.clone(),
        };
        let error: ErrorSlot = Arc::new(Mutex::new(None));
        let sink = error.clone();
        let options = AppOptions {
            app_id: package.id.clone(),
            title: "Timo NSH PoC".into(),
            data_dir: Some(self.root.join("data")),
            ..Default::default()
        };
        let view = cx.new(|cx| AppView::new(options, window, cx));
        view.read(cx).on_app_error(move |error| {
            if let Ok(mut slot) = sink.lock() {
                *slot = Some((error.is_fatal(), error.message.clone()));
            }
        });
        view.read(cx).run_package(&loaded);
        let appearance = self.appearance();
        view.update(cx, |view, cx| view.set_appearance(appearance, cx));
        self.pending = Some(Pending {
            view,
            loaded,
            package: package.clone(),
            error,
        });
        self.set_status(
            true,
            t!(
                "nsh.verified",
                version = package.version.clone(),
                build = package.build
            )
            .to_string(),
        );
        Ok(package)
    }

    /// 未签名/篡改包必须被拒绝；当前面板保持不动。
    pub(crate) fn expect_reject(&mut self, bytes: &[u8], source: NshSource) {
        match verify_package(&self.trust, bytes, &self.assets_root()) {
            Ok(_) => self.set_status(false, t!("nsh.unexpected_accept").to_string()),
            Err(error) => self.set_status(
                false,
                t!("nsh.rejected", file = source.file(), error = error).to_string(),
            ),
        }
    }

    /// 健康检查结束：无致命错误且引擎未报错才接受并交换面板。
    pub(crate) fn finish_pending(&mut self) -> bool {
        let Some(pending) = self.pending.take() else {
            return false;
        };
        let failure = pending.error.lock().ok().and_then(|slot| slot.clone());
        if let Some((fatal, message)) = failure {
            // 丢掉新 View 即停止它的引擎；旧面板继续显示。
            drop(pending.view);
            let text = if fatal {
                t!("nsh.pending_failed", error = message).to_string()
            } else {
                t!("nsh.pending_error", error = message).to_string()
            };
            self.set_status(false, text);
            return true;
        }
        match accept_package(&mut self.trust, &pending.loaded) {
            Ok(()) => {
                let package = pending.package;
                let previous = self
                    .current
                    .replace((pending.view, package.clone(), pending.error));
                if let Some((view, _, _)) = previous {
                    drop(view);
                }
                self.set_status(
                    true,
                    t!(
                        "nsh.accepted",
                        version = package.version.clone(),
                        sha256 = short_sha(&package.sha256)
                    )
                    .to_string(),
                );
            }
            Err(error) => {
                drop(pending.view);
                self.set_status(false, t!("nsh.accept_failed", error = error).to_string());
            }
        }
        true
    }

    /// 运行期应用错误：致命时停止面板，其余只提示。
    pub(crate) fn poll_errors(&mut self) -> bool {
        let Some((_, _, sink)) = &self.current else {
            return false;
        };
        let failure = sink.lock().ok().and_then(|mut slot| slot.take());
        let Some((fatal, message)) = failure else {
            return false;
        };
        if fatal {
            let view = self.current.take().map(|(view, _, _)| view);
            if let Some(view) = view {
                drop(view);
            }
            self.set_status(false, t!("nsh.runtime_failed", error = message).to_string());
        } else {
            self.set_status(false, t!("nsh.runtime_error", error = message).to_string());
        }
        true
    }

    /// 停止预览并丢弃两个 View。
    pub(crate) fn stop(&mut self) {
        self.pending = None;
        self.current = None;
        self.set_status(true, t!("nsh.stopped").to_string());
    }

    /// 清空已接受的构建记录（保留根公钥），下次可重新接受同一/更旧构建。
    pub(crate) fn reset_trust(&mut self) {
        self.stop();
        let dir = self.root.join("trust");
        let _ = std::fs::remove_file(dir.join("nsh-trust.json"));
        match open_trust(&[dir]) {
            Ok(trust) => {
                self.trust = trust;
                self.set_status(true, t!("nsh.trust_reset").to_string());
            }
            Err(error) => self.set_status(false, error),
        }
    }

    /// 跟随主应用明暗；只在变化时调用。
    pub(crate) fn sync_appearance(&mut self, dark: bool, cx: &mut Context<DesktopApp>) {
        if self.dark == dark {
            return;
        }
        self.dark = dark;
        let appearance = self.appearance();
        if let Some((view, _, _)) = &self.current {
            view.update(cx, |view, cx| view.set_appearance(appearance, cx));
        }
        if let Some(pending) = &self.pending {
            pending
                .view
                .update(cx, |view, cx| view.set_appearance(appearance, cx));
        }
    }

    fn appearance(&self) -> Appearance {
        if self.dark {
            Appearance::Dark
        } else {
            Appearance::Light
        }
    }

    /// 面板区域：有 View 就交给 NSH 绘制，否则显示占位说明。
    pub(crate) fn render(&self, cx: &mut Context<DesktopApp>) -> AnyElement {
        let panel = if let Some((view, _, _)) = &self.current {
            view.clone().into_any_element()
        } else if let Some(pending) = &self.pending {
            pending.view.clone().into_any_element()
        } else {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("nsh.empty").to_string()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(self.root.join("packages").display().to_string()),
                )
                .into_any_element()
        };
        v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .overflow_hidden()
            .child(panel)
            .into_any_element()
    }
}

/// 数据目录下的 PoC 根（`<agent_dir>/nsh-poc`）。
pub(crate) fn poc_dir() -> Option<PathBuf> {
    timo_kernel::data_dir().ok().map(|dir| dir.join("nsh-poc"))
}

/// 打开信任库并把各目录里的 `*.pub` 注册为根（第一个目录是持久化的信任目录）。
///
/// 一个根都没有就返回「无信任根」；额外的目录不存在时静默跳过，
/// 这样调试构建可以同时喂进开发根（设计 02 §5）。
pub(crate) fn open_trust(dirs: &[PathBuf]) -> Result<TrustStore, String> {
    let Some(first) = dirs.first() else {
        return Err(t!("nsh.no_root").to_string());
    };
    std::fs::create_dir_all(first).map_err(|error| {
        t!(
            "nsh.dir_failed",
            path = first.display().to_string(),
            error = error
        )
        .to_string()
    })?;
    let mut trust = TrustStore::open(first)
        .map_err(|error| t!("nsh.trust_failed", error = error).to_string())?;
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("pub") {
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .map_err(|error| t!("nsh.root_read_failed", error = error).to_string())?;
            let key = PublicKey::from_base64(text.trim())
                .map_err(|error| t!("nsh.root_invalid", error = error).to_string())?;
            trust.add_root(key);
        }
    }
    if trust.roots().is_empty() {
        return Err(t!("nsh.no_root").to_string());
    }
    Ok(trust)
}

fn short_sha(sha: &str) -> String {
    sha.chars().take(8).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// 未签名字节不能通过校验（不生成包、不用密钥；设计 03 §8）。
    #[test]
    fn unsigned_bytes_are_rejected() {
        let trust = TrustStore::new();
        let root = std::env::temp_dir().join(format!("timo-nsh-unsigned-{}", std::process::id()));
        let error = verify_package(&trust, b"not a signed package", &root)
            .expect_err("unsigned bytes must not verify");
        assert!(!error.is_empty());
    }
}
