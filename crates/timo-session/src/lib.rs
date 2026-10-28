//! timo-session：会话持久化（设计：docs/design/phase-1/04；格式定稿：architecture/05）。
//!
//! 本 crate 是磁盘格式的**唯一所有者**（architecture/01 分层原则 4）；
//! entry 流是唯一权威，其余一切（列表/恢复）都从它重建。

// 测试代码自由使用 unwrap/expect/panic（standards/04 §2）。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod entry;
mod error;
mod reader;
mod writer;

pub use entry::{
    COMPACTION_SUMMARY_PREFIX, COMPACTION_SUMMARY_SUFFIX, ENTRY_VERSION, Entry, SessionHeader,
    extract_summary, kinds, summary_message,
};
pub use error::SessionError;
pub use reader::{SessionInfo, list_sessions, project_context, read_entries, serialize_export};
pub use writer::{IdGen, SessionWriter};

/// 结果别名：本 crate 一律以 [`SessionError`] 报错。
pub type Result<T> = std::result::Result<T, SessionError>;

/// unix 毫秒 → `YYYY-MM-DD HH:MM UTC`（列表展示用；不引入第二种日期库）。
#[must_use]
pub fn format_utc_minutes(ms: i64) -> String {
    match time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000) {
        Ok(stamp) => {
            let stamp = stamp.to_offset(time::UtcOffset::UTC);
            format!(
                "{:04}-{:02}-{:02} {:02}:{:02} UTC",
                stamp.year(),
                u8::from(stamp.month()),
                stamp.day(),
                stamp.hour(),
                stamp.minute()
            )
        }
        Err(_) => format!("{ms} ms"),
    }
}

#[cfg(test)]
mod tests {
    use super::format_utc_minutes;

    #[test]
    fn formats_epoch_in_utc() {
        assert_eq!(format_utc_minutes(0), "1970-01-01 00:00 UTC");
        assert_eq!(
            format_utc_minutes(1_700_000_000_000),
            "2023-11-14 22:13 UTC"
        );
        // 负毫秒（1970 之前）按 UTC 正常回推。
        assert_eq!(format_utc_minutes(-86_400_000), "1969-12-31 00:00 UTC");
    }
}
