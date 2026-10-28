# P-017 本机 WSL 验 Linux 时的四个假信号与一个真缺陷

## 场景

Phase 4 P1-b 把 NSH 变成桌面常驻依赖后，CI（只有 `windows-latest`）看不见 Linux 侧是否还能构建。用本机 WSL Ubuntu-24.04 跑等价门禁，四条红里三条是环境假信号，一条是产品缺陷。

## 四个坑

1. **WSL 的 `/tmp` 会被清掉**。`wsl.exe -e bash script.sh` 跑完后，若没有附加进程，发行代会在空闲后自动关停，`/tmp`（tmpfs）随之清空。把日志写到 `/tmp/timo-*.log` 再想回头读，第二次进去就只剩空文件——看起来像"任务没跑"。
   - **做法**：验证日志落 `/root/...` 这类持久路径；或让脚本自己 `tail` 到 stdout。
2. **WSL 的 PATH 里混着 Windows 目录**。`/mnt/c/Program Files/Git/usr/bin` 在 PATH 中，于是 Linux 上 `bash_candidates()` 能找到 `bash.exe` 并且它 `uname -s` 返回 `MINGW64_NT-...`。生产代码 `resolve_shell` 本来用 `cfg!(windows)` 关住这条探测，测试却无条件拿 `native_bash()` 与 `resolve_shell` 比较 → **测试假红**，产品行为其实正确。
   - **做法**：带平台分支的生产逻辑，测试要把分支本身写成断言（非 Windows 断言 `(Bash, "bash")`），不能只测 Windows 那一条路。
3. **`std::fs::copy` 的 mtime 语义不跨平台**。Windows 走 `CopyFile` 保 mtime，Linux 不保（新文件拿到当前时间）。任何"两份副本谁赢"的判定只要挂在 mtime 上，Windows 绿、Linux 翻脸。`readdir` 顺序同样不保证。
   - 本仓库的真实缺陷正在这里：`list_sessions` 用 `sort_by_key(modified_ms)` + `dedup_by(id==)` 去重分区副本与旧平铺副本。`dedup_by` 只删**相邻**重复，同 id 不相邻时根本删不掉；而相邻与否又取决于 mtime 与 readdir 顺序。改成"按 id 归并，优先分区布局、其次更新的 mtime"，顺序与平台都无关。
4. **Linux 缺的不是编译器而是 `-dev` 包头**。`libdbus-sys` 的 build.rs 直接 panic 索要 `dbus-1.pc`；gpui-fast/nsh 在 Linux 还需要 alsa/xkbcommon/wayland/EGL/vulkan/fontconfig/xcb 一组 dev 包。实测可用清单：
   `build-essential pkg-config libasound2-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libegl1-mesa-dev libgles2-mesa-dev libvulkan-dev libfontconfig1-dev libx11-dev libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxcursor-dev libxrandr-dev libxi-dev libssl-dev libdbus-1-dev libpulse-dev libxtst-dev cmake`

## 预防

1. **跨平台断言要镜像生产的平台分支**，不要在生产有 `cfg!(windows)` 的地方写无条件比较。
2. **"两份同名数据留哪一份"必须是显式优先级**（布局、显式命名规则），不能是 mtime + 目录顺序这种平台相关量；新增这类去重时，必须写一条"副本更旧也要赢"的判别测试。
3. **WSL 验证把日志放 `/root`**，并在同一次调用里把结论打出来。
4. 本机 WSL 跑 Linux 门禁不占 GitHub 时长；CI 只有 `windows-latest` 时，Linux 面只能靠本机验证，改动涉平台依赖（如 NSH 常驻）后应当轮跑一次。
