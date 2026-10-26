# 设计 07：测试、fixture 与 CI

- 状态：定稿
- 日期：2026-10-26
- 关联：standards/05（全部硬性义务）；architecture/03（接缝检查点）；roadmap/phase-1 §4 验收场景

## 1. 测试分层落地（standards/05 §1）

| 层 | 落点 | Phase 1 内容 |
|---|---|---|
| 单元 | 各 crate 内联 `#[test]` | 截断、ULID 单调、配置解析、SSE 解码、编辑模糊匹配、JSON 容忍解析… |
| 集成 | `crates/*/tests/` | 会话读写崩溃安全、注册表统一性、事件订阅无需改循环（编译期示例）、kernel 端到端（mock api） |
| 协议 | `crates/timo-ai/tests/` + `fixtures/protocol/` | 回放测试（见 §2） |
| 端到端 | scripts/e2e-*（可脚本化部分）+ 人工 | A1~A6（roadmap §4） |

- 禁网络、禁真实 LLM（standards/05 §3）；时钟/ULID 注入（timo-ai `clock.rs`、timo-session 注入构造）。

## 2. 协议 fixture 计划（standards/05 §2.1）

每协议四类路径 × 各≥1 夹具（`.sse` 文本 = 原始 SSE 字节流 + `.json` 期望终态）：

| # | 路径 | 内容要点 |
|---|---|---|
| 1 | 正常流式 | text/thinking 增量、usage、stop 映射 |
| 2 | 工具调用 | input_json_delta/tool_calls 分片聚合、id 规范化、arguments_error 路径 |
| 3 | 错误与重试 | 429（retry-after）→重试成功；5xx 穷尽→Error 终态；quota 类不重试 |
| 4 | 取消 | 流中途 cancel → Aborted 终态 + 已收增量保留 |

- 来源声明（文件头必注）：SSE 事件序列按官方 API 文档格式手构 + 从 pi 测试（MIT，packages/ai/test/ 内嵌 SSE 样本）提取的事件形状；**不含真实密钥/私有数据**。抓包日期=构建日，标注"文档格式 + pi 测试样本混合来源"。
- 回放器：`mock transport`（reqwest mock 或注入 `HttpClient` trait——取注入方案，避免额外 mock 依赖）。

> 实现回填（2026-10-26）：注入点已落为 `timo_ai::SseTransport`（默认 `ReqwestTransport`）+ `api_with_transport(kind, transport)`。夹具在 `fixtures/protocol/<api>/{text_flow,tool_call,error_*,cancel_midstream}.sse`，期望终态同目录 `.json`；`crates/timo-ai/tests/replay.rs` 逐夹具回放（含取消路径的"已收增量保留"断言）。HTTP 429 重试由 `retry.rs` / `http.rs` 单测覆盖，不在 SSE 回放范围内。

## 3. 会话格式兼容 fixtures（standards/05 §2.2，自 v1 起留存）

`fixtures/session/`：`v1-linear.jsonl`（纯对话）、`v1-tools.jsonl`（含工具调用/结果）、`v1-unknown-type.jsonl`（含 `custom.demo` 未知型行）、`v1-truncated-last.jsonl`（半行结尾）。
断言：可读、投影消息序列快照稳定、未知型跳过、半行丢弃。

## 4. 接缝检查点 → 自动化映射（architecture/03 逐条）

| 接缝 | 自动化 |
|---|---|
| #1 会话格式 | §3 fixtures 回归进 CI；格式文档状态=定稿（本文档批次完成） |
| #2 事件总线 | `timo-agent` 编译期示例测试：外部订阅者仅依赖 KernelEvent 编译运行；CI 脚本 grep `timo-agent/src` 禁止 `extension`/hook 名分支出现 |
| #3 协议 trait | 两适配器共用同一 trait 的泛型测试（同一次循环代码跑两个 mock api）；新协议不动循环由代码结构保证（Phase 5 实证） |
| #4 工具元模型 | 注册表测试：内置 + 模拟 MCP 工具同表注册、同管线执行、仅名字不同 |
| #5 JSON Schema | 工具参数校验测试（合法/非法/缺字段）；全仓 grep 无第二 schema 依赖 |
| #6 富流事件 | 类型级保证（enum）；消费方测试走 StreamEvent 全变 |
| #7 取消贯穿 | run 中途 cancel：HTTP mock 挂起流→Aborted 终态；bash 长进程→树杀+会话文件完好（A3 自动化） |

## 5. CI（GitHub Actions，`.github/workflows/ci.yml`）

- 触发：push/PR（main）。
- job（windows-latest，与首发平台一致）：checkout → Rust stable（dtolnay/rust-toolchain + Swatinem/rust-cache）→ `cargo fmt --all --check` → `cargo clippy --workspace --all-targets -- -D warnings` → `cargo test --workspace`。
- clippy 额外 lint（workspace `Cargo.toml [lints]`）：`unwrap_used = "deny"`、`missing_docs = "warn"`（Phase 1 末评估转 deny，standards/04 §4）、`panic = "deny"`（库 crate）。
- 体量哨兵：`scripts/check-size` 以 warn 输出并入 CI 日志（fail 规则见 standards/08 §4：文件 >500 行直接 fail）。

## 6. scripts/（构建卫生，standards/08 §7）

| 脚本 | 功能 |
|---|---|
| `scripts/check-size`（bash） | ①源文件行数 >500 列出并 fail（生成代码白名单注释豁免）；②target/ 体积对照 15G 软上限（超限 warn）；③`#[allow(dead_code)]` 无注释归属则 fail |
| `scripts/clean-stale`（bash） | 有 cargo-sweep→`cargo sweep -t 30`；无→提示安装并给出手工命令；sccache 在位时才允许 `--full` 全清 |

- `.cargo/config.toml` 按 standards/08 §7 逐字落地（dev line-tables-only、依赖 O2、release strip+thin LTO）。

## 7. 验收场景与测试对应（roadmap §4 A1~A6）

A1 人工执行（真实 LLM）+ mock 版集成测试同路径；A2 `sessions`/`resume` 命令 + §3 断言；A3 = §4 #7 自动化 + 人工复核；A4 集成测试起本地 echo MCP server（stdio，脚本内起子进程）+ 人工一次真实配置；A5 CI 本体；A6 进度报告逐条引用 §4 表结果。

> 实现回填（2026-10-26）：A4 的本地 server 为 `fixtures/mcp/echo_server.py`（Python stdio JSON-RPC，无网络），集成测试 `crates/timo-mcp/tests/stdio_echo.rs` 覆盖惰性连接、命名空间、schema 透传与统一管线调用。
