# P-003 Windows bash 落到 WSL 启动器

## 现象

真实任务里模型的 shell 命令跑在 WSL：项目 Windows 路径 `d:/…` 在 Linux 侧不存在，`cd` 失败；stdout 混入 `wsl: 检测到 localhost 代理配置…` 警告；首次 cat 耗时 18 秒。

## 根因

Windows 上 `on_path("bash")` 只要 PATH 里有 `bash.exe` 就认为可用。System32 的 `bash.exe` 是 WSL 启动器，不是原生 POSIX 层；`Command::current_dir(Windows 路径)` 对它无效。

## 修复

探测改为执行 `bash -c "uname -s"`，仅当输出含 `mingw`/`msys`/`cygwin` 时使用 bash，否则按 D1 回退 PowerShell（结果缓存，避免每次工具调用都探测）。提交 `2f725e3`。

## 预防

- Windows 上探测外部工具要验证**行为**（能认工作目录/是本平台原生实现），不能只看文件存在。
- CI 的 windows-latest 没有 WSL 干扰，但开发机可能有——探测逻辑的单测要覆盖"Linux uname → 不可用"分支。
