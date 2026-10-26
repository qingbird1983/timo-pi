# Phase 2 阶段进度报告：编排类扩展宿主（A 轨）

- 状态：完成
- 日期：2026-10-26
- 关联：design/phase-2/00~05；roadmap/phase-2；ADR-0006/0011；architecture/02~04
- 结论：B1~B6 全部通过；真实 DeepSeek 会话中扩展工具、钩子、权限与诊断均按设计生效。

## 1. 交付概览

| 模块 | 落点 | 要点 |
|---|---|---|
| 发现 | `timo-ext/src/discovery.rs` | 项目/项目 pi/用户/显式四来源；安装目录优先读 manifest 入口；同名按优先级取舍 |
| 打包 | `timo-ext/src/pack.rs` | rolldown 1.2 + shim 别名；单文件 `bundle.cjs`；禁用面扫描 E104/E105；顶层 await E106 |
| 宿主 | `timo-ext/src/host*` | 每扩展一个 QuickJS Context、单专用线程；`module/exports` 单次求值；30s 中断 |
| shim | `timo-ext/src/shim/*` | pi 1.4.0 面、惰性 Proxy、`VERSION="1.4.0"`、全局 fetch、命令执行 |
| 钩子 | `timo-agent`、`timo-ai`、`timo-ext/src/bridge.rs` | `LoopHooks` / `ProviderHooks`；循环内无扩展分支；abort 退休 |
| 权限 | `timo-ext/src/permissions.rs` | 声明→授予双闸；域名/整树/硬拒规则；`permissions.json` 绑定源码指纹 |
| 内核 | `timo-kernel/src/extensions/*` | 装载、工具并入注册表、事件转发、诊断报告、`session_start` |
| CLI | `apps/timo-cli` | `ext list/add/remove/trust/doctor`；`--extension`/`--no-extensions`/`--ext-flag`；`/ext` 与扩展命令 |

## 2. 验收结果（B1~B6）

| # | 场景 | 结果 | 证据（真实运行，模型 `deepseek-flash`） |
|---|---|---|---|
| B1 | 无依赖扩展在 CLI 会话真实生效 | ✅ | 自写 `hello`：模型调用 `plugin_hello_hello`，`tool_call` 钩子把参数改为 `qoder (via hook)`，回复 `Hello, QODER (VIA HOOK)!`；自写 `guard`：模型请求 `rm -rf /tmp/timo-guard-target`，钩子否决，工具 0 ms 未执行，目标目录不存在，stderr 出现 `[ext:warning] guard: blocked a destructive command` |
| B2 | 纯 TUI 扩展不崩溃、逐项诊断 | ✅ | 自写 `tui`：工具返回 `ui-noop-ok`；报告显示 3 条降级（`setWidget`/`setFooter`/`custom`），会话正常结束 |
| B3 | 钩子语义专项 | ✅ | 自写 `slow`（`before_agent_start` 死循环）+ `TIMO_EXT_BUDGET_MS=300`：报告记 E301，模型仍正常回复 `budget-test-ok`；中断后同一 Context 继续可用由 `context_is_reusable_after_budget_interrupt` 断言；abort 退休/错误隔离/白名单回读见 `tests/bridge.rs`、`tests/hooks.rs` |
| B4 | 权限专项 | ✅ | 未声明网络：`E402 net denied: example.com`，诊断逐项留痕，测试证明 0 连接；声明 `api.example.com` 后仍请求 `example.com` → E402；`exec` 未授予 `E401`，`timo ext trust exec` 后同一工具输出 `timo-exec-ok`；`write:["**"]`/`delete:["C:\\"]` → E403，扩展 failed；`.env`/`.ssh`/`.git`/私钥 → E404 |
| B5 | 打包失败透明 | ✅ | 自写 `node-fs` 调 `timo ext add`：`pack failed (E104): bundle references node:* built-ins`，退出码 1，失败安装目录已清理 |
| B6 | 接缝与 CI | ✅ | 门禁四连通过；185 个测试全绿；接缝 #2 循环无扩展分支、#4 工具全字段映射、#6 富流事件、#7 取消贯穿 |

### B3 补充说明

- 单 handler 30s 预算：生产 30s；测试用 `TIMO_EXT_BUDGET_MS` 注入（设计 04 §5 的「可注入」要求）。
- payload detach：内核 → JS 每次 `serde_json` 序列化为新对象；只有设计 04 §6.3 白名单回读，其余改动不生效。
- abort 退休：桥在派发前检查共享取消标志；在飞 handler 由 QuickJS 中断处理器切断，结果丢弃。

## 3. 真机测试方法

1. 临时数据目录：`models.json` 声明深寻供应商（`openai-completions`，`api_key: "$DEEPSEEK_API_KEY"`），`settings.json` 指定默认模型；密钥仅从环境变量读取，未打印、未入库。
2. `TIMO_AGENT_DIR=<临时目录> timo ext add fixtures/ext/<name>.ts` 安装；`timo ext trust` 授权。
3. `timo run --provider deepseek --model deepseek-flash "<prompt>"` 驱动模型选择扩展工具或执行命令。
4. 观测量：stdout 工具起止行、扩展输出、usage、stderr 诊断与 `extensions-diagnostics.json`。

## 4. 偏离与限制

| # | 内容 | 处置 |
|---|---|---|
| D27 | 产物为 CJS（`bundle.cjs`），非 ESM；顶层 await → E106 | 已登记设计 00/02 |
| D28 | 安装期静态扫描的「建议能力」写入 manifest 作为**声明**（仍须用户授予） | 已登记设计 00/02；否则安装出的扩展永远无法申请 exec |
| D29 | 通知队列无界（设计原为 1024 有界丢最旧） | 当前无订阅者直接丢弃、有订阅者顺序投递；长会话积压风险记 backlog |
| — | `route_model`/虚拟模型：本版恒 `None` | D23 收敛；`registerVirtualModel` 仅诊断 |
| — | `registerProvider`/`registerMcpServer`/`sessionManager`/`fs.*` | no-op + E202/E302 诊断；fs 只有声明校验 |
| — | `ui.select/confirm/input` 仅 TTY 行交互；非 TTY 返回取消 | D16；未做自动化真机断言 |
| — | `message_update` 已接线，但未在真实样本中消费 | 映射覆盖 Start/BlockStart/Delta/BlockEnd/Done/Error |
| — | 扩展命令名带 `plugin_<id>_` 前缀；REPL 支持后缀匹配 | K9；一次性 `timo run` 不进入命令循环 |

## 5. 门禁与体量

- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（185 通过）、`bash scripts/check-size` 全部通过。
- Rust 单文件 ≤500 行：拆分 `timo-ext/host/{api,types,runner}`、`timo-kernel/extensions/{mod,cli,events}`、`timo-agent/run/{mod,tools}`。
- `target/` 约 15 GB 触及软上限；`scripts/clean-stale` 需 `cargo-sweep`（本机未安装），未执行清理，仅记录。

## 6. 已知风险

1. **通知队列无界**（D29）：高频 `message_update` 下内存随积压增长；下一步实现有界丢弃计数。
2. **权限提示**：非 TTY 一律拒绝，脚本化场景需显式 `timo ext trust`；交互提示为一次性 y/N。
3. **打包依赖 rolldown 1.2 次版本**：已锁次版本；升级按 standards/08 §5 窗口化，适配层集中在 `pack.rs`。
4. **TUI 行为**：no-op 不抛错，但依赖 UI 回调的扩展无法真正工作（B2 仅要求透明降级）。
5. **pi 样本未入库**（铁律 7）：B1 使用自写等价夹具；人工可另用本机 pi 官方样本复验。

## 7. 收尾清单

- [x] 设计文档回填（D27/D28/D29、发现入口、预算注入、夹具说明）
- [x] 看板与阶段文档状态
- [x] CHANGELOG 0.1.19
- [x] `.agents/CONTEXT.md` 交接卡
- [x] tag `phase-2.end`
