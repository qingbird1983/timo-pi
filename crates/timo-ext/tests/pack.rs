//! 安装期打包集成测试（设计 02 §3~§5）。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::PathBuf;

use timo_ext::{ErrorCode, ExtError, PackOptions, pack};

fn setup(source: &str) -> (tempfile::TempDir, PackOptions) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    fs::create_dir_all(root.join("source")).unwrap();
    fs::write(root.join("source/index.ts"), source).unwrap();
    let options = PackOptions {
        root: root.clone(),
        entry: root.join("source/index.ts"),
        id: "demo".into(),
        out_dir: root,
    };
    (dir, options)
}

fn expect_code(error: ExtError, code: ErrorCode) {
    match error {
        ExtError::Pack { code: actual, .. } => assert_eq!(actual, code),
        other => panic!("unexpected error: {other:?}"),
    }
}

const HELLO: &str = r#"
import { Type } from "@earendil-works/pi-ai";
import { defineTool, type ExtensionAPI } from "@earendil-works/pi-coding-agent";

const tool = defineTool({
  name: "hello",
  label: "Hello",
  description: "A simple greeting tool",
  parameters: Type.Object({ name: Type.String({ description: "Name" }) }),
  async execute(_id: string, params: { name: string }) {
    return { content: [{ type: "text", text: `Hello, ${params.name}!` }], details: {} };
  },
});

export default function (pi: ExtensionAPI) {
  pi.registerTool(tool);
}
"#;

#[tokio::test]
async fn bundles_shim_imports_into_single_cjs() {
    let (dir, options) = setup(HELLO);
    let out = pack(options).await.unwrap();
    let code = fs::read_to_string(&out.bundle_path).unwrap();
    assert!(code.contains("Hello"));
    assert!(!code.contains("@earendil-works"));
    assert!(!code.contains("node:"));
    assert_eq!(out.manifest.entry, "source/index.ts");
    assert_eq!(out.manifest.bundle, "bundle.cjs");
    assert!(out.manifest.source_hash.starts_with("sha256:"));
    assert!(dir.path().join("root/manifest.json").is_file());
    assert!(!out.proposed.exec);
}

#[tokio::test]
async fn node_builtin_is_rejected_with_e104() {
    let (dir, options) = setup(
        r#"
import fs from "node:fs";
export default function () { void fs; }
"#,
    );
    let error = pack(options).await.unwrap_err();
    expect_code(error, ErrorCode::E104);
    drop(dir);
}

#[tokio::test]
async fn dynamic_eval_is_rejected_with_e105() {
    let (dir, options) = setup(
        r#"
export default function () { eval("1 + 1"); }
"#,
    );
    let error = pack(options).await.unwrap_err();
    expect_code(error, ErrorCode::E105);
    drop(dir);
}

#[tokio::test]
async fn top_level_await_is_rejected_with_e106() {
    let (dir, options) = setup(
        r#"
const ready = await Promise.resolve(1);
export default function () { void ready; }
"#,
    );
    let error = pack(options).await.unwrap_err();
    expect_code(error, ErrorCode::E106);
    drop(dir);
}

#[tokio::test]
async fn entry_outside_source_is_rejected_with_e101() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    fs::create_dir_all(root.join("source")).unwrap();
    fs::write(root.join("outside.ts"), "export default () => {};").unwrap();
    let options = PackOptions {
        root: root.clone(),
        entry: root.join("outside.ts"),
        id: "demo".into(),
        out_dir: root,
    };
    let error = pack(options).await.unwrap_err();
    expect_code(error, ErrorCode::E101);
}

#[tokio::test]
async fn missing_source_dir_is_rejected_with_e101() {
    let dir = tempfile::tempdir().unwrap();
    let root = PathBuf::from(dir.path()).join("root");
    fs::create_dir_all(&root).unwrap();
    let options = PackOptions {
        root: root.clone(),
        entry: root.join("index.ts"),
        id: "demo".into(),
        out_dir: root,
    };
    let error = pack(options).await.unwrap_err();
    expect_code(error, ErrorCode::E101);
}
