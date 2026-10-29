use super::*;
use crate::writer::{IdGen, SessionWriter};
use std::sync::Arc;
use timo_ai::user_text;
use timo_config::FixedClock;

fn setup(name: &str) -> (PathBuf, Arc<IdGen>) {
    let d = std::env::temp_dir().join(format!("timo-sess-read-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    (d, Arc::new(IdGen::new()))
}

#[test]
fn roundtrip_and_projection() {
    let (dir, idgen) = setup("rt");
    let mut w =
        SessionWriter::create(&dir, "D:/demo", None, Arc::new(FixedClock::new(1)), idgen).unwrap();
    w.append_raw(
        kinds::MESSAGE,
        serde_json::to_value(user_text("hello", 5)).unwrap(),
    )
    .unwrap();
    w.append_raw("custom.demo", serde_json::json!({"x": 1}))
        .unwrap();
    w.append_raw(
        kinds::MESSAGE,
        serde_json::to_value(user_text("again", 6)).unwrap(),
    )
    .unwrap();
    let entries = read_entries(w.path()).unwrap();
    assert_eq!(entries.len(), 4);
    let msgs = project_context(&entries);
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].timestamp(), 5);
}

#[test]
fn projection_uses_the_latest_compaction_checkpoint() {
    let (dir, idgen) = setup("compaction");
    let mut w =
        SessionWriter::create(&dir, "D:/demo", None, Arc::new(FixedClock::new(1)), idgen).unwrap();
    for (i, text) in ["one", "two", "three", "four", "five"].iter().enumerate() {
        w.append_raw(
            kinds::MESSAGE,
            serde_json::to_value(user_text(text, i as i64)).unwrap(),
        )
        .unwrap();
    }
    w.append_raw(
        kinds::COMPACTION,
        serde_json::json!({"summary": "checkpoint", "keep_recent_messages": 2}),
    )
    .unwrap();
    w.append_raw(
        kinds::MESSAGE,
        serde_json::to_value(user_text("six", 6)).unwrap(),
    )
    .unwrap();

    let msgs = project_context(&read_entries(w.path()).unwrap());
    assert_eq!(msgs.len(), 4);
    assert_eq!(crate::extract_summary(&msgs[0]), Some("checkpoint"));
    assert_eq!(msgs[1].timestamp(), 3);
    assert_eq!(msgs[2].timestamp(), 4);
    assert_eq!(msgs[3].timestamp(), 6);
}

#[test]
fn projection_falls_back_when_the_summary_is_empty() {
    let (dir, idgen) = setup("compaction-bad");
    let mut w =
        SessionWriter::create(&dir, "D:/demo", None, Arc::new(FixedClock::new(1)), idgen).unwrap();
    w.append_raw(
        kinds::MESSAGE,
        serde_json::to_value(user_text("one", 1)).unwrap(),
    )
    .unwrap();
    w.append_raw(kinds::COMPACTION, serde_json::json!({"summary": ""}))
        .unwrap();
    w.append_raw(
        kinds::MESSAGE,
        serde_json::to_value(user_text("two", 2)).unwrap(),
    )
    .unwrap();

    let msgs = project_context(&read_entries(w.path()).unwrap());
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].timestamp(), 1);
    assert_eq!(msgs[1].timestamp(), 2);
}

#[test]
fn truncated_last_line_dropped() {
    let (dir, idgen) = setup("trunc");
    let mut w =
        SessionWriter::create(&dir, "D:/x", None, Arc::new(FixedClock::new(1)), idgen).unwrap();
    w.append_raw(kinds::MESSAGE, serde_json::json!({"role": "user"}))
        .unwrap();
    let path = w.path().to_path_buf();
    drop(w);
    let mut raw = std::fs::read_to_string(&path).unwrap();
    raw.push_str(r#"{"v":1,"id":"ZZ","ts":"t","parent_id":null,"type":"message","payload":{"rol"#);
    std::fs::write(&path, raw).unwrap();
    let entries = read_entries(&path).unwrap();
    assert_eq!(entries.len(), 2); // header + 完整 message，半行被丢弃
}

#[test]
fn mid_file_corruption_is_error() {
    let (dir, idgen) = setup("mid");
    let mut w =
        SessionWriter::create(&dir, "D:/x", None, Arc::new(FixedClock::new(1)), idgen).unwrap();
    w.append_raw(kinds::MESSAGE, serde_json::json!({"role": "user"}))
        .unwrap();
    w.append_raw(kinds::MESSAGE, serde_json::json!({"role": "user"}))
        .unwrap();
    let path = w.path().to_path_buf();
    drop(w);
    let raw = std::fs::read_to_string(&path).unwrap();
    let mut lines: Vec<String> = raw.lines().map(String::from).collect();
    lines.insert(1, "{corrupt".into());
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    let err = read_entries(&path).unwrap_err();
    assert!(matches!(err, SessionError::CorruptEntry { line: 2, .. }));
}

#[test]
fn list_scans_headers() {
    let (dir, idgen) = setup("list");
    let mut w =
        SessionWriter::create(&dir, "D:/demo", None, Arc::new(FixedClock::new(1)), idgen).unwrap();
    w.append_raw(
        kinds::MESSAGE,
        serde_json::to_value(user_text("first prompt here", 5)).unwrap(),
    )
    .unwrap();
    let infos = list_sessions(&dir);
    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].cwd, "D:/demo");
    assert_eq!(infos[0].first_message, "first prompt here");
    assert_eq!(infos[0].message_count, 1);
}

#[test]
fn list_scans_partitions_and_dedupes() {
    let (dir, idgen) = setup("list-partition");
    let partition = dir.join("--D--demo--");
    let mut w = SessionWriter::create(
        &partition,
        "D:/demo",
        None,
        Arc::new(FixedClock::new(1)),
        idgen,
    )
    .unwrap();
    w.append_raw(
        kinds::MESSAGE,
        serde_json::to_value(user_text("partition prompt", 5)).unwrap(),
    )
    .unwrap();
    let path = w.path().to_path_buf();
    drop(w);
    // 同一会话又出现在旧平铺位置：列表只保留一份
    std::fs::copy(&path, dir.join(path.file_name().unwrap())).unwrap();

    let infos = list_sessions(&dir);
    assert_eq!(infos.len(), 1);
    assert!(infos[0].path.starts_with(&partition));
}

/// 分区副本比平铺副本更旧时仍要保留分区副本：证明优先级来自布局而不是
/// mtime（`std::fs::copy` 在 Windows 保 mtime、Linux 不保，只靠 mtime 会随平台翻脸）。
#[test]
fn the_partition_copy_wins_even_when_it_is_older() {
    let (dir, idgen) = setup("list-partition-older");
    let partition = dir.join("--D--demo--");
    let mut w = SessionWriter::create(
        &partition,
        "D:/demo",
        None,
        Arc::new(FixedClock::new(1)),
        idgen,
    )
    .unwrap();
    w.append_raw(
        kinds::MESSAGE,
        serde_json::to_value(user_text("partition prompt", 5)).unwrap(),
    )
    .unwrap();
    let path = w.path().to_path_buf();
    drop(w);
    let flat = dir.join(path.file_name().unwrap());
    std::fs::copy(&path, &flat).unwrap();
    let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(60 * 60);
    let handle = std::fs::File::options().write(true).open(&path).unwrap();
    handle.set_modified(long_ago).unwrap();
    drop(handle);

    let infos = list_sessions(&dir);
    assert_eq!(infos.len(), 1);
    assert!(
        infos[0].path.starts_with(&partition),
        "expected the partition copy, got {}",
        infos[0].path.display()
    );
}

#[test]
fn export_serializes_the_leaf_chain() {
    let (dir, idgen) = setup("export");
    let mut w =
        SessionWriter::create(&dir, "D:/demo", None, Arc::new(FixedClock::new(7)), idgen).unwrap();
    w.append_raw(
        kinds::MESSAGE,
        serde_json::to_value(user_text("one", 5)).unwrap(),
    )
    .unwrap();
    w.append_raw("custom.demo", serde_json::json!({"x": 1}))
        .unwrap();
    w.append_raw(
        kinds::MESSAGE,
        serde_json::to_value(user_text("two", 6)).unwrap(),
    )
    .unwrap();
    let entries = read_entries(w.path()).unwrap();
    let body = serialize_export(&entries).unwrap();
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), entries.len());
    let first: Entry = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first.kind, kinds::HEADER);
    let last: Entry = serde_json::from_str(lines[lines.len() - 1]).unwrap();
    assert_eq!(
        Some(last.id.as_str()),
        entries.last().map(|e| e.id.as_str())
    );
}
