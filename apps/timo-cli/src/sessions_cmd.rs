//! `timo sessions` 列表：按 cwd 过滤并渲染修改时间（从 main.rs 拆出守行数预算）。

use std::path::{Path, PathBuf};

use clap::Args;

use timo_kernel::{Kernel, format_utc_minutes};

#[derive(Args)]
pub(crate) struct SessionsArgs {
    /// Only show sessions created in this working directory.
    #[arg(long)]
    cwd: Option<PathBuf>,
}

pub(crate) fn sessions_command(args: SessionsArgs) -> i32 {
    let dir = match Kernel::sessions_dir(None) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[error] {e}");
            return 1;
        }
    };
    let filter = args.cwd.as_ref().and_then(|p| p.canonicalize().ok());
    for s in Kernel::list_sessions_at(&dir) {
        if let Some(f) = &filter {
            let matches = Path::new(&s.cwd)
                .canonicalize()
                .map(|c| &c == f)
                .unwrap_or(false);
            if !matches {
                continue;
            }
        }
        println!(
            "{}  {}  {:>4} msgs  {}",
            s.id,
            format_utc_minutes(s.modified_ms),
            s.message_count,
            s.first_message
        );
    }
    0
}
