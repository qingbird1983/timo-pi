# Phase 3.5 / 10：provider composer 与虚拟模型（P2 第二题）

- 范围：pi `core/provider-composer.ts`（737 行）的"配置 → 请求"面，加 `core/virtual-models.ts`（238 行）的路由面。
- 蓝本（只读，MIT 不得复制）：`composeModelProvider` / `ProviderConfigInput` / `ProviderChatModelConfig` / `resolveCompatibilityRequestConfig` / `configuredRequestAuthStatus`；`VirtualModelDefinition` / `ModelRouteRequest` / `ModelRoute` / `getBranchSelection` / `getVirtualModelState` / `withVirtualModels`。
- 前置：05（模型目录与凭证）、05 §5（解析优先级）、08 §5（裸 id 解析）。

## 1. 目标（一句话）

把"模型从配置到请求"这段补齐到 pi 的表达力（请求头、认证头开关、兼容性开关可按供应商/模型声明），并让"一个名字、路由到真实模型"的虚拟模型能注册、能路由、能把选择与状态存进会话。

## 2. 蓝本行为（只读摘录，不含实现代码）

### 2.1 配置面

| 面 | pi 的行为 |
|---|---|
| 供应商配置 | `name` / `baseUrl` / `apiKey`（字符串、`$ENV`、命令三种形态）/ `api` / `headers` / `authHeader` / `models[]` / `oauth` / `refreshModels()` |
| 模型配置 | `id` / `name` / `api?` / `baseUrl?` / `input[]` / `inputLimits?` / `cost` / `headers?` / `reasoning` / `thinkingLevelMap?` / `promptCache?` / `contextWindow` / `maxTokens` / `samplingParams?` / `samplingParamsByThinkingLevel?` / `compat?`；另有 `image` / `classifier` 两种类型 |
| 请求头合并 | `resolveCompatibilityRequestConfig`：模型自身 headers 与配置 headers（先供应商、后模型）合并；`authHeader` = 扩展 > 配置 > `false` |
| 认证状态 | `configuredRequestAuthStatus` 区分来源：`environment`（带变量名）/ `models_json_key` / `models_json_command` / `fallback` / `stored` / `runtime` |

### 2.2 虚拟模型

| 面 | pi 的行为 |
|---|---|
| 画像 | 目录条目 = `{provider, id, name}`，api 标记 `pi-virtual`；挂在一个（可有物理模型的）供应商下；id 不得与同供应商的物理模型重复。列出的 limits 只在首个响应前生效，之后改用实际回答的物理模型。 |
| 路由 | `route(request) → {model, thinkingLevel, state?}`；`request.reason` ∈ `user`（用户消息后的首个请求）/ `continuation`（工具结果后的续轮）/ `retry`（失败重试，含压缩后重试）/ `direct`（旁路请求：压缩摘要、扩展调用）；`previous` = 最近一次成功响应的物理模型；`failed` = 失败的那次请求；`state` = 本分支上次返回的路由状态。 |
| 状态 | 路由返回的新状态写进会话分支的 `pi.virtual-model-state` 条目 `{provider, modelId, state}`；`direct` 请求不存。 |
| 未路由就请求 | 报错：`Virtual model <provider>/<id> must be routed before streaming`。 |
| 分支选择 | 虚拟模型的 `model_change` 一直保持到下一次变更（响应里记的是物理模型）；虚拟模型注销后不再保持，退回最后响应的物理模型。 |
| 失败的路由 | 失败请求的消息上仍记虚拟模型名。 |

## 3. 我们的现状与缺口

| # | 缺口 | 现状 |
|---|---|---|
| G1 | `Compat` 恒 `Default`，models.json 无处声明 | `Compat{max_tokens_field, supports_usage_in_streaming, supports_finish_reason}` 已存在，只是没有配置入口 |
| G2 | 请求头只有供应商级 | `CustomProvider.headers` 有；模型级没有；`authHeader` 完全没有（内置 adapter 一律带 `Authorization: Bearer`） |
| G3 | 扩展 `registerProvider` / `registerVirtualModel` 是 **noop** | `shim/runtime.js` 里 Phase 2 占位 |
| G4 | 目录只有两种物理协议，没有虚拟条目 | `ApiKind::{AnthropicMessages, OpenaiCompletions}` |
| G5 | 请求模型在装配时固定 | `LoopConfig.model` → `RunState.model`，一轮里不会变 |
| G6 | 会话没有路由状态与选择语义 | 会话只有 message/compaction 等条目；无 `model_change`，无虚拟模型状态 |

## 4. 接口契约

### 4.1 批 A：配置面（`models.json` 新字段，全部可选，缺省保持现状）

```jsonc
{
  "providers": {
    "gateway": {
      "api": "openai-completions",
      "base_url": "https://gw.example/v1",
      "auth_header": false,          // false = 不带协议自带的认证头（用 headers 自己带）；不写 = 照旧发送
      "headers": [["X-Org", "acme"]],
      "models": [{
        "id": "m1",
        "headers": [["X-Model", "v1"]],   // 模型级，覆盖供应商级同名头
        "compat": { "max_tokens_field": true, "supports_usage_in_streaming": false, "supports_finish_reason": false }
      }]
    }
  }
}
```

- 合并顺序（后者覆盖同名）：内置默认 → `provider.headers` → `model.headers` → 扩展 `before_headers` 钩子（既有）。
- `auth_header: false` 时 adapter 不发 `Authorization`；凭证仍按既有三级解析（显式 > 引用 > env），由 adapter 决定放在哪里（批 A 只做"不发"）。
- `Compat` 三个字段逐项透传到 adapter（它们已被 adapter 使用，只是此前不可配）。

### 4.2 批 B：虚拟模型

- **目录条目**：`Model.kind = Physical | Virtual`；虚拟条目 `api` 记为 `ApiKind::Virtual`，`cost` 全 0，`thinking_levels` 记录可选档位。`catalog.get(provider, id)` 可命中；`ListModels`/`--model` 能选中。
- **不可直接请求**：任何把虚拟条目送到 adapter 的路径返回 `ModelMustBeRouted`（文案对齐蓝本）。
- **路由契约（Rust 侧）**：

```rust
pub enum RouteReason { User, Continuation, Retry, Direct }
pub struct RouteRequest<'a> {
    pub model: &'a Model,
    pub thinking: ThinkingLevel,
    pub reason: RouteReason,
    pub previous: Option<(&'a Model, ThinkingLevel)>,
    pub failed: Option<(&'a Model, ThinkingLevel)>,
    pub state: Option<&'a serde_json::Value>,
    pub messages: &'a [Message],
}
pub struct RouteChoice {
    pub model: Model,
    pub thinking: ThinkingLevel,
    pub state: Option<serde_json::Value>,
}
```

- **路由来源**：批 B2 先做"扩展注册的 JS route 函数"（`registerVirtualModel` 实装，host 每次请求前同步调一次 JS）；内置/声明式路由不做（偏离 D64）。
- **reason 判定点**（loop 内）：用户消息后的首个请求 = `User`；工具结果后的续轮 = `Continuation`；重试（含压缩后重试）= `Retry`；压缩摘要与扩展旁路调用 = `Direct`。
- **状态与恢复**：新 entry `session.virtual_model_state {provider, model_id, state}`，只增不改；按分支取最后一条同 `(provider, model_id)` 的记录；`Direct` 不写。
- **分支选择**：恢复会话时，若分支上最后一条 `session.model_change` 指向仍注册的虚拟模型，则保持该选择；否则退回最后一条助手消息记录的物理模型。我们目前没有 `model_change` 条目，批 B4 一并加（`session.model_change {provider, model_id, at}`）。

## 5. 批次切分（每批独立门禁 + 提交）

| 批 | 内容 | 交付判据 |
|---|---|---|
| A | 4.1 配置面 | 单测覆盖合并顺序与 `auth_header` 两个方向；真机 DeepSeek 无回归 |
| B1 | 目录与画像（`kind`、`ApiKind::Virtual`、不可直接请求、清单可见） | 选中虚拟模型不路由时报出蓝本文案 |
| B2 | 路由契约 + loop 内 reason 判定 + 扩展注册（`registerVirtualModel` 实装） | 扩展注册一个"按 reason 换模型"的虚拟模型，真机两次请求落到不同物理模型 |
| B3 | 状态与分支选择（`virtual_model_state` / `model_change` entry、恢复语义） | 重启后选择与路由状态可复现 |

## 6. 验收

| # | 场景 | 通过标准 |
|---|---|---|
| T1 | 头合并 | 供应商级与模型级同名头，模型级胜；`before_headers` 钩子再覆盖 |
| T2 | `auth_header: false` | 抓请求：无 `Authorization`；`true`/缺省：有 |
| T3 | compat 透传 | `max_tokens_field=true` 时请求体用 `max_tokens`，缺省用 `max_completion_tokens` |
| T4 | 虚拟条目不可直连 | `--model provider/virtual` 且未注册路由 → 明确报错，不发出网络请求 |
| T5 | 路由换模型 | 同一虚拟模型，`User` 与 `Continuation` 返回不同物理模型；真机读数两轮分别记录不同 provider/model |
| T6 | 状态往返 | 路由返回 state → 落 entry → 下次请求 `request.state` 原样可取 |
| T7 | 分支选择 | 恢复会话：虚拟选中保持；虚拟注销后退回最后响应的物理模型 |
| T8 | 失败路由 | 路由抛错时消息仍记虚拟模型名，错误可见 |
| T9 | 回归 | 既有 workspace 测试一字不改全绿 |

## 7. 体量

| 项 | 预算 |
|---|---|
| `timo-config`（models.json 新字段） | +60 行 |
| `timo-ai`（catalog kind / 头合并 / compat 透传） | +120 行 |
| `timo-kernel`（虚拟注册、路由调用、entry） | +220 行 |
| `timo-agent`（reason 判定、按请求换模型） | +150 行 |
| `timo-ext`（registerVirtualModel 桥） | +120 行 |
| 单文件仍受 500 行硬限；超了就按"内核/CLI/桌面拆分"的既有先例拆 |

## 8. 偏离登记（接续 09 的 D63）

| # | 偏离 | 类别 | 说明 |
|---|---|---|---|
| D64 | 不做声明式路由，route 只能由扩展的 JS 函数提供 | a（对齐蓝本） | 蓝本 route 就是函数；我们把它实装到扩展通道（§4.2） |
| D65 | 只支持 chat 类虚拟模型 | d（范围收敛） | 我们只有 chat 通道；image/classifier 模型类型不在本阶段 |
| D66 | 会话 entry 用 `session.virtual_model_state` / `session.model_change` | a（自有会话格式） | 沿用本项目 `session.*` 前缀（设计 05） |
| D67 | 分支选择按线性链实现 | b（实现更简） | 我们的会话是线性链（D47 同源），"最后一条 model_change 胜"退化为倒序查找 |
| D68 | `oauth` / `refreshModels` / `thinkingLevelMap` / `samplingParams` 不做 | d（范围收敛） | 我们是自有 adapter，OAuth 与动态刷新属另一条能力线；按需再立题 |
