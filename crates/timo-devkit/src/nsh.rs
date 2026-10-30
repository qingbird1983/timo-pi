//! 官方 `nsh` CLI 的薄封装（设计 06 §1）。
//!
//! 只调用 CLI，不链接 NSH；私钥路径原样透传，从不读取或打印。

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Output};

use sha2::{Digest as _, Sha256};

/// 包元数据（`nsh verify` 的一行输出）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageMeta {
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) build: u64,
}

/// `nsh` 可执行文件；`NSH` 环境变量优先（与 `scripts/build-nsh-poc` 一致）。
fn program() -> OsString {
    std::env::var_os("NSH").unwrap_or_else(|| OsString::from("nsh"))
}

/// 在 `dir` 里运行 `nsh <args>`。
pub(crate) fn run(dir: &Path, args: &[OsString]) -> Result<Output, String> {
    let program = program();
    Command::new(&program)
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|error| format!("cannot run `{}`: {error}", program.to_string_lossy()))
}

/// 验证包的签名并读出元数据（不校验信任根，与桌面安装前的格式检查同口径）。
pub(crate) fn verify(package: &Path) -> Result<PackageMeta, String> {
    let output = Command::new(program())
        .arg("verify")
        .arg(package)
        .output()
        .map_err(|error| format!("cannot run `nsh verify`: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "verify refused: {}",
            last_line(&output.stderr, "nsh verify failed")
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_verify(&stdout)
        .ok_or_else(|| format!("unexpected `nsh verify` output: {}", stdout.trim()))
}

/// 生成密钥对，返回公钥指纹（公钥文件的 SHA-256 前缀）。
pub(crate) fn keygen(path: &Path) -> Result<String, String> {
    let output = Command::new(program())
        .arg("keygen")
        .arg(path)
        .output()
        .map_err(|error| format!("cannot run `nsh keygen`: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "keygen failed: {}",
            last_line(&output.stderr, "nsh keygen failed")
        ));
    }
    let mut public = path.as_os_str().to_os_string();
    public.push(".pub");
    let hash = sha256_file(Path::new(&public))?;
    let hex = hash.strip_prefix("sha256:").unwrap_or(&hash);
    Ok(hex.chars().take(16).collect())
}

/// 从 `nsh publish` 的摘要行里读出签名密钥指纹（`signed by <hex>`）。
pub(crate) fn signed_by(stdout: &str) -> Option<String> {
    let (_, rest) = stdout.rsplit_once("signed by ")?;
    let fingerprint: String = rest.chars().take_while(char::is_ascii_hexdigit).collect();
    (fingerprint.len() >= 8).then_some(fingerprint)
}

/// 文件内容的 SHA-256（`sha256:<64 hex>`）。
pub(crate) fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes =
        std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Ok(format!("sha256:{}", hex_lower(&Sha256::digest(&bytes))))
}

/// 小写十六进制。
pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn last_line(text: &[u8], fallback: &str) -> String {
    String::from_utf8_lossy(text)
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map_or_else(|| fallback.to_string(), |line| line.trim().to_string())
}

/// `signature valid: <id> <version> (build <n>), for runtime ...`
fn parse_verify(text: &str) -> Option<PackageMeta> {
    let rest = text.trim().strip_prefix("signature valid: ")?;
    let (id, rest) = rest.split_once(' ')?;
    let (version, rest) = rest.split_once(" (build ")?;
    let build = rest.split(')').next()?.parse().ok()?;
    Some(PackageMeta {
        id: id.to_string(),
        version: version.to_string(),
        build,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_line_is_parsed() {
        let meta = parse_verify(
            "signature valid: com.timo.poc 1.0.0 (build 7), for runtime 0.3.0 or newer; \
             the certificate was not checked (pass --root)\n",
        )
        .unwrap();
        assert_eq!(
            meta,
            PackageMeta {
                id: "com.timo.poc".into(),
                version: "1.0.0".into(),
                build: 7,
            }
        );
    }

    #[test]
    fn unexpected_output_is_not_parsed() {
        assert!(parse_verify("signature invalid: nope").is_none());
        assert!(parse_verify("").is_none());
    }

    #[test]
    fn hashes_are_lowercase_hex() {
        assert_eq!(hex_lower(&[0x00, 0xab, 0xff]), "00abff");
    }

    #[test]
    fn publish_summary_yields_the_signing_key() {
        let line = "plugin.nshpkg: com.timo.poc 1.0.0 (build 1), 0 assets, 3 KB, \
                    signed by d42be9117c427545, for runtime 0.3.0 or newer\n";
        assert_eq!(signed_by(line).as_deref(), Some("d42be9117c427545"));
        assert!(signed_by("✓ built src/App.tsx").is_none());
    }
}
