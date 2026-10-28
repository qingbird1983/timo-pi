//! 桌面设置（`<数据目录>/desktop.json`；设计 02 §4）。
//!
//! 只存非敏感项；损坏时回默认值并让界面提示一次“已重置”。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 桌面设置 schema 版本。
pub const SCHEMA: u32 = 1;

/// 外观偏好。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePref {
    /// 跟随系统。
    #[default]
    System,
    /// 浅色。
    Light,
    /// 深色。
    Dark,
}

/// 审阅开关（对新请求生效）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReviewPrefs {
    /// 文件改动前审阅。
    pub files: bool,
    /// 命令执行前审阅。
    pub commands: bool,
}

impl Default for ReviewPrefs {
    fn default() -> Self {
        Self {
            files: true,
            commands: true,
        }
    }
}

/// 窗口尺寸（重开时恢复）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowPrefs {
    /// 宽（逻辑像素）。
    pub width: f32,
    /// 高（逻辑像素）。
    pub height: f32,
}

impl Default for WindowPrefs {
    fn default() -> Self {
        Self {
            width: 1280.0,
            height: 800.0,
        }
    }
}

/// `desktop.json` 顶层结构。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopSettings {
    /// schema 版本（未来迁移用）。
    pub schema: u32,
    /// 语言（`zh-CN` / `en`）。
    pub locale: String,
    /// 外观。
    pub theme: ThemePref,
    /// 界面字号倍率（0.9~1.3）。
    pub font_scale: f32,
    /// 审阅开关。
    pub review: ReviewPrefs,
    /// 窗口尺寸。
    pub window: WindowPrefs,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            locale: "zh-CN".into(),
            theme: ThemePref::System,
            font_scale: 1.0,
            review: ReviewPrefs::default(),
            window: WindowPrefs::default(),
        }
    }
}

impl DesktopSettings {
    /// 读取桌面设置；文件不存在或损坏时回默认值。
    ///
    /// 返回值第二项表示是否因损坏而重置（界面提示一次）。
    #[must_use]
    pub fn load() -> (Self, bool) {
        let Some(path) = path() else {
            return (Self::default(), false);
        };
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return (Self::default(), false);
        };
        match serde_json::from_str::<Self>(&raw) {
            Ok(mut settings) => {
                settings.clamp();
                (settings, false)
            }
            Err(_) => (Self::default(), true),
        }
    }

    /// 原子写回（`.tmp` + rename）。
    ///
    /// # Errors
    /// 目录不可建或写入失败时返回底层错误。
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, raw)?;
        std::fs::rename(tmp, path)
    }

    /// 归一化非法值（损坏字段不阻塞启动）。
    pub fn clamp(&mut self) {
        if self.locale != "en" {
            self.locale = "zh-CN".into();
        }
        self.font_scale = self.font_scale.clamp(0.9, 1.3);
        self.window.width = self.window.width.clamp(800.0, 4000.0);
        self.window.height = self.window.height.clamp(600.0, 3000.0);
    }
}

/// 设置文件路径（`<数据目录>/desktop.json`）。
#[must_use]
pub fn path() -> Option<PathBuf> {
    timo_kernel::data_dir()
        .ok()
        .map(|dir| dir.join("desktop.json"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_stable() {
        let settings = DesktopSettings::default();
        assert_eq!(settings.locale, "zh-CN");
        assert_eq!(settings.theme, ThemePref::System);
        assert!(settings.review.files && settings.review.commands);
    }

    #[test]
    fn unknown_fields_and_bad_values_are_tolerated() {
        let parsed: DesktopSettings = serde_json::from_str(
            r#"{"schema":1,"locale":"fr","theme":"dark","extra":{"x":1},"review":{"files":false}}"#,
        )
        .expect("parse");
        let mut parsed = parsed;
        parsed.clamp();
        assert_eq!(parsed.locale, "zh-CN");
        assert_eq!(parsed.theme, ThemePref::Dark);
        assert!(!parsed.review.files);
        assert!(parsed.review.commands);
    }
}
