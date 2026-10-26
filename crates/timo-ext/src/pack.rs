//! 安装期打包（设计 02 §3~§5）：rolldown 打包 + 禁用面扫描 + 能力建议。
//!
//! 产物为 CommonJS 单文件（`bundle.cjs`），宿主用 `module/exports` 垫片执行；
//! 这样 QuickJS 不需要模块解析器，pi 的 ESM 写法在打包期被消除。

use std::fs;
use std::path::{Path, PathBuf};

use rolldown::{
    Bundler, BundlerOptions, InputItem, OutputFormat, Platform, ResolveOptions, TreeshakeOptions,
};

use crate::Result;
use crate::diag::ErrorCode;
use crate::error::ExtError;
use crate::manifest::{Capabilities, MANIFEST_SCHEMA, Manifest, compute_source_hash};

/// 打包产物上限（设计 02 §2）。
pub const MAX_BUNDLE_BYTES: usize = 5 * 1024 * 1024;

/// 打包选项：`root/source/` 已就位，产物写入 `out_dir`。
#[derive(Debug, Clone)]
pub struct PackOptions {
    /// 扩展根（`manifest.json` 与 `bundle.cjs` 的落点；必须包含 `source/`）。
    pub root: PathBuf,
    /// 入口文件（必须位于 `root/source/` 内）。
    pub entry: PathBuf,
    /// 扩展 id。
    pub id: String,
    /// 产物目录（通常等于 `root`）。
    pub out_dir: PathBuf,
}

/// 打包结果。
#[derive(Debug, Clone)]
pub struct PackOutput {
    /// 生成的 manifest（能力字段为默认全拒，授权另存）。
    pub manifest: Manifest,
    /// 产物路径。
    pub bundle_path: PathBuf,
    /// 静态扫描得到的建议能力（仅建议）。
    pub proposed: Capabilities,
    /// 打包器警告（人读）。
    pub warnings: Vec<String>,
}

/// 执行一次打包并写出 `bundle.cjs` / `manifest.json`。
///
/// # Errors
/// 入口非法（E101）、打包器失败（E103/E106/E107）、禁用面（E104/E105）、
/// 产物超限（E108）→ [`ExtError::Pack`]；源码指纹失败 → [`ExtError::Hash`]。
pub async fn pack(options: PackOptions) -> Result<PackOutput> {
    let source_dir = options.root.join("source");
    if !source_dir.is_dir() {
        return Err(fail(
            ErrorCode::E101,
            format!("missing source dir: {}", source_dir.display()),
        ));
    }
    let entry = if options.entry.is_absolute() {
        options.entry.clone()
    } else {
        options.root.join(&options.entry)
    };
    if !entry.is_file() {
        return Err(fail(
            ErrorCode::E101,
            format!("entry not readable: {}", entry.display()),
        ));
    }
    // 不用 canonicalize()：Windows 会得到 `\\?\` 前缀，rolldown 解析不了。
    let entry_relative = entry.strip_prefix(&source_dir).map_err(|_| {
        fail(
            ErrorCode::E101,
            format!(
                "entry {} is outside {}",
                entry.display(),
                source_dir.display()
            ),
        )
    })?;
    let entry_relative = entry_relative.to_string_lossy().replace('\\', "/");

    let source_hash = compute_source_hash(&source_dir)?;
    let shims = tempfile::Builder::new()
        .prefix("timo-shims-")
        .tempdir()
        .map_err(|error| fail(ErrorCode::E107, format!("cannot create shim dir: {error}")))?;
    write_shims(shims.path())?;

    let aliases = [
        ("@earendil-works/pi-coding-agent", "coding-agent.js"),
        ("@earendil-works/pi-ai", "pi-ai.js"),
        ("@earendil-works/pi-agent-core", "inert.js"),
        ("@earendil-works/pi-tui", "inert.js"),
        ("@mariozechner/pi-coding-agent", "coding-agent.js"),
        ("@mariozechner/pi-ai", "pi-ai.js"),
        ("@mariozechner/pi-agent-core", "inert.js"),
        ("@mariozechner/pi-tui", "inert.js"),
        ("typebox", "typebox.js"),
        ("@mariozechner/typebox", "typebox.js"),
    ]
    .iter()
    .map(|(specifier, file)| {
        (
            (*specifier).to_string(),
            vec![Some(shims.path().join(file).to_string_lossy().into_owned())],
        )
    })
    .collect();

    let bundler_options = BundlerOptions {
        input: Some(vec![InputItem {
            name: None,
            import: entry.to_string_lossy().into_owned(),
        }]),
        cwd: Some(options.root.clone()),
        format: Some(OutputFormat::Cjs),
        platform: Some(Platform::Neutral),
        shim_missing_exports: Some(true),
        treeshake: TreeshakeOptions::Boolean(true),
        resolve: Some(ResolveOptions {
            alias: Some(aliases),
            extensions: Some(vec![
                ".ts".into(),
                ".tsx".into(),
                ".js".into(),
                ".mjs".into(),
                ".cjs".into(),
                ".json".into(),
            ]),
            condition_names: Some(vec!["import".into(), "default".into()]),
            ..ResolveOptions::default()
        }),
        ..BundlerOptions::default()
    };
    let mut bundler = Bundler::new(bundler_options)
        .map_err(|error| fail(ErrorCode::E107, format!("bundler init failed: {error:?}")))?;
    let output = bundler
        .generate()
        .await
        .map_err(|error| classify(&format!("{error:?}")))?;

    let asset = output
        .assets
        .first()
        .ok_or_else(|| fail(ErrorCode::E107, "bundler produced no assets"))?;
    let code = String::from_utf8_lossy(asset.content_as_bytes()).into_owned();
    if code.len() > MAX_BUNDLE_BYTES {
        return Err(fail(
            ErrorCode::E108,
            format!("bundle is {} bytes (limit {MAX_BUNDLE_BYTES})", code.len()),
        ));
    }
    scan_forbidden(&code)?;

    fs::create_dir_all(&options.out_dir).map_err(|error| {
        fail(
            ErrorCode::E107,
            format!(
                "cannot create out dir {}: {error}",
                options.out_dir.display()
            ),
        )
    })?;
    let bundle_path = options.out_dir.join("bundle.cjs");
    fs::write(&bundle_path, &code).map_err(|error| {
        fail(
            ErrorCode::E107,
            format!("cannot write {}: {error}", bundle_path.display()),
        )
    })?;

    let manifest = Manifest {
        schema: MANIFEST_SCHEMA,
        id: options.id,
        entry: format!("source/{entry_relative}"),
        bundle: "bundle.cjs".to_string(),
        source_hash,
        bundler: "rolldown-1.2+shim-1".to_string(),
        created_at: None,
        capabilities: Capabilities::default(),
    };
    manifest
        .validate(&options.out_dir.join("manifest.json"))
        .map_err(|error| fail(ErrorCode::E110, error.to_string()))?;
    let manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|error| fail(ErrorCode::E107, format!("manifest encode failed: {error}")))?;
    fs::write(options.out_dir.join("manifest.json"), manifest_json).map_err(|error| {
        fail(
            ErrorCode::E107,
            format!("cannot write manifest.json: {error}"),
        )
    })?;

    let mut warnings = output
        .warnings
        .iter()
        .map(|warning| format!("{warning:?}"))
        .collect::<Vec<_>>();
    if code.contains("fetch(") || code.contains("WebSocket") {
        warnings.push("network usage detected: declare net domains in manifest".to_string());
    }
    Ok(PackOutput {
        manifest,
        bundle_path,
        proposed: propose_capabilities(&code),
        warnings,
    })
}

fn write_shims(dir: &Path) -> Result<()> {
    let shims = [
        ("typebox.js", include_str!("shim/typebox.js")),
        ("coding-agent.js", include_str!("shim/coding-agent.js")),
        ("pi-ai.js", include_str!("shim/pi-ai.js")),
        ("inert.js", include_str!("shim/inert.js")),
    ];
    for (name, content) in shims {
        fs::write(dir.join(name), content).map_err(|error| {
            fail(
                ErrorCode::E107,
                format!("cannot write shim {name}: {error}"),
            )
        })?;
    }
    Ok(())
}

fn scan_forbidden(code: &str) -> Result<()> {
    if code.contains("node:") {
        return Err(fail(
            ErrorCode::E104,
            "bundle references node:* built-ins (not available in QuickJS)",
        ));
    }
    if code.contains("require(") || code.contains("eval(") || code.contains("new Function(") {
        return Err(fail(
            ErrorCode::E105,
            "bundle contains dynamic code loading (require/eval/new Function)",
        ));
    }
    Ok(())
}

fn propose_capabilities(code: &str) -> Capabilities {
    Capabilities {
        exec: code.contains("pi.exec"),
        ..Capabilities::default()
    }
}

fn classify(message: &str) -> ExtError {
    let lower = message.to_lowercase();
    let code = if lower.contains("could not resolve") || lower.contains("failed to resolve") {
        ErrorCode::E103
    } else if lower.contains("parse")
        || lower.contains("syntax")
        || lower.contains("top-level await")
        || lower.contains("top level await")
    {
        ErrorCode::E106
    } else {
        ErrorCode::E107
    };
    fail(code, message.to_string())
}

fn fail(code: ErrorCode, message: impl Into<String>) -> ExtError {
    ExtError::Pack {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn capability_probe_is_advisory() {
        let proposed =
            propose_capabilities("await pi.exec('git', ['status']); fetch('https://x');");
        assert!(proposed.exec);
        assert!(proposed.net.domains.is_empty());
        assert!(!propose_capabilities("const x = 1;").exec);
    }

    #[test]
    fn forbidden_scan_catches_node_and_eval() {
        assert!(matches!(
            scan_forbidden("import fs from 'node:fs'"),
            Err(ExtError::Pack {
                code: ErrorCode::E104,
                ..
            })
        ));
        assert!(matches!(
            scan_forbidden("eval('1')"),
            Err(ExtError::Pack {
                code: ErrorCode::E105,
                ..
            })
        ));
        assert!(scan_forbidden("const x = 1;").is_ok());
    }
}
