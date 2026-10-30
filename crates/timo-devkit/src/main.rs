//! B 轨插件开发工具（设计 06）。
//!
//! 只做脚手架、静态校验与官方 `nsh` CLI 的编排；不自研签名与打包器。

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod check;
mod fingerprint;
mod nsh;
mod pack;
mod skeleton;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "timo-devkit",
    version,
    about = "Timo GUI plugin devkit (design 06)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a plugin skeleton
    New(NewArgs),
    /// Validate the manifest, nsh.json and the TSX entry
    Check(CheckArgs),
    /// Build and sign a package, then write its manifests
    Pack(PackArgs),
    /// Verify a package and print its metadata (exit code 2 when refused)
    Verify(VerifyArgs),
    /// Create a signing key pair and print the public key fingerprint
    Keygen(KeygenArgs),
}

#[derive(Args)]
struct NewArgs {
    /// Plugin id: lowercase letters, digits, `_` and `-`
    id: String,
    /// Directory to create (default: the id)
    #[arg(long)]
    dir: Option<PathBuf>,
    /// Display name (default: the id)
    #[arg(long)]
    name: Option<String>,
    /// Slot kind to scaffold
    #[arg(long, value_parser = ["panel", "toolCard"], default_value = "panel")]
    slot: String,
    /// Tool bound by a toolCard skeleton
    #[arg(long)]
    tool: Option<String>,
}

#[derive(Args)]
struct CheckArgs {
    /// Plugin directory
    dir: PathBuf,
    /// Also run `nsh check` (downloads TypeScript on first use)
    #[arg(long)]
    types: bool,
}

#[derive(Args)]
struct PackArgs {
    /// Plugin directory
    dir: PathBuf,
    /// Package path (default: `<dir>/<manifest.bundle>`)
    #[arg(long)]
    out: Option<PathBuf>,
    /// Release secret key (never printed)
    #[arg(long)]
    key: PathBuf,
    /// Release certificate from `nsh certify`
    #[arg(long)]
    cert: PathBuf,
    /// Build number (default: `git rev-list --count HEAD`)
    #[arg(long)]
    build: Option<u64>,
    /// Also run `nsh check` during the pre-flight check
    #[arg(long)]
    types: bool,
}

#[derive(Args)]
struct VerifyArgs {
    /// Package (`.nshpkg`)
    package: PathBuf,
}

#[derive(Args)]
struct KeygenArgs {
    /// Path and name of the key files, e.g. `keys/release`
    path: PathBuf,
}

/// 一次失败：退出码 + 逐行写 stderr 的文案。
struct Failure {
    code: u8,
    message: String,
}

impl Failure {
    fn new(message: impl Into<String>) -> Self {
        Self {
            code: 1,
            message: message.into(),
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            for line in failure.message.lines() {
                eprintln!("timo-devkit: {line}");
            }
            ExitCode::from(failure.code)
        }
    }
}

fn run(cli: Cli) -> Result<(), Failure> {
    match cli.command {
        Command::New(args) => skeleton::create(
            &args.id,
            args.dir.as_deref(),
            args.name.as_deref(),
            &args.slot,
            args.tool.as_deref(),
        )
        .map(|files| {
            for file in files {
                println!("created {file}");
            }
        })
        .map_err(Failure::new),
        Command::Check(args) => match check::run(&args.dir, args.types) {
            Ok(checked) => {
                println!(
                    "ok {} v{} ({} slot(s))",
                    checked.manifest.id,
                    checked.manifest.version,
                    checked.manifest.slots.len()
                );
                Ok(())
            }
            Err(problems) => Err(Failure {
                code: 1,
                message: problems.join("\n"),
            }),
        },
        Command::Pack(args) => pack::run(pack::Options {
            dir: args.dir,
            out: args.out,
            key: args.key,
            cert: args.cert,
            build: args.build,
            types: args.types,
        })
        .map_err(Failure::new),
        Command::Verify(args) => match nsh::verify(&args.package) {
            Ok(meta) => match nsh::sha256_file(&args.package) {
                Ok(sha) => {
                    println!(
                        "verified {} {} (build {})",
                        meta.id, meta.version, meta.build
                    );
                    println!("{sha}");
                    Ok(())
                }
                Err(message) => Err(Failure { code: 2, message }),
            },
            Err(message) => Err(Failure { code: 2, message }),
        },
        Command::Keygen(args) => nsh::keygen(&args.path)
            .map(|fingerprint| println!("public key fingerprint {fingerprint}"))
            .map_err(Failure::new),
    }
}
