# P-014 共享活值必须在每个宿主构建点透传，默认值会把漏线藏起来

## 现象

批 B 的 `ctx.isProjectTrusted()` 在真机上恒为 `true`：扩展明明投了"不信任"，项目资源也确实没加载（stderr 有 `project not trusted`），可扩展自己的探针工具依旧返回 `TRUST=true`。同一时刻，宿主层单测 `is_project_trusted_reads_shared_flag` 与内核层决策链测试全绿。

## 根因

两处接线漏了，且都被"默认 `true`"吃掉：

1. `BasicServices` 有 `project_trusted: Arc<AtomicBool>`（默认 `true`，沿用批 A 前的行为），但 `load_extensions_with` 构建**逐扩展** services 时没有 `.with_project_trusted(...)`；宿主级 base services 与逐扩展 services 是两个构建点，只接一个就有一半路径读默认值。
2. `Kernel::new` 的 pre-trust 装载用了 `Arc::new(AtomicBool::new(false))` 而不是 `Arc::clone(&ext_project_trust)`；决定之后复用的是 pre-trust 宿主，它握着那份独立 `Arc`，永远停在 false/true 的初始值。单测只覆盖"内存里的开关会翻转"，不覆盖"内核是否把同一个开关交给了宿主"。

## 修复

- 两个构建点都 `.with_project_trusted(Arc::clone(&options.project_trusted))`。
- pre-trust 装载与最终装载共享同一个 `Arc`。
- 补内核级断言：扩展命令处理器在 `!ctx.isProjectTrusted()` 时抛 `UNTRUSTED`，分别覆盖"命令行覆盖"与"pre-trust 扩展决定"两条路径；pre-trust 用例必须让决定为 `yes`（决定为 `no` 时，独立 `Arc` 的初值恰好也是 false，测不出漏线——这就是变异 M7 最初没有变红的原因）。

## 预防

1. 新增"宿主可见的状态"时，先列**所有** constructing 点（base services、逐扩展 services、测试夹具），逐个接；默认值要选"漏接时测试会红"的那一侧，或干脆不给 `Default`。
2. 活值类断言必须走**真机或端到端**：单元测试只证明开关本身可用，证明不了装载路径把同一个开关递了进去。
3. 变异方向要覆盖"决定为 true"的一侧；只测 false 的用例会被初值巧合救活。
4. 真机夹具用扩展命令/工具把值**打回主进程可见的输出**（本例：`probe_trust` 工具文本 `TRUST=<bool>`），比读日志推断可靠。

## 归属

phase-3.5/12 批 B 真机验收（2026-10-28）；修 `crates/timo-kernel/src/kernel.rs`、`crates/timo-kernel/src/extensions/mod.rs`，补 `crates/timo-kernel/tests/trust.rs` 两条断言。
