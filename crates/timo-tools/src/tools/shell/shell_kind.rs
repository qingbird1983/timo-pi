//! Shell 选择：Windows 上找原生 bash（Git Bash/MSYS/Cygwin），否则回落 PowerShell。

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShellKind {
    Bash,
    PowerShell,
}

impl ShellKind {
    pub(super) fn program(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::PowerShell => "powershell",
        }
    }
}

/// PATH 里的全部 `bash.exe` 候选（Windows）。
fn bash_candidates() -> Vec<PathBuf> {
    std::env::var("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join("bash.exe"))
                .filter(|path| path.is_file())
                .collect()
        })
        .unwrap_or_default()
}

/// System32/WindowsApps 里的 bash 是 WSL 启动器，不是原生 POSIX 层。
fn is_wsl_launcher(path: &Path) -> bool {
    let text = path.to_string_lossy().to_ascii_lowercase();
    text.contains("\\windows\\system32\\") || text.contains("\\windowsapps\\")
}

/// 可用的原生 bash。按 PATH 顺序取第一个通过 `uname -s` 探测的**绝对路径**——
/// 不能用裸 `bash`：CreateProcess 会先命中 System32 的 WSL 启动器，导致冷启动
/// 十几秒并误判为不可用。
fn native_bash() -> Option<PathBuf> {
    static CACHE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| {
            bash_candidates()
                .into_iter()
                .filter(|path| !is_wsl_launcher(path))
                .find(|path| {
                    let out = std::process::Command::new(path)
                        .args(["-c", "uname -s"])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::null())
                        .output();
                    match out {
                        Ok(o) if o.status.success() => {
                            is_native_bash_uname(&String::from_utf8_lossy(&o.stdout))
                        }
                        _ => false,
                    }
                })
        })
        .clone()
}

/// 按平台把请求的 shell 解析成实际程序（Windows 无原生 bash 时回落 PowerShell）。
pub(super) fn resolve_shell(kind: ShellKind) -> (ShellKind, PathBuf) {
    if kind == ShellKind::Bash && cfg!(windows) {
        match native_bash() {
            Some(path) => (ShellKind::Bash, path),
            None => (ShellKind::PowerShell, PathBuf::from("powershell")),
        }
    } else {
        (kind, PathBuf::from(kind.program()))
    }
}

/// `uname -s` 是否来自 Git Bash/MSYS/Cygwin（纯函数，便于测试）。
fn is_native_bash_uname(uname: &str) -> bool {
    let s = uname.to_ascii_lowercase();
    s.contains("mingw") || s.contains("msys") || s.contains("cygwin")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn native_bash_detection() {
        assert!(is_native_bash_uname("MINGW64_NT-10.0-19045\n"));
        assert!(is_native_bash_uname("MSYS_NT-10.0"));
        assert!(is_native_bash_uname("CYGWIN_NT-10.0"));
        assert!(!is_native_bash_uname("Linux\n"));
    }

    #[test]
    fn wsl_launchers_are_not_native_bash() {
        assert!(is_wsl_launcher(Path::new(r"C:\Windows\System32\bash.exe")));
        assert!(is_wsl_launcher(Path::new(
            r"C:\Users\u\AppData\Local\Microsoft\WindowsApps\bash.exe"
        )));
        assert!(!is_wsl_launcher(Path::new(
            r"C:\Program Files\Git\usr\bin\bash.exe"
        )));
    }

    #[test]
    fn native_bash_path_is_used_when_available() {
        let Some(path) = native_bash() else {
            return;
        };
        assert_eq!(resolve_shell(ShellKind::Bash), (ShellKind::Bash, path));
    }
}
