#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;

use super::*;

fn caller<'a>(
    tier: PermissionTier,
    queries: &'a [String],
    actions: &'a [String],
    tools: &'a [String],
) -> Caller<'a> {
    Caller {
        tier,
        queries,
        actions,
        tools,
    }
}

fn names(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn unknown_and_undeclared_queries_are_refused() {
    let declared = names(&["sessions.list"]);
    let actions = names(&[]);
    let tools = names(&[]);
    let who = caller(PermissionTier::ReadOnly, &declared, &actions, &tools);
    let error = parse_query(&who, "sessions.nope", &json!({})).unwrap_err();
    assert_eq!(error.code(), "E740");
    let error = parse_query(&who, "sessions.search", &json!({"query": "x"})).unwrap_err();
    assert_eq!(error.code(), "E740");
    let query = parse_query(&who, "sessions.list", &json!({})).unwrap();
    assert_eq!(
        query,
        Query::SessionsList {
            limit: 50,
            offset: 0
        }
    );
}

#[test]
fn composer_text_needs_declaration() {
    let declared = names(&["composer.text"]);
    let actions = names(&[]);
    let tools = names(&[]);
    let who = caller(PermissionTier::ReadOnly, &declared, &actions, &tools);
    assert_eq!(
        parse_query(&who, "composer.text", &Value::Null).unwrap(),
        Query::ComposerText
    );
    let undeclared = caller(PermissionTier::ReadOnly, &actions, &actions, &tools);
    assert_eq!(
        parse_query(&undeclared, "composer.text", &Value::Null)
            .unwrap_err()
            .code(),
        "E740"
    );
}

#[test]
fn query_params_are_bounded() {
    let declared = names(&["sessions.list", "sessions.search", "storage.get"]);
    let actions = names(&[]);
    let tools = names(&[]);
    let who = caller(PermissionTier::ReadOnly, &declared, &actions, &tools);
    assert_eq!(
        parse_query(&who, "sessions.list", &json!({"limit": 101}))
            .unwrap_err()
            .code(),
        "E741"
    );
    assert_eq!(
        parse_query(&who, "sessions.search", &json!({"query": "  "}))
            .unwrap_err()
            .code(),
        "E741"
    );
    assert_eq!(
        parse_query(&who, "storage.get", &json!({"key": "bad key"}))
            .unwrap_err()
            .code(),
        "E741"
    );
    let query = parse_query(&who, "sessions.search", &json!({"query": "needle"})).unwrap();
    assert_eq!(
        query,
        Query::SessionsSearch {
            query: "needle".into(),
            limit: 20
        }
    );
}

#[test]
fn actions_need_declaration_and_listed_tools() {
    let queries = names(&[]);
    let declared = names(&["notify", "runTool"]);
    let tools = names(&["read"]);
    let who = caller(PermissionTier::Trusted, &queries, &declared, &tools);
    assert_eq!(
        parse_action(&who, "openSession", &json!({"sessionId": "x"}))
            .unwrap_err()
            .code(),
        "E740"
    );
    assert_eq!(
        parse_action(&who, "runTool", &json!({"name": "write", "arguments": {}}))
            .unwrap_err()
            .code(),
        "E740"
    );
    let action = parse_action(&who, "runTool", &json!({"name": "read"})).unwrap();
    assert!(matches!(action, Action::RunTool { .. }));
}

#[test]
fn tiers_follow_the_tool_class() {
    let queries = names(&[]);
    let actions = names(&["sendPrompt", "openPath", "runTool", "notify"]);
    let tools = names(&["read", "write", "bash"]);
    let read_only = caller(PermissionTier::ReadOnly, &queries, &actions, &tools);
    let workspace = caller(PermissionTier::Workspace, &queries, &actions, &tools);

    let prompt = parse_action(&workspace, "sendPrompt", &json!({"text": "hi"})).unwrap();
    authorize(&workspace, &prompt, None).unwrap();
    let submit = parse_action(
        &workspace,
        "sendPrompt",
        &json!({"text": "hi", "submit": true}),
    )
    .unwrap();
    assert_eq!(
        authorize(&workspace, &submit, None).unwrap_err().code(),
        "E722"
    );

    let open = parse_action(&read_only, "openPath", &json!({"path": "a.txt"})).unwrap();
    assert_eq!(
        authorize(&read_only, &open, None).unwrap_err().code(),
        "E722"
    );

    let read = parse_action(&read_only, "runTool", &json!({"name": "read"})).unwrap();
    authorize(&read_only, &read, Some(true)).unwrap();
    let write = parse_action(&read_only, "runTool", &json!({"name": "write"})).unwrap();
    assert_eq!(
        authorize(&read_only, &write, None).unwrap_err().code(),
        "E722"
    );
    let bash = parse_action(&workspace, "runTool", &json!({"name": "bash"})).unwrap();
    assert_eq!(
        authorize(&workspace, &bash, None).unwrap_err().code(),
        "E722"
    );
    let notify = parse_action(
        &read_only,
        "notify",
        &json!({"level": "info", "message": "ok"}),
    )
    .unwrap();
    authorize(&read_only, &notify, None).unwrap();
}

#[test]
fn action_params_are_strict() {
    let queries = names(&[]);
    let actions = names(&["notify", "clipboardWrite"]);
    let tools = names(&[]);
    let who = caller(PermissionTier::ReadOnly, &queries, &actions, &tools);
    assert_eq!(
        parse_action(&who, "notify", &json!({"level": "loud", "message": "x"}))
            .unwrap_err()
            .code(),
        "E741"
    );
    assert_eq!(
        parse_action(&who, "notify", &json!({"level": "info"}))
            .unwrap_err()
            .code(),
        "E741"
    );
    let big = "x".repeat(64 * 1024 + 1);
    assert_eq!(
        parse_action(&who, "clipboardWrite", &json!({"text": big}))
            .unwrap_err()
            .code(),
        "E741"
    );
    assert_eq!(
        parse_action(&who, "notify", &json!(["not", "an", "object"]))
            .unwrap_err()
            .code(),
        "E741"
    );
}
