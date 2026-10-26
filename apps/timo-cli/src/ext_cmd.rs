//! `timo ext` 子命令（设计 03 §4、02 §3.6）：安装、列出、授权、移除与诊断。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use clap::{Args, Subcommand};
use timo_kernel::{
    ExtensionOptions, PermissionStore, install_extension, load_extensions, permissions_path,
    remove_extension, summarize, trust_extension,
};

/// `timo ext` 参数。
#[derive(Args)]
pub struct ExtArgs {
    #[command(subcommand)]
    command: ExtCommand,
}

#[derive(Subcommand)]
enum ExtCommand {
    /// List discovered extensions and their grants.
    List,
    /// Install (pack) an extension file into the agent directory.
    Add {
        /// Path to the extension source file.
        path: PathBuf,
    },
    /// Remove an installed extension and its grants.
    Remove {
        /// Extension id.
        id: String,
    },
    /// Grant (or revoke) all declared capabilities.
    Trust {
        /// Extension id.
        id: String,
        /// Revoke instead of grant.
        #[arg(long)]
        revoke: bool,
    },
    /// Load extensions and print the diagnostic report.
    Doctor {
        /// Emit JSON instead of the human table.
        #[arg(long)]
        json: bool,
    },
}

/// 执行子命令。
pub async fn run(args: ExtArgs) -> i32 {
    match args.command {
        ExtCommand::List => list(),
        ExtCommand::Add { path } => match data_dir() {
            Ok(data) => match install_extension(&data, &path).await {
                Ok(manifest) => {
                    println!("installed {} ({})", manifest.id, manifest.bundle);
                    0
                }
                Err(error) => fail(&error),
            },
            Err(error) => fail(&error),
        },
        ExtCommand::Remove { id } => match data_dir() {
            Ok(data) => match remove_extension(&data, &id) {
                Ok(()) => {
                    println!("removed {id}");
                    0
                }
                Err(error) => fail(&error),
            },
            Err(error) => fail(&error),
        },
        ExtCommand::Trust { id, revoke } => match data_dir() {
            Ok(data) => match trust_extension(&data, &id, revoke) {
                Ok(()) => {
                    println!("{} {id}", if revoke { "revoked" } else { "granted" });
                    0
                }
                Err(error) => fail(&error),
            },
            Err(error) => fail(&error),
        },
        ExtCommand::Doctor { json } => doctor(json).await,
    }
}

fn list() -> i32 {
    let data = match data_dir() {
        Ok(data) => data,
        Err(error) => return fail(&error),
    };
    let work_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let store = PermissionStore::load(&permissions_path(&data)).unwrap_or_default();
    let found = summarize(&data, &work_dir, vec![]);
    if found.is_empty() {
        println!("（无扩展）");
        return 0;
    }
    println!("{:<20} {:<8} {:<10} DECLARED", "ID", "SCOPE", "GRANT");
    for extension in found {
        let (label, declared) = match &extension.manifest {
            Some(manifest) => {
                let state = store.effective(&extension.id, &manifest.source_hash).1;
                let declared = describe(&manifest.capabilities);
                (format!("{state:?}").to_lowercase(), declared)
            }
            None => ("missing".to_string(), "-".to_string()),
        };
        println!(
            "{:<20} {:<8} {:<10} {}",
            extension.id,
            scope_label(extension.scope),
            label,
            declared
        );
    }
    0
}

async fn doctor(json: bool) -> i32 {
    let data = match data_dir() {
        Ok(data) => data,
        Err(error) => return fail(&error),
    };
    let work_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let runtime = load_extensions(ExtensionOptions {
        data_dir: data,
        work_dir,
        explicit: vec![],
        interactive: false,
        include_project_pi: true,
        abort: Arc::new(AtomicBool::new(false)),
        running: Arc::new(AtomicBool::new(false)),
        flags: Default::default(),
    })
    .await;
    if json {
        println!("{}", runtime.report.to_json_pretty().unwrap_or_default());
    } else {
        print!("{}", runtime.report.render_human());
    }
    if runtime.report.summary().failed > 0 {
        1
    } else {
        0
    }
}

fn describe(capabilities: &timo_kernel::Capabilities) -> String {
    let mut parts = Vec::new();
    if capabilities.exec {
        parts.push("exec".to_string());
    }
    if !capabilities.net.domains.is_empty() {
        parts.push(format!("net:{}", capabilities.net.domains.join(",")));
    }
    for (label, scopes) in [
        ("fs.read", &capabilities.fs.read),
        ("fs.write", &capabilities.fs.write),
        ("fs.delete", &capabilities.fs.delete),
    ] {
        if !scopes.is_empty() {
            parts.push(format!("{label}:{}", scopes.join(",")));
        }
    }
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join(" ")
    }
}

fn scope_label(scope: timo_kernel::Scope) -> &'static str {
    match scope {
        timo_kernel::Scope::Explicit => "explicit",
        timo_kernel::Scope::Project => "project",
        timo_kernel::Scope::ProjectPi => "project-pi",
        timo_kernel::Scope::User => "user",
    }
}

fn data_dir() -> Result<PathBuf, String> {
    timo_kernel::data_dir().map_err(|error| error.to_string())
}

fn fail(error: &impl std::fmt::Display) -> i32 {
    eprintln!("[error] {error}");
    1
}
