//! 权限双闸（设计 03）：声明校验、授权存储、域名匹配与拒绝留痕。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use timo_ext::{
    BasicServices, Capabilities, ErrorCode, FsCapability, GrantState, Grants, HostServices,
    Manifest, NetCapability, PERMISSIONS_SCHEMA, PermissionStore, domain_allowed, is_valid_domain,
    validate_capabilities,
};

fn manifest(id: &str, hash: &str, capabilities: Capabilities) -> Manifest {
    serde_json::from_value(json!({
        "schema": 1,
        "id": id,
        "entry": "source/index.ts",
        "bundle": "bundle.cjs",
        "source_hash": hash,
        "bundler": "test",
        "created_at": "2026-10-26T12:00:00Z",
        "capabilities": capabilities,
    }))
    .unwrap()
}

fn caps(exec: bool, domains: &[&str]) -> Capabilities {
    Capabilities {
        exec,
        net: NetCapability {
            domains: domains.iter().map(|d| (*d).to_string()).collect(),
        },
        fs: FsCapability::default(),
    }
}

#[test]
fn domain_syntax_accepts_hosts_and_ports_only() {
    for ok in [
        "api.example.com",
        "api.example.com:8443",
        "*.corp.example",
        "localhost",
        "127.0.0.1",
    ] {
        assert!(is_valid_domain(ok), "{ok} should be valid");
    }
    for bad in [
        "",
        "*",
        "*example.com",
        "*.",
        "*.com",
        "api.*.example.com",
        "https://api.example.com",
        "api.example.com/path",
        "api.example.com:0",
        "api.example.com:99999",
        "例子.中国",
        "-bad.example.com",
    ] {
        assert!(!is_valid_domain(bad), "{bad} should be invalid");
    }
}

#[test]
fn domain_matching_is_leftmost_wildcard_and_port_exact() {
    let patterns = vec![
        "api.example.com".to_string(),
        "*.corp.example".to_string(),
        "api.example.com:8443".to_string(),
    ];
    assert!(domain_allowed(&patterns, "api.example.com", 443));
    assert!(domain_allowed(&patterns, "API.EXAMPLE.COM.", 80));
    assert!(domain_allowed(&patterns, "a.corp.example", 443));
    assert!(domain_allowed(&patterns, "a.b.corp.example", 443));
    assert!(!domain_allowed(&patterns, "corp.example", 443));
    assert!(!domain_allowed(&patterns, "other.example.com", 443));
    assert!(domain_allowed(&patterns, "api.example.com", 8443));
    // 未写端口的声明不限定端口（显式端口项才收紧）。
    assert!(domain_allowed(&patterns, "api.example.com", 9443));
    assert!(!domain_allowed(&[], "api.example.com", 443));
}

#[test]
fn whole_tree_write_or_delete_is_e403() {
    for (field, value) in [
        ("write", json!({ "write": ["**"] })),
        ("write", json!({ "write": ["/"] })),
        ("write", json!({ "write": ["C:\\"] })),
        ("delete", json!({ "delete": ["~"] })),
        ("delete", json!({ "delete": ["/**"] })),
    ] {
        let mut capabilities = caps(false, &[]);
        capabilities.fs = serde_json::from_value(value).unwrap();
        let error = validate_capabilities("wide", &capabilities).unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::E403), "field {field}");
    }
    let mut capabilities = caps(false, &[]);
    capabilities.fs.read = vec!["**".into()];
    assert!(validate_capabilities("reader", &capabilities).is_ok());
}

#[test]
fn protected_paths_are_e404() {
    for scope in [
        ".env",
        "**/.env.local",
        "**/.ssh/**",
        "**/.git/**",
        "id_rsa",
    ] {
        let mut capabilities = caps(false, &[]);
        capabilities.fs.read = vec![scope.into()];
        let error = validate_capabilities("protected", &capabilities).unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::E404), "scope {scope}");
    }
}

#[test]
fn manifest_validation_runs_capability_rules() {
    let mut manifest = manifest(
        "demo",
        "sha256:1111111111111111111111111111111111111111111111111111111111111111",
        caps(false, &["bad domain"]),
    );
    let error = manifest
        .validate(std::path::Path::new("m.json"))
        .unwrap_err();
    assert!(error.to_string().contains("bad domain"));

    manifest.capabilities = caps(false, &[]);
    manifest.capabilities.fs.write = vec!["**".into()];
    let error = manifest
        .validate(std::path::Path::new("m.json"))
        .unwrap_err();
    assert_eq!(error.code(), Some(ErrorCode::E403));
}

#[test]
fn grants_are_bound_to_source_hash() {
    let declared = manifest(
        "demo",
        "sha256:1111111111111111111111111111111111111111111111111111111111111111",
        caps(true, &["api.example.com"]),
    );
    let mut store = PermissionStore::default();
    store.grant(&declared, "2026-10-26T12:00:00Z");
    let (granted, state) = store.effective(
        "demo",
        "sha256:1111111111111111111111111111111111111111111111111111111111111111",
    );
    assert_eq!(state, GrantState::Granted);
    assert!(granted.exec);
    assert_eq!(granted.net.domains, vec!["api.example.com"]);

    let (stale, state) = store.effective(
        "demo",
        "sha256:2222222222222222222222222222222222222222222222222222222222222222",
    );
    assert_eq!(state, GrantState::Stale);
    assert!(!stale.exec);
    assert!(stale.net.domains.is_empty());

    assert_eq!(
        store
            .effective(
                "other",
                "sha256:1111111111111111111111111111111111111111111111111111111111111111"
            )
            .1,
        GrantState::Missing
    );
    assert!(store.revoke("demo"));
    assert!(!store.revoke("demo"));
}

#[test]
fn store_round_trips_and_never_widens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("permissions.json");
    let declared = manifest(
        "demo",
        "sha256:1111111111111111111111111111111111111111111111111111111111111111",
        caps(true, &["api.example.com"]),
    );
    let mut store = PermissionStore::load(&path).unwrap();
    assert!(store.grants.is_empty());
    store.grant(&declared, "2026-10-26T12:00:00Z");
    store.save(&path).unwrap();

    let loaded = PermissionStore::load(&path).unwrap();
    assert_eq!(loaded.schema, PERMISSIONS_SCHEMA);
    let grant = loaded.grants.get("demo").unwrap();
    assert_eq!(
        grant.source_hash,
        "sha256:1111111111111111111111111111111111111111111111111111111111111111"
    );
    assert!(grant.exec);
    assert_eq!(grant.net, vec!["api.example.com"]);
}

#[test]
fn denied_fetch_does_not_connect() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let services = BasicServices::new(
        std::env::temp_dir(),
        Grants::default(),
        serde_json::Value::Null,
    );
    let error = services
        .fetch(&format!("http://127.0.0.1:{port}/"))
        .unwrap_err();
    assert!(error.starts_with("E402"), "error was {error}");
    assert!(
        listener.accept().is_err(),
        "denied fetch must not open a connection"
    );
}

#[test]
fn granted_exec_runs_command() {
    let dir = tempfile::tempdir().unwrap();
    let services = BasicServices::new(
        dir.path(),
        Grants {
            exec: true,
            net_domains: vec![],
        },
        serde_json::Value::Null,
    );
    #[cfg(windows)]
    let (command, args) = ("cmd", vec!["/C".to_string(), "echo hi".to_string()]);
    #[cfg(not(windows))]
    let (command, args) = ("sh", vec!["-c".to_string(), "echo hi".to_string()]);
    let out = services
        .exec(command, &args, &serde_json::Value::Null)
        .unwrap();
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("hi"));

    let denied = BasicServices::new(dir.path(), Grants::default(), serde_json::Value::Null);
    let error = denied
        .exec(command, &args, &serde_json::Value::Null)
        .unwrap_err();
    assert!(error.starts_with("E401"));
}
