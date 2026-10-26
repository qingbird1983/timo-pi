# 设计 02：凭证、钥匙串与桌面设置

- 状态：定稿
- 日期：2026-10-26
- 关联：standards/06 §3；设计 05（数据目录/凭证引用）；设计 00/01

## 1. 目标与威胁模型

用户只做一次：在设置里粘贴 API key，点保存。之后的任何界面、配置、日志、诊断、崩溃报告都不应出现明文。威胁模型沿用 standards/06：本机同用户进程可读钥匙串，因此本设计防的是**意外泄露**（配置备份、截图、日志、错误弹窗、仓库提交），不是同用户恶意进程。

三条硬规则：

1. GUI **从不**把密钥写进 `settings.json`/`models.json`/桌面设置。
2. 引用解析失败的报错只含服务名/账号名，不含值。
3. 任何 Debug/日志输出必须脱敏；可见界面保存后只显示“已保存”，不回显。

## 2. 钥匙串条目约定

| 项 | 值 |
|---|---|
| 服务（service） | `timo-pi`（固定，与数据目录同名） |
| 账号（account） | provider id，例如 `deepseek`、`anthropic` |
| 值 | 该 provider 的 API key 原文 |
| 平台后端 | Windows Credential Manager / macOS Keychain / Linux Secret Service |

Windows 上条目显示为“timo-pi / deepseek”；不按模型 id 存，同一 provider 的多个模型共用一个 key。

## 3. 配置引用语法

在 `models.json` 的 `provider.api_key` 中新增第四形态：

```json
{"providers": {"deepseek": {
  "base_url": "https://api.deepseek.com",
  "api": "openai-completions",
  "api_key": "keychain:timo-pi/deepseek",
  "models": [{"id": "deepseek-flash"}]
}}}
```

- 语法：`keychain:<service>/<account>`；两段都必须非空，只允许一个 `/` 分隔（service/account 内不允许 `/`）。
- `ValueRef` 增加 `Keychain { service: String, account: String }`；解析顺序在 `$`/`!` 之后、字面量之前。
- 序列化回同一字符串；roundtrip 测试必须覆盖。
- 解析失败（格式错）按配置错误处理（`Invalid{path, reason}`），不静默当字面量。
- 读取失败（条目不存在/后端不可用/权限拒绝）→ 新增 `ConfigError::Keychain { service, account, reason }`；消息不含密钥值。
- 解析在 `spawn_blocking` 中调用 `keyring::Entry::new(service, account)?.get_password()`，不阻塞 tokio 工作线程。
- 既有优先级不变：显式 `--api-key` > provider 引用 > 内置环境变量；GUI 不使用显式参数。

`ValueRef` 的手写 `Debug` 必须脱敏：`Literal` 显示 `"<redacted>"`，`Command` 显示 `"<redacted>"`，`Env`/`Keychain` 只显示名字。现有 `resolve` 行为与测试保持不变。

## 4. 桌面设置文件

内核的 `settings.json` 不变；桌面壳另存 `<数据目录>/desktop.json`（schema 1，原子写：`.tmp` + `rename`）：

```json
{
  "schema": 1,
  "locale": "zh-CN",
  "theme": "system",
  "font_scale": 1.0,
  "window": {"width": 1280, "height": 800, "sidebar": 280, "review": 480},
  "review": {"files": true, "commands": true}
}
```

- 只存非敏感项；未知字段忽略；损坏时用默认值并提示一次“桌面设置已重置”，不阻塞启动。
- `font_scale` 映射到 gpui-kit 主题 base font；范围 0.9~1.3，默认 1.0。
- 主题 `system|light|dark`；语言 `zh-CN|en`。
- 审阅开关在**新工具请求**生效，不追溯已排队请求。

## 5. 设置页交互（凭证）

每个 provider 一行：

| 状态 | 展示 | 动作 |
|---|---|---|
| 钥匙串已存 | “已保存在系统钥匙串” + account | 替换、移除 |
| 环境变量 | “来自环境变量 `$NAME`” | 改用钥匙串（可选） |
| 命令 | “来自命令引用” | 不展示命令文本；改用钥匙串 |
| 明文 | 危险 Badge“配置中是明文” | “移动到钥匙串” |
| 未配置 | 中性“未配置” | 设置 |

- 设置：密码输入框（`Input` password，写入后清空），保存按钮在非空时可用；保存成功后只显示状态行，不回显任何前缀/尾号。
- 替换：同一输入流程；成功后覆盖同一条目。
- 移除：`AlertDialog` 标题“移除 deepseek 的 API key？”，正文“钥匙串条目将被删除。使用该 provider 的会话在重新配置前无法运行。”按钮“取消 / 移除”。
- 移动到钥匙串：先写钥匙串，成功后再原子改写 `models.json` 引用；任一步失败都不改动配置，错误内联显示。
- 后端不可用（常见于无 Secret Service 的 Linux）：显示“系统钥匙串不可用：{reason}”，提供“改用环境变量引用”的说明；**不**回退为明文。

## 6. 脱敏边界

- UI：保存后不显示值；错误弹窗只显示引用名；复制按钮不存在。
- 日志：不记录 `resolve()` 返回值；HTTP 头中的 Authorization 已由协议层遮蔽（Phase 1 既有）；新增测试扫描日志字符串。
- 诊断/事件：`ProviderRequest` 只含 provider/model，不含鉴权。
- 会话文件：不写凭证；模型 header 只写 provider/model。
- 崩溃/panic：消息只可能来自配置错误文本，不含值。
- 仓库：`keychain:` 引用可以入库（不含秘密）；密钥本体永不入库。

## 7. 测试与验收（C3）

1. `ValueRef`：解析/序列化 `keychain:timo-pi/deepseek`；非法空段报错；Debug 脱敏。
2. `resolve`：用测试专用 service/account 写入假密钥 → 读回一致；删除后报错文案含 service/account、不含值。测试结束清理条目；若平台无钥匙串，测试标记 skipped 并记录原因（CI 在 Windows 跑真实路径）。
3. 设置流程（无头/调用逻辑层）：保存后 `models.json` 只含引用；用唯一假密钥字符串全量扫描数据目录与捕获日志，断言不存在。
4. 移除流程：条目删除；解析报错；配置引用保留，提示重新配置。
5. 大模型真机：`DEEPSEEK_API_KEY` 路径与 `keychain:timo-pi/deepseek` 路径各跑一次 hello 会话；只记录成功/失败，不打印 key。

## 8. 平台差异

| 平台 | 后端 | 已知限制 |
|---|---|---|
| Windows 10/11 | Credential Manager | 本阶段主验路径；无额外守护进程 |
| macOS | Keychain | 首次写入可能弹系统授权；后台进程需用户解锁钥匙串 |
| Linux | Secret Service（GNOME Keyring/KWallet） | 无桌面会话/无 Secret Service 时不可用；明确报错，允许 env 引用 |

Linux CI 不把钥匙串测试设成硬门槛；以 Windows 真机证据为准，Linux 记录“可用/不可用”结果。
