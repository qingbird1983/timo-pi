//! 每绝对路径文件变更互斥队列（write/edit 共享；pi withFileMutationQueue 同构）。

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

static QUEUE: OnceLock<Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();

fn queue() -> &'static Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>> {
    QUEUE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 取指定路径的互斥锁句柄（同一路径全局唯一）。
fn lock_for(path: &Path) -> Arc<tokio::sync::Mutex<()>> {
    let key = dunce_like_canonical(path);
    let mut map = queue()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    map.entry(key).or_default().clone()
}

/// 规范化为绝对路径（不解析符号链接，避免 IO；与 pi 以 abs path 为键同构）。
fn dunce_like_canonical(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    }
}

/// 在该路径的互斥锁内执行 `f`（write/edit 的文件变更串行化入口）。
///
/// # Errors
/// `f` 的错误原样透传。
pub async fn with_file_mutation_queue<T, F, Fut>(path: &Path, f: F) -> crate::Result<T>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = crate::Result<T>>,
{
    let lock = lock_for(path);
    let _guard = lock.lock().await;
    f().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn serializes_same_path() {
        let p = std::path::Path::new("D:/tmp/timo-mq-test.txt");
        let seq = Arc::new(std::sync::Mutex::new(vec![]));
        let mut handles = vec![];
        for i in 0..4 {
            let seq = seq.clone();
            handles.push(tokio::spawn(async move {
                with_file_mutation_queue(p, || {
                    let seq = seq.clone();
                    async move {
                        seq.lock().unwrap().push(i);
                        Ok(0)
                    }
                })
                .await
                .unwrap();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        assert_eq!(seq.lock().unwrap().len(), 4);
    }
}
