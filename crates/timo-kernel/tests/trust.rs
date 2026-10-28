#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! 项目信任门控的内核级接线（设计 phase-3.5/12 §3.3 / §9.1）。

use std::fs;
use std::path::{Path, PathBuf};

use timo_kernel::{Kernel, KernelOptions, TrustChoice, TrustScopeChoice, TrustSource};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("timo-kernel-trust-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn options(data: &Path, work: &Path, trust: Option<bool>) -> KernelOptions {
    KernelOptions {
        data_dir: Some(data.to_path_buf()),
        work_dir: Some(work.to_path_buf()),
        model_override: Some(("anthropic".into(), "claude-sonnet-4-5".into())),
        api_key: Some("test-key".into()),
        trust_override: trust,
        trust_interactive: Some(false),
        ..Default::default()
    }
}

/// 项目内四类可见资源 + 一个不受门控的上下文文件。
fn project_fixture(data: &Path, work: &Path) {
    fs::create_dir_all(work.join(".timo/skills/demo")).unwrap();
    fs::create_dir_all(work.join(".timo/prompts")).unwrap();
    fs::write(
        work.join(".timo/settings.json"),
        r#"{"default_thinking_level":"high"}"#,
    )
    .unwrap();
    fs::write(work.join(".timo/SYSTEM.md"), "PROJECT_SECRET").unwrap();
    fs::write(
        work.join(".timo/skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: demo skill\n---\nbody",
    )
    .unwrap();
    fs::write(
        work.join(".timo/prompts/hello.md"),
        "---\ndescription: hi\n---\nHello $1",
    )
    .unwrap();
    fs::write(work.join("AGENTS.md"), "CONTEXT_SECRET").unwrap();
    fs::create_dir_all(data).unwrap();
    fs::write(
        data.join("mcp.json"),
        r#"{"servers": {"docs": {"command": "npx", "args": ["x"]}}}"#,
    )
    .unwrap();
    fs::write(
        work.join(".timo/mcp.json"),
        r#"{"servers": {"docs": {"command": "npx", "args": ["x"], "enabled": false}}}"#,
    )
    .unwrap();
}

#[tokio::test]
async fn untrusted_project_skips_project_resources_but_keeps_context_files() {
    let data = temp("untrusted");
    let work = data.join("work");
    project_fixture(&data, &work);

    let mut kernel = Kernel::new(options(&data, &work, Some(false)))
        .await
        .unwrap();
    kernel.reload_resources();

    let trust = kernel.project_trust();
    assert!(!trust.trusted);
    assert_eq!(trust.source, TrustSource::Override);
    assert!(trust.skipped.len() >= 5, "{:?}", trust.skipped);

    // 项目设置的 default_thinking_level 不生效（回到默认 medium）。
    assert_eq!(kernel.thinking_label(), "medium");
    // 项目 SYSTEM.md 被跳过，上下文文件仍注入。
    assert!(!kernel.system_prompt().contains("PROJECT_SECRET"));
    assert!(kernel.system_prompt().contains("CONTEXT_SECRET"));
    // 项目技能与模板不加载。
    assert!(!kernel.has_template("hello"));
    assert!(
        !kernel
            .slash_commands()
            .iter()
            .any(|command| command.name == "skill:demo")
    );
    // 项目 mcp.json 不读：全局的 docs 仍启用。
    assert!(kernel.mcp_servers().iter().any(|name| name == "docs"));
    // 告警出口有一条摘要。
    assert!(
        kernel
            .resource_warnings()
            .iter()
            .any(|warning| warning.kind == timo_kernel::WarningKind::UntrustedProject)
    );

    assert!(!data.join("trust.json").exists());
    let _ = fs::remove_dir_all(&data);
}

#[tokio::test]
async fn trusted_project_loads_everything() {
    let data = temp("trusted");
    let work = data.join("work");
    project_fixture(&data, &work);

    let mut kernel = Kernel::new(options(&data, &work, Some(true)))
        .await
        .unwrap();
    kernel.reload_resources();

    assert!(kernel.project_trust().trusted);
    assert_eq!(kernel.thinking_label(), "high");
    assert!(kernel.system_prompt().contains("PROJECT_SECRET"));
    assert!(kernel.system_prompt().contains("CONTEXT_SECRET"));
    assert!(kernel.has_template("hello"));
    assert_eq!(kernel.expand_input("/hello"), "Hello ");
    assert!(
        kernel
            .slash_commands()
            .iter()
            .any(|command| command.name == "skill:demo")
    );
    // 项目文件把 docs 关掉了。
    assert!(!kernel.mcp_servers().iter().any(|name| name == "docs"));
    assert!(!data.join("trust.json").exists());
    let _ = fs::remove_dir_all(&data);
}

#[tokio::test]
async fn saved_decision_applies_on_next_start() {
    let data = temp("saved");
    let work = data.join("work");
    project_fixture(&data, &work);

    let first = Kernel::new(options(&data, &work, None)).await.unwrap();
    assert!(!first.project_trust().trusted);
    assert_eq!(first.project_trust().source, TrustSource::NoUi);
    first
        .save_project_trust(TrustChoice {
            trusted: true,
            scope: TrustScopeChoice::Directory,
        })
        .unwrap();
    assert!(data.join("trust.json").exists());

    let second = Kernel::new(options(&data, &work, None)).await.unwrap();
    assert!(second.project_trust().trusted);
    assert_eq!(second.project_trust().source, TrustSource::Store);
    let _ = fs::remove_dir_all(&data);
}

#[tokio::test]
async fn no_project_resources_needs_no_decision() {
    let data = temp("noresources");
    let work = data.join("work");
    fs::create_dir_all(&work).unwrap();
    let kernel = Kernel::new(options(&data, &work, None)).await.unwrap();
    assert!(kernel.project_trust().trusted);
    assert_eq!(kernel.project_trust().source, TrustSource::NoResources);

    // 命令行覆盖仍排在"无资源"之前（决策顺序，设计 12 §2.2）。
    let overridden = Kernel::new(options(&data, &work, Some(false)))
        .await
        .unwrap();
    assert!(!overridden.project_trust().trusted);
    assert_eq!(overridden.project_trust().source, TrustSource::Override);
    assert!(!data.join("trust.json").exists());
    let _ = fs::remove_dir_all(&data);
}

const HELLO_BUNDLE: &str = r#"
module.exports = function (pi) {
  pi.registerTool({
    name: "hello",
    label: "Hello",
    description: "test tool",
    parameters: {},
    execute: async () => ({ content: [{ type: "text", text: "ok" }], details: {} }),
  });
};
"#;

/// 在 `<base>/extensions/<id>` 装一个已打包扩展（测试镜像 `tests/extensions.rs` 的夹具）。
fn install_at(base: &Path, id: &str, bundle: &str) {
    let root = base.join("extensions").join(id);
    let source = root.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("index.ts"), "export default function () {}\n").unwrap();
    fs::write(root.join("bundle.cjs"), bundle).unwrap();
    let hash = timo_ext::compute_source_hash(&root).unwrap();
    let manifest = serde_json::json!({
        "schema": 1,
        "id": id,
        "entry": "source/index.ts",
        "bundle": "bundle.cjs",
        "source_hash": hash,
        "bundler": "test",
        "created_at": "2026-10-26T12:00:00Z",
        "capabilities": { "exec": false, "net": { "domains": [] }, "fs": { "read": [], "write": [], "delete": [] } },
    });
    fs::write(
        root.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

fn install_extension(work: &Path, id: &str) {
    install_at(&work.join(".timo"), id, HELLO_BUNDLE);
}

#[tokio::test]
async fn untrusted_project_skips_project_extensions() {
    let data = temp("ext-untrusted");
    let work = data.join("work");
    install_extension(&work, "demo");

    let mut kernel = Kernel::new(options(&data, &work, Some(false)))
        .await
        .unwrap();
    let _ = kernel.tools().await;
    let report = kernel.extension_report().expect("extension report");
    assert_eq!(report.summary().loaded, 0);
    let _ = fs::remove_dir_all(&data);

    let data = temp("ext-trusted");
    let work = data.join("work");
    install_extension(&work, "demo");
    let mut kernel = Kernel::new(options(&data, &work, Some(true)))
        .await
        .unwrap();
    let _ = kernel.tools().await;
    let report = kernel.extension_report().expect("extension report");
    assert_eq!(report.summary().loaded, 1);
    let _ = fs::remove_dir_all(&data);
}

#[tokio::test]
async fn override_beats_saved_decision() {
    let data = temp("override-saved");
    let work = data.join("work");
    project_fixture(&data, &work);

    let first = Kernel::new(options(&data, &work, None)).await.unwrap();
    first
        .save_project_trust(TrustChoice {
            trusted: false,
            scope: TrustScopeChoice::Directory,
        })
        .unwrap();

    let second = Kernel::new(options(&data, &work, Some(true)))
        .await
        .unwrap();
    assert!(second.project_trust().trusted);
    assert_eq!(second.project_trust().source, TrustSource::Override);
    let _ = fs::remove_dir_all(&data);
}

#[tokio::test]
async fn user_extension_decides_trust_and_remembers() {
    let data = temp("ext-decide");
    let work = data.join("work");
    project_fixture(&data, &work);
    install_at(
        &data,
        "trustor",
        r#"module.exports = function (pi) {
  pi.on("project_trust", async (event, ctx) => ({ trusted: "yes", remember: true }));
};"#,
    );

    let kernel = Kernel::new(options(&data, &work, None)).await.unwrap();
    assert!(kernel.project_trust().trusted);
    assert_eq!(kernel.project_trust().source, TrustSource::Extension);
    assert!(data.join("trust.json").exists());
    let _ = fs::remove_dir_all(&data);
}

#[tokio::test]
async fn extension_undecided_falls_through_to_no_ui() {
    let data = temp("ext-undecided");
    let work = data.join("work");
    project_fixture(&data, &work);
    install_at(
        &data,
        "undecided",
        r#"module.exports = function (pi) {
  pi.on("project_trust", async () => ({ trusted: "undecided" }));
};"#,
    );

    let mut kernel = Kernel::new(options(&data, &work, None)).await.unwrap();
    assert!(!kernel.project_trust().trusted);
    assert_eq!(kernel.project_trust().source, TrustSource::NoUi);
    // 用户级扩展仍在（pre-trust 装载被复用，不因未信任而消失）。
    let _ = kernel.tools().await;
    assert_eq!(kernel.extension_report().unwrap().summary().loaded, 1);
    assert!(!data.join("trust.json").exists());
    let _ = fs::remove_dir_all(&data);
}

const PROBE_BUNDLE: &str = r#"
module.exports = function (pi) {
  pi.registerCommand("probe", {
    description: "throws UNTRUSTED when the project is not trusted",
    handler: async (args, ctx) => {
      if (!ctx.isProjectTrusted()) throw new Error("UNTRUSTED");
    },
  });
};
"#;

fn probe_command(kernel: &Kernel) -> String {
    kernel
        .extension_commands()
        .into_iter()
        .find(|name| name.ends_with("_probe"))
        .expect("probe command")
}

/// `ctx.isProjectTrusted()` 必须跟着本次决定走（设计 12 §8 批 B）。
#[tokio::test]
async fn extension_context_reads_trust_override() {
    let data = temp("ext-live-override");
    let work = data.join("work");
    project_fixture(&data, &work);
    install_at(&data, "prober", PROBE_BUNDLE);

    let mut untrusted = Kernel::new(options(&data, &work, Some(false)))
        .await
        .unwrap();
    let _ = untrusted.tools().await;
    let command = probe_command(&untrusted);
    let error = untrusted
        .run_extension_command(&command, serde_json::json!({}))
        .unwrap_err();
    assert!(error.to_string().contains("UNTRUSTED"), "{error}");
    drop(untrusted);

    let mut trusted = Kernel::new(options(&data, &work, Some(true)))
        .await
        .unwrap();
    let _ = trusted.tools().await;
    let command = probe_command(&trusted);
    trusted
        .run_extension_command(&command, serde_json::json!({}))
        .unwrap();
    let _ = fs::remove_dir_all(&data);
}

/// pre-trust 装载复用的宿主也必须读到决定后的活值（这里决定是"信任"，能区分复用与重载）。
#[tokio::test]
async fn extension_context_reads_pretrust_decision() {
    let data = temp("ext-live-pretrust");
    let work = data.join("work");
    project_fixture(&data, &work);
    install_at(
        &data,
        "prober",
        r#"module.exports = function (pi) {
  pi.on("project_trust", async () => ({ trusted: "yes" }));
  pi.registerCommand("probe", {
    description: "throws UNTRUSTED when the project is not trusted",
    handler: async (args, ctx) => {
      if (!ctx.isProjectTrusted()) throw new Error("UNTRUSTED");
    },
  });
};
"#,
    );

    let mut kernel = Kernel::new(options(&data, &work, None)).await.unwrap();
    assert_eq!(kernel.project_trust().source, TrustSource::Extension);
    assert!(kernel.project_trust().trusted);
    let _ = kernel.tools().await;
    let command = probe_command(&kernel);
    kernel
        .run_extension_command(&command, serde_json::json!({}))
        .unwrap();
    let _ = fs::remove_dir_all(&data);
}

#[tokio::test]
async fn extension_decision_without_remember_is_not_persisted() {
    let data = temp("ext-nomem");
    let work = data.join("work");
    project_fixture(&data, &work);
    install_at(
        &data,
        "trustor",
        r#"module.exports = function (pi) {
  pi.on("project_trust", async () => ({ trusted: "no" }));
};"#,
    );

    let kernel = Kernel::new(options(&data, &work, None)).await.unwrap();
    assert!(!kernel.project_trust().trusted);
    assert_eq!(kernel.project_trust().source, TrustSource::Extension);
    assert!(!data.join("trust.json").exists());
    let _ = fs::remove_dir_all(&data);
}
