//! 思考档位（对齐 pi ThinkingLevel；设计 01 §2）。

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 思考档位；`Off` 表示不发送 thinking 参数（设计 05 §5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThinkingLevel {
    /// 关闭思考。
    Off,
    /// 最小档。
    Minimal,
    /// 低档。
    Low,
    /// 中档（设置默认值）。
    #[default]
    Medium,
    /// 高档。
    High,
    /// 超高档。
    Xhigh,
    /// 最高档。
    Max,
}

impl ThinkingLevel {
    /// 小写规范名（serde 与 CLI 共用）。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }

    /// 解析小写规范名；未知值返回 `None`（调用方决定报错或取默认）。
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "off" => Self::Off,
            "minimal" => Self::Minimal,
            "low" => Self::Low,
            "medium" => Self::Medium,
            "high" => Self::High,
            "xhigh" => Self::Xhigh,
            "max" => Self::Max,
            _ => return None,
        })
    }
}

impl std::fmt::Display for ThinkingLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ThinkingLevel {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ThinkingLevel {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown thinking level: {s}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_levels() {
        for lvl in [
            ThinkingLevel::Off,
            ThinkingLevel::Minimal,
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
            ThinkingLevel::Xhigh,
            ThinkingLevel::Max,
        ] {
            assert_eq!(ThinkingLevel::parse(lvl.as_str()), Some(lvl));
        }
    }

    #[test]
    fn parse_rejects_unknown() {
        assert_eq!(ThinkingLevel::parse("ultra"), None);
        assert_eq!(ThinkingLevel::parse("Medium"), None);
    }
}
