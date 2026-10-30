//! 清单、`nsh.json` 与 TSX 入口的静态校验（设计 06 §1）。

use std::ffi::OsString;
use std::path::Path;

use serde_json::Value;
use timo_ext::GuiManifest;

/// 校验通过后的清单（pack 继续使用）。
pub(crate) struct Checked {
    pub(crate) manifest: GuiManifest,
}

/// 校验插件目录；失败时返回**全部**问题。
pub(crate) fn run(dir: &Path, types: bool) -> Result<Checked, Vec<String>> {
    if !dir.is_dir() {
        return Err(vec![format!("not a directory: {}", dir.display())]);
    }
    let mut problems = Vec::new();
    let manifest = match GuiManifest::load(&dir.join("timo-plugin.json")) {
        Ok(Some(manifest)) => Some(manifest),
        Ok(None) => {
            problems.push("E720 missing timo-plugin.json".to_string());
            None
        }
        Err(error) => {
            problems.push(error.to_string());
            None
        }
    };
    let mut entry_exists = false;
    if let Some(manifest) = &manifest {
        entry_exists = dir.join(&manifest.entry).is_file();
        if !entry_exists {
            problems.push(format!("E720 entry not found: {}", manifest.entry));
        }
        check_nsh_config(dir, manifest, &mut problems);
    }
    if entry_exists && let Some(manifest) = &manifest {
        compile(dir, &manifest.entry, &mut problems, types);
    }
    match manifest {
        Some(manifest) if problems.is_empty() => Ok(Checked { manifest }),
        _ => Err(problems),
    }
}

/// `nsh.json` 的 id/版本与清单一致，且 `capabilities.host` 覆盖清单声明。
fn check_nsh_config(dir: &Path, manifest: &GuiManifest, problems: &mut Vec<String>) {
    let path = dir.join("nsh.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            problems.push("E720 missing nsh.json (needed by the nsh CLI)".to_string());
            return;
        }
        Err(error) => {
            problems.push(format!("E720 cannot read nsh.json: {error}"));
            return;
        }
    };
    let config: Value = match serde_json::from_str(&text) {
        Ok(config) => config,
        Err(error) => {
            problems.push(format!("E720 invalid nsh.json: {error}"));
            return;
        }
    };
    let nsh_id = manifest.nsh_id.as_deref().unwrap_or_default();
    if config.get("id").and_then(Value::as_str) != Some(nsh_id) {
        problems.push(format!("E720 nsh.json id must be `{nsh_id}`"));
    }
    if config.get("version").and_then(Value::as_str) != Some(manifest.version.as_str()) {
        problems.push(format!(
            "E720 nsh.json version must be `{}`",
            manifest.version
        ));
    }
    let grants = host_grants(&config);
    for name in manifest
        .queries
        .iter()
        .chain(manifest.actions.iter())
        .map(String::as_str)
        .chain(["theme.update"])
    {
        if !granted(&grants, name) {
            problems.push(format!(
                "E720 nsh.json capabilities.host must grant `{name}`"
            ));
        }
    }
}

fn host_grants(config: &Value) -> Vec<String> {
    config
        .get("capabilities")
        .and_then(|capabilities| capabilities.get("host"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// 授权项要么全名相等，要么是以 `*` 结尾的前缀。
fn granted(grants: &[String], name: &str) -> bool {
    grants.iter().any(|grant| match grant.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => grant == name,
    })
}

/// 用 `nsh build` 编译到临时目录（离线）；`--types` 时再跑 `nsh check`。
fn compile(dir: &Path, entry: &str, problems: &mut Vec<String>, types: bool) {
    let staging = match tempfile::tempdir() {
        Ok(staging) => staging,
        Err(error) => {
            problems.push(format!("cannot create a staging directory: {error}"));
            return;
        }
    };
    let build = vec![
        OsString::from("build"),
        OsString::from(entry),
        OsString::from("-o"),
        staging.path().as_os_str().to_os_string(),
    ];
    if !run_step(dir, "nsh build", &build, problems) {
        return;
    }
    if types {
        let check = vec![OsString::from("check"), OsString::from(entry)];
        run_step(dir, "nsh check", &check, problems);
    }
}

fn run_step(dir: &Path, label: &str, args: &[OsString], problems: &mut Vec<String>) -> bool {
    let output = match super::nsh::run(dir, args) {
        Ok(output) => output,
        Err(error) => {
            problems.push(error);
            return false;
        }
    };
    if output.status.success() {
        return true;
    }
    let text = if output.stderr.iter().any(u8::is_ascii_graphic) {
        String::from_utf8_lossy(&output.stderr).into_owned()
    } else {
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(20)
        .peekable();
    if lines.peek().is_none() {
        problems.push(format!("{label}: failed"));
    }
    for line in lines {
        problems.push(format!("{label}: {line}"));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::granted;

    fn grants(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn host_grants_accept_exact_names_and_prefixes() {
        let list = grants(&["sessions.*", "openSession"]);
        assert!(granted(&list, "sessions.search"));
        assert!(granted(&list, "openSession"));
        assert!(!granted(&list, "sessionsX"));
        assert!(!granted(&list, "openPath"));
        assert!(!granted(&grants(&[]), "theme.update"));
    }
}
