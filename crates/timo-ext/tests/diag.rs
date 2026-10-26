//! 诊断报告与错误码集成测试（设计 02 §5/§6）。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use timo_ext::{
    DegradeClass, DegradedItem, DiagnosticEntry, DiagnosticReport, ErrorCode, ExtensionDiagnostic,
    Phase, Status,
};

#[test]
fn summary_counts_statuses() {
    let mut report = DiagnosticReport::new("2026-10-26T00:00:00Z", "0.1.0");
    report.push(ExtensionDiagnostic::new("a", "/a", Status::Loaded));
    report.push(ExtensionDiagnostic::new("b", "/b", Status::Degraded));
    report.push(ExtensionDiagnostic::new("c", "/c", Status::Failed));
    let summary = report.summary();
    assert_eq!(
        (summary.loaded, summary.degraded, summary.failed),
        (1, 1, 1)
    );
}

#[test]
fn json_contains_code_and_summary() {
    let mut report = DiagnosticReport::new("2026-10-26T00:00:00Z", "0.1.0");
    let mut ext = ExtensionDiagnostic::new("node-fs", "/x", Status::Failed);
    ext.errors.push(DiagnosticEntry::new(
        ErrorCode::E104,
        "node:fs is not available",
    ));
    report.push(ext);
    let json = report.to_json_pretty().unwrap();
    assert!(json.contains("\"E104\""));
    assert!(json.contains("\"failed\": 1"));
    assert!(json.contains("\"phase\": \"pack\""));
}

#[test]
fn human_table_has_header_and_summary() {
    let mut report = DiagnosticReport::new("2026-10-26T00:00:00Z", "0.1.0");
    let mut ext = ExtensionDiagnostic::new("pure-tui", "/x", Status::Degraded);
    ext.degraded.push(DegradedItem {
        member: "ctx.ui.setWidget".into(),
        class: DegradeClass::Tui,
        reason: "TUI member is a no-op".into(),
    });
    report.push(ext);
    let text = report.render_human();
    assert!(text.contains("ID"));
    assert!(text.contains("pure-tui"));
    assert!(text.contains("degraded=1"));
}

#[test]
fn empty_report_is_explicit() {
    let report = DiagnosticReport::new("2026-10-26T00:00:00Z", "0.1.0");
    assert_eq!(report.render_human(), "（无扩展）\n");
}

#[test]
fn codes_are_unique_and_phase_mapped() {
    use std::collections::HashSet;
    let codes = [
        ErrorCode::E101,
        ErrorCode::E102,
        ErrorCode::E103,
        ErrorCode::E104,
        ErrorCode::E105,
        ErrorCode::E106,
        ErrorCode::E107,
        ErrorCode::E108,
        ErrorCode::E109,
        ErrorCode::E110,
        ErrorCode::E201,
        ErrorCode::E202,
        ErrorCode::E203,
        ErrorCode::E204,
        ErrorCode::E205,
        ErrorCode::E206,
        ErrorCode::E301,
        ErrorCode::E302,
        ErrorCode::E303,
        ErrorCode::E304,
        ErrorCode::E305,
        ErrorCode::E306,
        ErrorCode::E401,
        ErrorCode::E402,
        ErrorCode::E403,
        ErrorCode::E404,
        ErrorCode::E405,
        ErrorCode::E406,
    ];
    let unique: HashSet<_> = codes.iter().map(|c| c.as_str()).collect();
    assert_eq!(unique.len(), codes.len());
    assert_eq!(ErrorCode::E101.phase(), Phase::Discover);
    assert_eq!(ErrorCode::E104.phase(), Phase::Pack);
    assert_eq!(ErrorCode::E204.phase(), Phase::Load);
    assert_eq!(ErrorCode::E301.phase(), Phase::Runtime);
    assert_eq!(ErrorCode::E402.phase(), Phase::Permission);
}
