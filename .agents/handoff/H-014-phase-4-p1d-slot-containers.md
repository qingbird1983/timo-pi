# H-014 Phase 4 P1-d：四种 slot 容器与多 kind 运行时

- 日期：2026-10-29
- 范围：Phase 4 P1-d（设计 02 清单字段 / 03 slot / 04 宿主 API / 05 运行时）

## 本轮做了什么

1. **设计口径补齐**（2026-10-29 先改文档再写码）
   - 02：新增 `slots[].tools`——仅 `toolCard` 可用且必填（1~4 个、去重、每个 ≤64 字符），
     其他 kind 出现即 E720。
   - 03：`toolCard` 每个绑定工具一个实例、只显示当前会话最近一次调用（无调用时 `idle`）；
     `entryExtra` 每会话一个；`composerTrigger` 每插件一个；脱敏口径 = 8 KiB 字符截断 →
     凭据形态替换；`composerControl` 仍只声明不实例化。
   - 04：新增 `composer.text` 查询（≤8 KiB、1s）与 `slot.changed {instanceId, data}` 事件。
   - 05：一个 slot 实例一个 `AppView`；单插件实例上限 4。
2. **多 kind 运行时**：期望集 `desired.rs` 由「已启用且无错的插件 × 当前会话」算出，
   实例 key 统一为 `plugin:kind:binding`；`panel` 全局 5、单插件 4，按
   `panel → composerTrigger → toolCard（声明序）→ entryExtra（会话序）` 取舍，超出记 E751。
   装载、1s 健康门、看门狗、版本替换在 `runtime/{load,tick}.rs` 内对所有 kind 共用。
3. **数据面**：启动参数 `set_params(kind, instanceId, data, locale)`；运行期变化用
   `slot.changed` 按实例去重推送。`toolCard` 数据 `{toolName,status,input,output}`、
   `entryExtra` `{sessionId,title}`、`composerTrigger {text}`；`composer.text` 与触发器共享
   同一份输入快照。脱敏先按字符边界截 8 KiB，再替换 `sk-*`/`gh?_*`/`github_pat_*`/`AKIA*`/
   `Bearer *`/`key|secret|token|password` 赋值（`=`、`:`、`=>`）。
4. **容器渲染**（宿主只画框，插件画内容）
   - 聊天：`ToolCards` trait 隔离 NSH 类型，工具卡挂在对应 trace 行下方（≤132px）。
   - 侧栏：`entryExtra` 渲染在会话条目下一行（≤48px，不拦截行点击）。
   - 输入区：`composerTrigger` 渲染在输入框上方（≤32px）。
   - 全部只取健康门通过的实例；每帧聊天快照仅在确有插件声明相关槽位/查询时构建。
5. **体量**：`runtime/mod.rs` 拆出 `tick.rs`，`chat.rs` 拆出 `chat_trace.rs`，
   内核 `host/mod.rs` 的测试移到 `host/tests.rs`；全部 ≤500 行。

## 验证

- `cargo fmt --all`、`cargo clippy --workspace --all-targets -- -D warnings` 干净。
- `cargo test --workspace`：**510 passed / 0 failed**（新增清单 `tools` 校验、`composer.text`
  查询与截断、期望集排序/上限、脱敏、`latest_tool_calls` 等 15 条）。
- `scripts/check-size` OK；`target/` 20.6GiB（deps 16.7GiB），收尾按标准 08 处理。
- 人工验收未做：真实 NSH 包的四种容器渲染需真机（CI 只跑拒绝路径与纯数据测试）。

## 遗留与下一步

- **P2**：devkit（06）与自证插件（07）。
- 健康门无首帧信号、`stop()` 同步 join 不可超时（设计 05 §3/§7 实现注记），平台 API 到位再补。
- `composerControl` 仍只校验声明、不实例化；等设计给出交互契约再实现。
- 本地提交尚未 push；推送后核远端 CI。

## 已提取的沉淀

- 新增 [P-019](../pitfalls/P-019-interpreter-rewrite-loses-source.md)：大段搬移代码不要用解释器脚本
  一次读写，脚本失败时目标文件可能已被覆盖。
- P-018 的跨模块 `pub(super)` 提醒在本次拆 `tick.rs` 时再次适用，未重复记录。
