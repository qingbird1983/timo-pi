//! 市场索引解析、筛选、缓存与本地状态。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use timo_kernel::plugins::market::{
    MarketQuery, MarketSort, MarketState, MarketTab, RefreshOutcome, filter, index_url,
    load_cached, load_state, parse_index, refresh, set_index_url, toggle_blocked, toggle_favorite,
    validate_index_url, write_cache,
};

fn plugin(id: &str, name: &str, categories: &[&str]) -> serde_json::Value {
    json!({
        "id": id,
        "name": name,
        "kind": "gui",
        "version": "1.0.0",
        "author": { "name": "timo", "url": "https://example.com" },
        "description": format!("{name} 的描述"),
        "categories": categories,
        "screenshots": ["https://example.com/shot.png"],
        "stats": { "downloads_30d": 10, "stars": 2 },
        "requires": { "timo": ">=0.4.0", "api": 1 },
        "package": {
            "url": format!("https://example.com/{id}.nshpkg"),
            "sha256": "a".repeat(64)
        },
        "discussion": "https://example.com/discuss"
    })
}

fn index(plugins: Vec<serde_json::Value>) -> String {
    json!({
        "schema": 1,
        "market_version": "1.0",
        "updated_at": "2026-10-28T12:00:00Z",
        "plugins": plugins
    })
    .to_string()
}

fn state() -> MarketState {
    MarketState::default()
}

#[test]
fn parses_and_filters_entries() {
    let text = index(vec![
        plugin("session-search", "会话搜索", &["会话与消息"]),
        plugin("warm-gray", "暖灰主题", &["主题与外观"]),
    ]);
    let load = parse_index(&text).unwrap();
    assert_eq!(load.index.plugins.len(), 2);
    assert!(load.warnings.is_empty());

    let all = filter(&load.index, &state(), &MarketQuery::default());
    assert_eq!(all.len(), 2);

    let theme = filter(
        &load.index,
        &state(),
        &MarketQuery {
            tab: MarketTab::Theme,
            ..MarketQuery::default()
        },
    );
    assert_eq!(theme[0].id, "warm-gray");

    let searched = filter(
        &load.index,
        &state(),
        &MarketQuery {
            search: "搜索".into(),
            ..MarketQuery::default()
        },
    );
    assert_eq!(searched.len(), 1);

    let sorted = filter(
        &load.index,
        &state(),
        &MarketQuery {
            sort: MarketSort::Downloads,
            ..MarketQuery::default()
        },
    );
    assert_eq!(sorted.len(), 2);
}

#[test]
fn blocked_and_favorites_are_local_views() {
    let text = index(vec![
        plugin("a-plugin", "A", &["UI 增强"]),
        plugin("b-plugin", "B", &["UI 增强"]),
    ]);
    let load = parse_index(&text).unwrap();
    let mut state = MarketState::default();
    state.favorites.push("b-plugin".into());
    state.blocked.push("a-plugin".into());

    let discover = filter(&load.index, &state, &MarketQuery::default());
    assert_eq!(discover.len(), 1);
    assert_eq!(discover[0].id, "b-plugin");

    let favorites = filter(
        &load.index,
        &state,
        &MarketQuery {
            tab: MarketTab::Favorites,
            ..MarketQuery::default()
        },
    );
    assert_eq!(favorites.len(), 1);

    let blocked = filter(
        &load.index,
        &state,
        &MarketQuery {
            tab: MarketTab::Blocked,
            ..MarketQuery::default()
        },
    );
    assert_eq!(blocked[0].id, "a-plugin");
}

#[test]
fn rejects_unknown_schema() {
    let text = json!({ "schema": 2, "plugins": [] }).to_string();
    assert_eq!(parse_index(&text).unwrap_err().code(), "E750");
}

#[test]
fn skips_invalid_entries_and_keeps_the_rest() {
    let mut bad_sha = plugin("bad-sha", "Bad", &["UI 增强"]);
    bad_sha["package"]["sha256"] = json!("xyz");
    let mut bad_url = plugin("bad-url", "Bad URL", &["UI 增强"]);
    bad_url["package"]["url"] = json!("http://example.com/x.nshpkg");
    let text = index(vec![
        bad_sha,
        bad_url,
        plugin("good", "Good", &["UI 增强"]),
        plugin("unknown-cat", "Category", &["不存在"]),
    ]);
    let load = parse_index(&text).unwrap();
    let ids: Vec<_> = load
        .index
        .plugins
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    assert_eq!(ids, ["good", "unknown-cat"]);
    assert!(load.index.plugins[1].categories.is_empty());
    assert!(load.warnings.iter().any(|warning| warning.code == "E752"));
    assert_eq!(load.warnings.len(), 3);
}

#[test]
fn cache_roundtrip_and_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let text = index(vec![plugin("session-search", "搜索", &["会话与消息"])]);
    let load = parse_index(&text).unwrap();
    write_cache(dir.path(), &load.index).unwrap();
    let cached = load_cached(dir.path()).unwrap().unwrap();
    assert_eq!(cached.index, load.index);

    std::fs::write(
        dir.path().join("market/index.json"),
        "{\"schema\":2,\"plugins\":[]}",
    )
    .unwrap();
    assert_eq!(load_cached(dir.path()).unwrap_err().code(), "E750");
}

/// 拉取失败（离线）不覆盖上一份索引缓存。
#[tokio::test]
async fn offline_refresh_keeps_the_cache() {
    let dir = tempfile::tempdir().unwrap();
    let text = index(vec![plugin("session-search", "搜索", &["会话与消息"])]);
    let load = parse_index(&text).unwrap();
    write_cache(dir.path(), &load.index).unwrap();

    let outcome = refresh(dir.path(), "https://127.0.0.1:1/market.json")
        .await
        .unwrap();
    assert!(matches!(outcome, RefreshOutcome::Offline(_)));
    assert_eq!(load_cached(dir.path()).unwrap().unwrap().index, load.index);
}

#[test]
fn favorites_and_blocked_persist() {
    let dir = tempfile::tempdir().unwrap();
    assert!(toggle_favorite(dir.path(), "session-search").unwrap());
    assert!(toggle_blocked(dir.path(), "noisy").unwrap());
    let state = load_state(dir.path());
    assert_eq!(state.favorites, ["session-search"]);
    assert_eq!(state.blocked, ["noisy"]);

    assert!(!toggle_favorite(dir.path(), "session-search").unwrap());
    assert!(load_state(dir.path()).favorites.is_empty());
}

#[test]
fn index_url_lives_in_settings_and_preserves_other_keys() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("settings.json"),
        json!({ "default_provider": "deepseek" }).to_string(),
    )
    .unwrap();
    assert!(index_url(dir.path()).is_none());

    set_index_url(dir.path(), Some("https://example.com/index.json")).unwrap();
    assert_eq!(
        index_url(dir.path()).as_deref(),
        Some("https://example.com/index.json")
    );
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(raw["default_provider"], "deepseek");

    assert_eq!(
        set_index_url(dir.path(), Some("http://example.com/index.json"))
            .unwrap_err()
            .code(),
        "E750"
    );
    set_index_url(dir.path(), None).unwrap();
    assert!(index_url(dir.path()).is_none());
}

#[test]
fn url_validation_requires_https() {
    assert!(validate_index_url("https://example.com/market.json"));
    assert!(!validate_index_url("http://example.com/market.json"));
    assert!(!validate_index_url("https://"));
    assert!(!validate_index_url("not a url"));
}
