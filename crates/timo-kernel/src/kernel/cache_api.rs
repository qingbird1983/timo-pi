//! 缓存统计门面（phase-3.5/05 §3）：从当前会话 entry 现算。

use super::Kernel;
use crate::Result;
use crate::cache_stats::{self, CacheStats};

impl Kernel {
    /// 会话级缓存统计；草稿会话返回全 0。
    ///
    /// # Errors
    /// 会话文件读取/解析失败 → [`crate::KernelError`]。
    pub fn cache_stats(&self) -> Result<CacheStats> {
        let Some(id) = &self.session_id else {
            return Ok(CacheStats::default());
        };
        let Some(path) = self.find_session_path(id) else {
            return Ok(CacheStats::default());
        };
        let entries = timo_session::read_entries(&path)?;
        let fallback_read_rate = self.resolved.model.cost.cache_read / 1_000_000.0;
        Ok(cache_stats::from_entries(&entries, fallback_read_rate))
    }
}
