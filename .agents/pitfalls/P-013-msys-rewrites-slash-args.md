# P-013 Git Bash 把 `/命令` 改写成 `C:/Program Files/Git/命令`

## 现象

在 Windows 的 Git Bash / MSYS2 里给 `timo -p` 传以 `/` 开头的提示（`/hello`、`/skill:secret`），真机验收显示模板与技能"没有展开"：模型收到的是 `C:/Program Files/Git/hello` 这类绝对路径，甚至跑去列目录。

同一夹具在 Rust 集成测试里 `expand_input("/hello")` 与 `has_template("hello")` 都正常。

## 根因

MSYS 的参数路径转换（POSIX → Windows）会改写**任何看起来像 Unix 绝对路径的参数**，包括 `/hello`。转换发生在进程启动之前，`timo` 收到的已经是 Windows 路径，与内核逻辑无关。

## 修复

验收命令加 `MSYS_NO_PATHCONV=1`（或 `MSYS2_ARG_CONV_EXCL='*'`）：

```bash
MSYS_NO_PATHCONV=1 ./target/debug/timo.exe -p "/hello"
```

## 预防

1. 真机验收斜杠命令前，先确认外部壳没有改写参数：传一个明显不是路径的写法对照，或直接在 PowerShell/cmd 里跑。
2. 出现"命令没生效"时先分叉：**内核行为**（集成测试可复现）还是**传参被改写**（只在某个 shell 下出现）。不要先改内核。
3. 以 `/` 开头的 CLI 参数在 Windows 文档里给出反例；子代理示例若从扩展里拼命令，应避开 shell 词法直传 argv。

## 归属

phase-3.5/12 批 A 真机验收（2026-10-28）；无代码改动。
