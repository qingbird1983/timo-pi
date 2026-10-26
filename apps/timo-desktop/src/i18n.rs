//! 界面语言：默认中文，可切英文（P3-D6）。文案全部在 `locales/`，代码不写死。

/// 默认语言。
pub const DEFAULT: &str = "zh-CN";
/// 英文语言标签。
pub const ENGLISH: &str = "en";

/// 设置默认语言；进程启动时调用一次。
pub fn init() {
    rust_i18n::set_locale(DEFAULT);
}

/// 切换界面语言。
pub fn set(locale: &str) {
    rust_i18n::set_locale(locale);
}

/// 当前语言标签。
pub fn current() -> String {
    rust_i18n::locale().to_string()
}

/// 读取当前语言的文案（测试与诊断用；界面代码直接用 `t!`）。
pub fn text(key: &str) -> String {
    t!(key).to_string()
}
