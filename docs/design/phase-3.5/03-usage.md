# 设计 03：用量与成本聚合（P0）

- 状态：定稿（2026-10-27）
- 关联：00-overview K8/D34；research/06 §2.6；design/phase-1/01（Usage 不变量）、03-usage 无
- 落点：`crates/timo-kernel/src/usage.rs`（新增），`Kernel` 保存目录引用

## 1. 目标与现状

现状：每条 assistant 消息已带 `Usage`（input 不含缓存、output 含 reasoning，`total_tokens` = 四项之和）；CLI 只在每次 run 后打印**最后一轮**用量；会话文件逐条保存消息，因此重开会话时用量可重建。

目标：

1. 会话级聚合（总计 + 按模型分桶 + 成本），从历史消息推导，不新增落盘格式。
2. 每轮用量仍以事件为准（`KernelEvent::ProviderResponse` 已存在），桌面可实时显示。
3. 为 P1 的缓存统计（命中率、浪费）留出字段与扩展点，但 P0 不计算命中率。

不做：写 `usage` entry、嵌套工具调用的真实合并（P2，D34）、跨会话统计、计费对账。

## 2. 数据模型

```rust
pub struct UsageTotals {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cost: f64,          // USD；未知模型记 0
}

pub struct UsageBucket {
    pub key: String,        // "provider/model" 或 "Tools/summaries"
    pub tokens: u64,        // input+output+cache_read+cache_write
    pub cost: f64,
}

pub struct UsageSummary {
    pub totals: UsageTotals,
    pub buckets: Vec<UsageBucket>,  // 按 cost 降序，cost 相同按 key 升序
}
```

`UsageTotals` 复用 `timo_ai::Usage` 的四项计数器语义；成本用目录里的 `RateCard` 计算（`timo_ai::catalog::calculate_cost`）。

固定字符串 `"Tools/summaries"` 与 pi 的同名桶一致，作为跨实现的对照锚点（也便于将来对齐）。

## 3. 数据来源与归属

| 来源 | 桶键 | P0 |
|---|---|---|
| assistant 消息的 `usage` | `provider/response_model.unwrap_or(model)` | 是 |
| tool result 消息的 `usage`（`Option`） | `Tools/summaries` | 是（当前工具不产出，桶通常为空） |
| `usage` entry（会话格式已预留） | 该 entry 的 `provider/model` | 否（P1 写入后接入） |
| compaction / branch summary entry 的 `usage` | `Tools/summaries` | 否（P1 压缩后出现） |

- 归属规则与 pi 一致：消息级模型归属，工具与摘要单独成桶。
- `response_model` 存在时优先（供应商可能重写模型 id），否则用请求模型。

## 4. 成本

- 目录查找：`Catalog::get(provider, model)`；命中 → `calculate_cost(model.cost, usage)`；未命中 → 成本 0（token 仍计入）。
- 内核在 `Kernel::new` 里 `load_catalog` 后**保存** `Arc<Catalog>`（现状是解析完即丢弃）；P0 不加新依赖。
- 金额单位 USD、不做币种换算；展示精度由界面决定（CLI 两位小数，桌面两位小数）。

## 5. 计算规则

1. 遍历 `history`（`&[Message]`）。
2. assistant：累加四项 + 成本；`input+output+cache_read+cache_write == 0` 的占位消息仍计入 totals（幂等无影响），但不产生桶项（过滤规则见下）。
3. tool result：`usage` 非空时累加到 `Tools/summaries`。
4. 桶过滤：`cost > 0 || tokens > 0`（与 pi 一致）；totals 不过滤。
5. 排序：cost 降序；相同 cost 按 key 升序（保证输出稳定，便于逐字断言）。
6. 不变量（测试断言）：`totals.input` 不含 cache；`totals.cost` 等于各桶成本之和（浮点用 1e-9 容差）。

## 6. 内核 API

```rust
impl Kernel {
    pub fn usage_totals(&self) -> UsageTotals;
    pub fn usage_breakdown(&self) -> Vec<UsageBucket>;
    pub fn usage_summary(&self) -> UsageSummary;   // 二者合一，界面用
}
```

- 全部从 `self.history` 现算；不做缓存（消息量在会话级，成本可忽略）。
- 未开跑（无历史）→ 全零；空桶列表。
- 每轮实时值继续用事件：`ProviderResponse { usage, .. }`；桌面的"本轮"与"累计"分别取事件与 `usage_summary()`。

## 7. 界面接线

| 界面 | P0 落点 |
|---|---|
| CLI | run 后打印 `[usage]` 行保持现状（最后一轮）；`/session` 打印 `usage_summary()`（总计 + 每桶一行） |
| 桌面 | 状态栏"上下文用量"仍来自事件；累计与桶先不做界面（P1 与缓存统计一起设计），避免为临时读数改布局 |
| `/session` 文本 | `session <id> · <model> · N turns · input=X output=Y cache_read=Z cache_write=W · $0.00` |

## 8. 测试

| 用例 | 断言 |
|---|---|
| 两模型两轮 | totals 四项为和；两个桶按成本降序 |
| 工具用量 | tool result 带 `usage` → `Tools/summaries` 桶出现 |
| 未知模型 | token 计入、cost=0、桶仍出现 |
| 占位消息 | 全零 usage → totals 不变、无桶项 |
| 成本合计 | 桶成本之和 == totals.cost（1e-9 容差） |
| 空历史 | 全零、空桶 |
| response_model | 有值时桶键用响应模型 |
| 稳定排序 | 相同成本时按 key 升序，两次调用输出一致 |

## 9. 体量与风险

- 预计 `usage.rs` ≈ 180 行（含测试）；无新文件依赖。
- 风险 1：目录价格是静态数据，供应商调价后成本会偏差；P0 只做展示，不做结算承诺。
- 风险 2：`usage` entry 与摘要用量未接入（D34/P1），跨压缩的会话在 P1 前会少算摘要成本；`/session` 文本不宣称"完整成本"。
