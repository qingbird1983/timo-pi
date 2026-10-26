# 设计 05：timo-config —— 数据目录、设置与模型解析

- 状态：定稿
- 日期：2026-10-26
- 关联：ADR-0003/0011；设计 01 §6~7、03 §1、04 §1；pi: coding-agent/src/config.ts、core/settings-*.ts、model-resolver.ts

## 1. 目录布局（D7：平台惯例；env `TIMO_AGENT_DIR` 覆盖一切）

| 平台 | 数据目录（`dirs::data_dir()/timo-pi`） |
|---|---|
| Windows | `%APPDATA%\timo-pi` |
| macOS | `~/Library/Application Support/Timo-pi` |
| Linux | `$XDG_DATA_HOME/timo-pi` |

```
<数据目录>/settings.json          全局设置
<数据目录>/models.json            自定义供应商/模型（可选）
<数据目录>/mcp.json               MCP server 配置（可选；设计 06 §1）
<数据目录>/sessions/*.jsonl       会话（设计 04）
<项目根>/.timo/settings.json      项目设置（可选；与全局深合并，项目胜出）
<项目根>/.timo/mcp.json           项目 MCP（仅 enabled/exposure 覆盖全局同名，pi 同构）
```

- 密钥**永不入** settings.json；引用形态见 §4（standards/06 安全红线）。

## 2. settings.json（字段对齐 pi settings-schema；snake_case）

| 字段 | 类型/默认 | 说明 |
|---|---|---|
| `default_provider` | string \| null | 默认供应商 id |
| `default_model` | string \| null | 默认模型 id（配合 default_provider） |
| `default_thinking_level` | enum，默认 `"medium"` | off/minimal/low/medium/high/xhigh/max |
| `default_tools` | array，默认 `["read","bash","edit","write"]` | 内置工具激活集；`+name`/`-name` 修饰（合并语义，pi 同构） |
| `session_dir` | string \| null | 会话目录覆盖 |
| `shell_path` | string \| null | bash/powershell 外壳路径（Windows 缺省探测 bash→PowerShell） |
| `shell_command_prefix` | string \| null | 命令前置行 |
| `retry` | `{enabled:true, max_retries:3, base_delay_ms:2000, max_retry_delay_ms:60000}` | assistant 轮次重试（设计 01 §8） |
| `images` | `{auto_resize:true}` | read 工具图像缩放开关 |
| `max_tokens` / `temperature` | u64 / f64 \| null | 请求覆盖（None→协议默认） |

- `$schema` 字段允许；未知字段**忽略不报错**（向前兼容，pi additionalProperties 同构）。
- 解析失败（非法 JSON/类型错）→ `ConfigError::Invalid{path, reason}`，CLI 明示文件路径——**配置错误不静默降级**。

## 3. models.json（自定义供应商/模型）

```json
{"providers": {"myprovider": {
    "base_url": "https://…", "api": "openai-completions",
    "api_key": "$MY_PROVIDER_KEY",
    "headers": {"X-Custom": "v"},
    "models": [{"id": "model-x", "name": "Model X", "reasoning": true,
                "context_window": 128000, "max_tokens": 16384,
                "cost": {"input": 0.5, "output": 1.5}}]}}}
```

- `api` ∈ `anthropic-messages | openai-completions`（Phase 1 两协议；接缝 #3：新增协议无需改此处结构）。
- 内置目录（timo-ai assets）与 models.json 合并：models.json 同名 provider 的 models **追加**，字段覆盖（模型级）——pi models.json 同构。
- Model 字段缺省：`context_window` 128000、`max_tokens` 16384、`reasoning` false、`input ["text"]`（pi 同值）。

## 4. 凭证解析（优先级从高到低；对齐 pi applyAuth）

1. 调用方显式（CLI `--api-key`）
2. provider `api_key` 三形态：字面量 / `$ENV_NAME` / `!command`（执行取 stdout，trim；命令失败→ConfigError）
3. 环境变量表：`ANTHROPIC_API_KEY`（anthropic）、`OPENAI_API_KEY`（openai）
4. 皆无 → 该 provider 不可用（模型解析时剔除；选中时报 `NoApiKey`）

## 5. 模型解析（pi model-resolver 优先级）

1. CLI `--provider P --model M`（或 `--model P/M[:level]`）：显式指定；未知组合→错误（不静默造模型）。
2. settings `default_provider` + `default_model`：须凭证可用，否则跳到 3。
3. 首个**有凭证**的内置 provider 的目录首个模型（anthropic 先于 openai，目录序）。
- thinking level 解析：CLI `:level` 后缀 > settings `default_thinking_level`；`off` 映射为不发 thinking 参数。
- 解析结果一次性定案并在会话 header/`model_change` 落盘（恢复时还原——pi 同构）。

## 6. 错误类型表

| 变体 | 触发 |
|---|---|
| NotFound{path} | 目录不可定/文件缺失（仅显式要求时） |
| Invalid{path, reason} | JSON/类型校验失败 |
| EnvVarMissing{name} | `$ENV` 引用未设 |
| CommandFailed{cmd, status} | `!cmd` 非零退出 |
| ModelNotFound{spec} | 解析优先级 1/2 未命中 |
| NoCredential{provider} | 选中模型但无凭证 |
