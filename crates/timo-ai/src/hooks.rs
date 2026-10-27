//! 供应商钩子（设计 04 §2.2）：请求体、请求头与响应头的可改写点。
//!
//! 钩子由扩展宿主实现；无钩子时适配器行为与 Phase 1 逐字节一致。

use async_trait::async_trait;
use serde_json::Value;

/// 供应商钩子；所有方法默认空实现。
#[async_trait]
pub trait ProviderHooks: Send + Sync {
    /// 请求体构建后、发送前；返回替换体（`None` = 不改）。
    async fn before_request(&self, payload: Value) -> Option<Value> {
        let _ = payload;
        None
    }

    /// 请求头组装后；返回补丁表（值为 `None` 表示删除该键）。
    async fn before_headers(
        &self,
        headers: Vec<(String, String)>,
    ) -> Option<Vec<(String, Option<String>)>> {
        let _ = headers;
        None
    }

    /// 收到成功响应、消费流前（只通知）。
    async fn after_response(&self, status: u16, headers: Vec<(String, String)>) {
        let _ = (status, headers);
    }
}

/// 应用头补丁：同名键覆盖或删除，未出现的键沿用原顺序。
#[must_use]
pub fn apply_header_patch(
    mut headers: Vec<(String, String)>,
    patch: Vec<(String, Option<String>)>,
) -> Vec<(String, String)> {
    for (name, value) in patch {
        headers.retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
        if let Some(value) = value {
            headers.push((name, value));
        }
    }
    headers
}

/// 叠加一层请求头：同名（大小写不敏感）由 `over` 覆盖，其余保持原顺序（设计 10 §4.1）。
#[must_use]
pub fn merge_headers(
    mut base: Vec<(String, String)>,
    over: &[(String, String)],
) -> Vec<(String, String)> {
    for (name, value) in over {
        base.retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
        base.push((name.clone(), value.clone()));
    }
    base
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn patch_overrides_and_deletes() {
        let headers = vec![
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string()),
        ];
        let patch = vec![
            ("A".to_string(), Some("9".to_string())),
            ("b".to_string(), None),
            ("c".to_string(), Some("3".to_string())),
        ];
        let out = apply_header_patch(headers, patch);
        assert_eq!(
            out,
            vec![
                ("A".to_string(), "9".to_string()),
                ("c".to_string(), "3".to_string())
            ]
        );
    }

    #[test]
    fn merge_headers_overrides_case_insensitively_and_keeps_order() {
        let base = vec![
            ("A".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string()),
        ];
        let over = vec![
            ("a".to_string(), "9".to_string()),
            ("c".to_string(), "3".to_string()),
        ];
        assert_eq!(
            merge_headers(base, &over),
            vec![
                ("b".to_string(), "2".to_string()),
                ("a".to_string(), "9".to_string()),
                ("c".to_string(), "3".to_string()),
            ]
        );
        let only = vec![("x".to_string(), "1".to_string())];
        assert_eq!(merge_headers(only.clone(), &[]), only);
    }
}
