//! `timo sessions` 列表：按 cwd 过滤并渲染修改时间（从 main.rs 拆出守行数预算）。

use std::path::{Path, PathBuf};

use clap::Args;

use timo_kernel::Kernel;

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
            format_ms(s.modified_ms),
            s.message_count,
            s.first_message
        );
    }
    0
}

/// unix 毫秒 → `YYYY-MM-DD HH:MM UTC`（不引入时间库）。
fn format_ms(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02} UTC",
        rem / 3600,
        (rem % 3600) / 60
    )
}

/// Howard Hinnant 的 days → (year, month, day)（公历）。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 { y + 1 } else { y },
        u32::try_from(m).unwrap_or(1),
        u32::try_from(d).unwrap_or(1),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn formats_epoch_in_utc() {
        assert_eq!(format_ms(0), "1970-01-01 00:00 UTC");
        assert_eq!(format_ms(1_700_000_000_000), "2023-11-14 22:13 UTC");
    }
}
