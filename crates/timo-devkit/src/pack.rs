//! `pack`：校验 → 指纹 → `nsh publish` → 复核包元数据 → 原子落盘与清单写回（设计 06 §3）。

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use timo_ext::GuiManifest;

/// 打包参数（与 CLI 一一对应）。
pub(crate) struct Options {
    pub(crate) dir: PathBuf,
    pub(crate) out: Option<PathBuf>,
    pub(crate) key: PathBuf,
    pub(crate) cert: PathBuf,
    pub(crate) build: Option<u64>,
    pub(crate) types: bool,
}

/// 校验、打包、签名并写回两份清单；失败时不留下半成品。
pub(crate) fn run(options: Options) -> Result<(), String> {
    let dir = absolute(&options.dir)?;
    let checked = super::check::run(&dir, options.types).map_err(|problems| problems.join("\n"))?;
    let mut manifest = checked.manifest;
    let source_hash = super::fingerprint::source_hash(&dir)?;
    let key = absolute(&options.key)?;
    let cert = absolute(&options.cert)?;
    let build = match options.build {
        Some(build) => build,
        None => git_build(&dir)?,
    };
    let out = resolve_out(&dir, &manifest.bundle, options.out.as_deref())?;
    if out.exists() {
        check_existing(&out, build)?;
    }
    let nsh_id = manifest
        .nsh_id
        .clone()
        .ok_or("manifest nsh_id is required to pack")?;

    // 在包所在卷上暂存，成功后 rename 覆盖目标（跨卷 rename 会失败）。
    let parent = out
        .parent()
        .ok_or_else(|| format!("invalid package path: {}", out.display()))?;
    let staging = tempfile::Builder::new()
        .prefix(".timo-devkit-")
        .tempdir_in(parent)
        .map_err(|error| format!("cannot stage next to {}: {error}", parent.display()))?;
    let staged = staging.path().join(
        out.file_name()
            .map_or_else(|| OsString::from("package.nshpkg"), OsString::from),
    );

    let args = publish_args(&manifest, &key, &cert, &staged, &nsh_id, build);
    let output = super::nsh::run(&dir, &args)?;
    if !output.status.success() {
        return Err(publish_error(&output));
    }
    let signed_by = super::nsh::signed_by(&String::from_utf8_lossy(&output.stdout));
    let meta = super::nsh::verify(&staged)?;
    if meta.id != nsh_id || meta.version != manifest.version || meta.build != build {
        return Err(format!(
            "package metadata {} {} (build {}) does not match the manifest",
            meta.id, meta.version, meta.build
        ));
    }
    std::fs::rename(&staged, &out)
        .map_err(|error| format!("cannot move the package to {}: {error}", out.display()))?;

    manifest.source_hash = source_hash;
    manifest.bundle = out.file_name().map_or_else(
        || manifest.bundle.clone(),
        |name| name.to_string_lossy().into_owned(),
    );
    write_manifest(&dir.join("timo-plugin.json"), &manifest)?;
    write_manifest(&parent.join("manifest.json"), &manifest)?;

    println!(
        "packed {} {} (build {build})",
        manifest.id, manifest.version
    );
    println!("package {}", out.display());
    let sha = super::nsh::sha256_file(&out)?;
    let hex = sha.strip_prefix("sha256:").unwrap_or(&sha);
    println!("sha256 {}", hex.chars().take(12).collect::<String>());
    if let Some(fingerprint) = signed_by {
        println!("signed by {fingerprint}");
    }
    Ok(())
}

fn publish_args(
    manifest: &GuiManifest,
    key: &Path,
    cert: &Path,
    staged: &Path,
    nsh_id: &str,
    build: u64,
) -> Vec<OsString> {
    vec![
        OsString::from("publish"),
        OsString::from(&manifest.entry),
        OsString::from("--key"),
        key.as_os_str().to_os_string(),
        OsString::from("--cert"),
        cert.as_os_str().to_os_string(),
        OsString::from("--out"),
        staged.as_os_str().to_os_string(),
        OsString::from("--id"),
        OsString::from(nsh_id),
        OsString::from("--app-version"),
        OsString::from(&manifest.version),
        OsString::from("--build"),
        OsString::from(build.to_string()),
    ]
}

/// 目标包必须存在且可验证；新 build 必须大于包内 build（设计 06 §5）。
fn check_existing(out: &Path, build: u64) -> Result<(), String> {
    let existing = super::nsh::verify(out).map_err(|error| {
        format!(
            "{} exists and cannot be verified ({error}); refusing to overwrite",
            out.display()
        )
    })?;
    if build <= existing.build {
        return Err(format!(
            "build {build} must be greater than {} in {}",
            existing.build,
            out.display()
        ));
    }
    Ok(())
}

/// 默认写到 `<dir>/<manifest.bundle>`；显式路径必须以 `.nshpkg` 结尾。
fn resolve_out(dir: &Path, bundle: &str, out: Option<&Path>) -> Result<PathBuf, String> {
    let path = match out {
        Some(out) => absolute(out)?,
        None => dir.join(bundle),
    };
    if path
        .extension()
        .is_none_or(|extension| extension != "nshpkg")
    {
        return Err(format!(
            "package path must end with .nshpkg: {}",
            path.display()
        ));
    }
    Ok(path)
}

/// 默认 build 取 `git rev-list --count HEAD`；非 git 目录必须显式传 `--build`。
fn git_build(dir: &Path) -> Result<u64, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-list", "--count", "HEAD"])
        .output()
        .map_err(|error| format!("cannot run git to count commits: {error}; pass --build"))?;
    if !output.status.success() {
        return Err(format!(
            "{} is not a git checkout with commits; pass --build",
            dir.display()
        ));
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .map_err(|error| format!("unexpected `git rev-list --count` output: {error}"))
}

fn publish_error(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let text: &str = if stderr.lines().any(|line| !line.trim().is_empty()) {
        &stderr
    } else {
        &stdout
    };
    let lines: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(10)
        .map(|line| format!("nsh publish: {line}"))
        .collect();
    if lines.is_empty() {
        "nsh publish failed".to_string()
    } else {
        lines.join("\n")
    }
}

fn write_manifest(path: &Path, manifest: &GuiManifest) -> Result<(), String> {
    let text = serde_json::to_string_pretty(manifest)
        .map_err(|error| format!("cannot serialize the manifest: {error}"))?
        + "\n";
    std::fs::write(path, text).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn absolute(path: &Path) -> Result<PathBuf, String> {
    std::path::absolute(path).map_err(|error| format!("cannot resolve {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_output_follows_the_bundle_name() {
        let root = tempfile::tempdir().unwrap();
        let out = resolve_out(root.path(), "plugin.nshpkg", None).unwrap();
        assert_eq!(out, root.path().join("plugin.nshpkg"));
    }

    #[test]
    fn explicit_output_must_be_a_package() {
        let root = tempfile::tempdir().unwrap();
        let wrong = root.path().join("plugin.zip");
        assert!(resolve_out(root.path(), "plugin.nshpkg", Some(&wrong)).is_err());
        let right = root.path().join("other.nshpkg");
        assert_eq!(
            resolve_out(root.path(), "plugin.nshpkg", Some(&right)).unwrap(),
            right
        );
    }

    #[test]
    fn package_metadata_comes_from_the_publish_flags() {
        let manifest = GuiManifest {
            schema: timo_ext::GUI_MANIFEST_SCHEMA,
            id: "demo".into(),
            kind: timo_ext::PluginKind::Gui,
            name: "Demo".into(),
            version: "1.2.3".into(),
            nsh_id: Some("com.timo.demo".into()),
            entry: "src/App.tsx".into(),
            bundle: "plugin.nshpkg".into(),
            source_hash: format!("sha256:{}", "0".repeat(64)),
            bundler: "timo-devkit/0.1".into(),
            created_at: None,
            api: Some(timo_ext::HOST_API_VERSION),
            tier: timo_ext::PermissionTier::ReadOnly,
            slots: vec![timo_ext::SlotDecl {
                kind: "panel".into(),
                ..timo_ext::SlotDecl::default()
            }],
            queries: Vec::new(),
            actions: Vec::new(),
            tools: Vec::new(),
            capabilities: timo_ext::Capabilities::default(),
        };
        let args = publish_args(
            &manifest,
            Path::new("keys/release.secret"),
            Path::new("keys/release.cert.json"),
            Path::new("stage/plugin.nshpkg"),
            "com.timo.demo",
            7,
        );
        let text: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(text[0], "publish");
        assert_eq!(text[1], "src/App.tsx");
        assert!(
            text.windows(2)
                .any(|pair| pair == ["--id", "com.timo.demo"])
        );
        assert!(
            text.windows(2)
                .any(|pair| pair == ["--app-version", "1.2.3"])
        );
        assert!(text.windows(2).any(|pair| pair == ["--build", "7"]));
    }
}
