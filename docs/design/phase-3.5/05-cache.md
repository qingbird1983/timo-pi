# Phase 3.5 / 05：缓存统计与预热

- 状态：定稿（2026-10-27）
- 关联：research/06 §4；pi `core/cache-stats.ts`、`core/cache-warmer.ts`（只读参照）；`design/phase-3.5/03-usage.md`

## 1. 目标

D4 的缓存半场：把"本应命中前缀缓存、却被重新计费"的 token 找出来（统计），并在经济上划算时提前续命缓存（预热）。两者都不改变模型请求的内容，只读取已有 usage 与既有请求。

## 2. 范围与偏离

| # | 偏离 | 类型 | 说明 |
|---|---|---|---|
| D41 | 统计从**会话 entry** 现算，不新增落盘字段 | b（会话格式只增） | 与 pi 一致：`message` 的 usage + `session.compaction` 边界 + `usage(cache_warm)` 条目就是全部输入。 |
| D42 | 预热需要模型声明 `cache_ttl_seconds`；没有声明的模型直接停用 | c（目录数据不全） | OpenAI 兼容网关多为自动缓存、无 TTL 可控概念；我们只给 Anthropic 目录补 300 秒。 |
| D43 | thinking 非 Off 时不做预热 | a（适配器约束） | 我们的 Anthropic 适配器把 `budget_tokens` 夹在 `max_tokens` 内，`max_tokens=1` 的重放会得到非法请求；pi 用 `forceAdaptiveThinking` 判断，我们没有该标志。 |
| D44 | 预热用量记 `usage(cache_warm)` entry，暂不计入 `/session` 总量 | d（P1 范围） | 与压缩摘要用量同策略；P2 统一计入。 |
| D50 | 预热任务只在 run 收尾排定；`streaming` 续一次即停，`idle` 在 30 分钟窗内重复 | d（范围收敛） | 循环内部没有"空闲点"回调；pi 在流式/工具执行期间续命，我们退到 run 级挂点。经济账与安全窗仍按 §4.2 执行。 |

统计与预热的**开关**：`cache_warming: "off" | "streaming" | "idle"`，缺省 `"streaming"`（pi 同款）。统计本身无开关，始终可查。

## 3. 缓存统计

### 3.1 输入

当前会话文件的全部 entry（`read_entries`），逐条扫描：

- `message` 且 role = assistant：候选请求；
- `session.compaction`：上下文合法变化，**重置**上一条请求基线（否则会把新提示误判成 miss）；
- `usage` 且 `payload.kind == "cache_warm"`：把预热请求当作上一条请求（它刷新了缓存），更新基线；
- 其他 entry 忽略。

### 3.2 判定（对齐 pi）

```text
prompt_tokens = input + cache_read + cache_write
baseline: 上一条请求的 prompt_tokens / model_key / timestamp / reported_cache
```

一次 assistant 请求算作 miss 的条件全部满足：

1. 有基线，且本请求 `prompt_tokens > 0`；
2. 本请求 `cache_read + cache_write > 0`，**或**基线曾报告过缓存活动（`reported_cache`）；否则视为"该供应商根本不上报缓存"，不计；
3. `missed_tokens = min(前次 prompt_tokens, 本次 prompt_tokens) - cache_read`；
4. `missed_tokens > 1024`（噪声地板：缓存断点对齐造成的零头）。

代价：`paid_rate = (cost.input + cost.cache_write) / (input + cache_write)`；`read_rate = cost.cache_read / cache_read`（为 0 时回退目录 `cache_read` 费率）；`missed_cost = missed_tokens × max(0, paid_rate − read_rate)`。

附带信息：`idle_ms = 本次 timestamp − 上次 timestamp`（> 5 分钟时在输出里提示缓存 TTL 可能已过）、`model_changed`（`provider/model` 变化，不豁免，因为换模型会重新计费）。

### 3.3 输出

```rust
pub struct CacheMiss {
    pub missed_tokens: u64,
    pub missed_cost: f64,
    pub idle_ms: u64,
    pub model_changed: bool,
}
pub struct CacheStats {
    pub missed_tokens: u64,
    pub missed_cost: f64,
    pub miss_count: usize,
    pub misses: Vec<CacheMiss>, // 与会话顺序一致，供后续 UI
}
```

- `Kernel::cache_stats() -> Result<CacheStats>`：草稿会话返回全 0；读文件失败向上报错。
- CLI `/session` 在 `miss_count > 0` 时追加一行：
  `cache: 3 misses · 12480 tok re-billed · $0.0123`；某次 `idle_ms > 300000` 时再加 `(last miss after 7m idle; TTL may have expired)`。
- 桌面 P1 只在会话结束时刷新 `/session` 对应的数据（状态栏费用已含在 tokens/cost 中），不新增面板。

## 4. 预热

### 4.1 前提

同时满足才进入 `Scheduled`：

1. `cache_warming != "off"`；
2. 模型声明了 `cache_ttl_seconds`（秒）；
3. 当前 thinking 档位为 Off（D43）；
4. 上一次真实请求的 `prompt_tokens > 0`。

任一不满足 → `Inactive { reason }`。

### 4.2 调度

- `delay = max(1, min(ttl×0.9, ttl−10s))`；`ttl ≤ 10s` 视为不可用。
- 安全窗：`streaming` 模式最长 60 分钟（从真实请求开始算），`idle` 模式 30 分钟；刷新截止时间 = 计划时间 + 剩余余量的一半，错过就停（迟到的刷新可能是全价缓存写，不是续命）。
- 每次刷新前算经济账：
  `warm_cost = price(cache_read × prompt_tokens + 1 output)`
  `miss_cost = price(cache_write × prompt_tokens) − price(cache_read × prompt_tokens)`（无 cache_write 费率的供应商用 input 费率）
  `probability = 1`（streaming）/ `0.15`（idle）
  `expected_savings = probability × miss_cost − warm_cost`；`≥ $0.05` 才发。
  价格未知（prompt 或费率为 0）→ 停。
- 刷新请求：原 `Context` 原样重发，`max_tokens = 1`、`max_retries = 0`、同一取消令牌；忽略流式输出；成功且 `stop_reason ∉ {Error, Aborted}` 时写 `usage` entry：
  `{"kind": "cache_warm", "provider": "…", "model": "…", "usage": {…}, "timestamp": 1760000000000}`（`timestamp` 供统计算 idle 间隔）。
- 失效条件：会话切换/新建、模型变化、上下文变化（新 run 开始）、新模式为 off、截止时间错过 → 取消定时器。
- 模式 `streaming`：run 结束后续一次，成功即停；`idle`：run 结束后继续按 30 分钟窗重复（D50）。

### 4.3 状态

```rust
pub enum WarmingState {
    Inactive { reason: String },
    Scheduled { next_warm_at_ms: i64 },
    Refreshing,
}
```

`Kernel::cache_warming_status()`；CLI `/session` 追加 `warming: scheduled in 4m / inactive (no cache ttl)`。

## 5. 模型目录

`Model` 与 `CustomModel` 新增可选字段 `cache_ttl_seconds: Option<u64>`（serde default；不写 = 未知）。`assets/models/anthropic.json` 的每个模型补 `"cache_ttl_seconds": 300`（适配器只发 5 分钟 ephemeral 缓存）；OpenAI 目录不补。

## 6. 内核装配

- 预热上下文来源：`RunOutcome` 增加 `last_request: Option<WarmRequest>`（system prompt、messages、tools、thinking、model id）。agent 在发最后一次请求前保存快照；kernel 在 run 结束后交给 `CacheWarmer`。
- 预热任务与内核同生命周期：`Kernel::drop` 取消；`new_session`/`resume`/每次 `run` 开始时取消旧任务（用取消令牌等价实现原计划的 `generation` 计数器）。
- 预热写 entry 复用 `SessionWriter`（`append_raw(kinds::USAGE, …)`），与消息写入同一把锁。

## 7. 测试

单测：

1. 首次请求不计数；全零缓存且从未报告 → 不计数；`missed_tokens ≤ 1024` → 不计数。
2. 正常 miss：上一轮 20000 prompt、本轮 cache_read 1500 → 18500 missed；代价按费率计算正确。
3. 压缩 entry 后基线重置：下一轮不因摘要变化记 miss。
4. `cache_warm` 条目作为基线：后续真实请求能命中。
5. 模型切换：`model_changed = true` 且照常计数。
6. 调度：`delay` 公式、10 秒下限、60/30 分钟窗、错过截止停。
7. 经济账：`expected_savings` 低于阈值停；费率未知停；thinking 非 Off 停。
8. 目录：JSON 缺 `cache_ttl_seconds` 反序列化为 None；anthropic 样本读到 300。

集成（fixture SSE）：

9. 两轮 fixture 请求 → `/session` 出现 miss 行（构造 cache_read 断档）。
10. 预热任务用一个 1 秒 TTL 的测试模型触发，断言发出 `max_tokens=1` 的请求并写入 `usage(cache_warm)`；`off` 模式不产生请求。
11. 取消：run 开始/换会话后旧定时器不再发请求（计数为 0）。

## 8. 体量

- `crates/timo-kernel/src/cache_stats.rs` ≈ 200 行 + tests；
- `crates/timo-kernel/src/warmer.rs` ≈ 220 行 + tests；
- `timo-ai` 目录/类型 +15 行；CLI 输出 +10 行。
