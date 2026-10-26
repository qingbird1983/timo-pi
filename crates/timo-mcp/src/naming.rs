//! MCP 工具命名（设计 06 §1.3；对齐 pi createMcpToolName）。

use sha2::{Digest, Sha256};

/// `mcp__<server>__<tool>`：清洗非法字符（含 server 名中的 `-`）；超 64 字符或
/// 冲突 → `name[0:56]_` + `sha256("server\0tool")[0:8]`。
#[must_use]
pub fn mcp_tool_name(server: &str, tool: &str) -> String {
    let clean = |s: &str| {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>()
    };
    let base = format!("mcp__{}__{}", clean(server), clean(tool));
    if base.chars().count() <= 64 {
        base
    } else {
        short_hash_name(
            &format!("mcp__{}__{}", clean(server), clean(tool)),
            server,
            tool,
        )
    }
}

/// 冲突/超长时的确定性短名（注册时若发现冲突也走这里）。
#[must_use]
pub fn short_hash_name(_truncated: &str, server: &str, tool: &str) -> String {
    let mut h = Sha256::new();
    h.update(server.as_bytes());
    h.update([0]);
    h.update(tool.as_bytes());
    let digest = h.finalize();
    let hex: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    // 总长 ≤64（供应商上限）：前缀 55 + `_` + 8 hex
    let prefix: String = _truncated.chars().take(55).collect();
    format!("{prefix}_{hex}")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_non_alnum() {
        assert_eq!(
            mcp_tool_name("docs-server", "search.all"),
            "mcp__docs_server__search_all"
        );
    }

    #[test]
    fn long_names_hashed_deterministically() {
        let server = "s".repeat(40);
        let tool = "t".repeat(40);
        let a = mcp_tool_name(&server, &tool);
        let b = mcp_tool_name(&server, &tool);
        assert_eq!(a, b);
        assert!(a.chars().count() <= 64);
        assert!(a.contains('_'));
    }
}
