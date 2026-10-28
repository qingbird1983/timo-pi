# 设计 02：清单、签名包与信任

- 状态：草稿（2026-10-28）
- 关联：roadmap/phase-4；ADR-0004；architecture/04；design/phase-3/03；design/phase-4/00、01
- 依赖事实：NSH 0.3 `verify_package` / `accept_package` / `Bundle`（Phase 3 PoC 已实测）

## 1. 两层结构

| 层 | 内容 | 谁保证完整性 |
|---|---|---|
| 代码包 | `plugin.nshpkg`（NSH 签名热更包） | ed25519 签名 + 哈希（NSH） |
| 清单 | `manifest.json`（能力、slot、API、挡位申请） | 安装时绑定包 sha256；能力授予以 `Grant` 为准 |

清单**不单独签名**：代码完整性由 NSH 包保证；清单能影响的是「申请哪些能力/槽位」，实际授权必须经用户确认并写入 `Grant`，槽位必须命中宿主白名单。篡改清单不能提升已授予能力，也不能让代码绕过签名校验。

## 2. A 轨 manifest（schema 1，现状不变）

字段与校验保持 `timo-ext/manifest.rs`：`schema=1`、`id`、`entry`、`bundle`、`source_hash`、`bundler`、`created_at`、`capabilities{exec,net.domains,fs.{read,write,delete}}`。

- 新增字段一律可选且向后兼容；本阶段不改写已安装扩展文件。
- 加载器看到 `schema=1` 即按 A 轨处理，`kind` 缺省为 `orchestration`。

## 3. B 轨 manifest（schema 2）

```json
{
  "schema": 2,
  "id": "session-search",
  "kind": "gui",
  "name": "会话搜索",
  "version": "1.0.0",
  "nsh_id": "com.timo.session-search",
  "entry": "source/index.tsx",
  "bundle": "plugin.nshpkg",
  "source_hash": "sha256:<64hex>",
  "bundler": "timo-devkit/0.1",
  "created_at": "2026-10-28T12:00:00Z",
  "api": 1,
  "tier": "read-only",
  "slots": [
    { "kind": "panel", "title": "会话搜索", "icon": "search", "order": 30 }
  ],
  "queries": ["sessions.list", "sessions.search"],
  "actions": ["notify"],
  "tools": [],
  "capabilities": {
    "exec": false,
    "net": { "domains": [] },
    "fs": { "read": [], "write": [], "delete": [] }
  }
}
```

| 字段 | 必填 | 规则 |
|---|---|---|
| `schema` | 是 | 必须为 `2` |
| `id` | 是 | 复用 A 轨 id 规则（`[a-z0-9][a-z0-9_-]{0,31}`），全局唯一 |
| `kind` | 是 | `gui` 或 `orchestration`（本阶段 B 轨只接受 `gui`） |
| `version` | 是 | 语义化版本；与 NSH 包的 app-version 一致 |
| `nsh_id` | `gui` 必填 | NSH app id（如 `com.timo.session-search`），必须与包元数据一致 |
| `entry` / `bundle` | 是 | 安全相对路径；`bundle` 必须以 `.nshpkg` 结尾 |
| `source_hash` | 是 | `sha256:<64hex>`；A 轨为源码指纹，B 轨为源码树指纹（用于更新提示） |
| `api` | `gui` 必填 | 宿主 API 主版本，当前 `1` |
| `tier` | 是 | `read-only` / `workspace` / `trusted`，安装向导允许下调 |
| `slots` | `gui` 必填 | 非空；元素 `kind` 必须命中 03 的白名单 |
| `queries` | 否 | 只读查询白名单；未列出的查询运行时拒绝 |
| `actions` | 否 | 动作白名单；未列出的动作运行时拒绝 |
| `tools` | 否 | `actions.runTool` 可调用的工具名；实际 = 声明 ∩ 挡位 ∩ 注册表 |
| `capabilities` | 是 | 与 A 轨同结构；`tier` 是能力上限 |

校验分三层：字段值域（E720）→ 挡位上限（E722）→ API 版本（E721）。`gui` 缺 `slots`、`api`、`nsh_id` 一律拒绝。

## 4. 安装包校验顺序

```text
bytes
 → 格式检查（magic/版本）
 → verify_package(trust, bytes, assets_root)      # 签名、证书、撤销、哈希、降级
 → PackageReceipt { app.id, app.version, app.build, sha256 }
 → 清单比对：nsh_id == app.id、version == app.version
 → 计算挡位上限（声明 ∩ 用户选择）
 → 写入 staging → rename 到 plugins/<id>/
 → 写 Grant 与 plugins.json（记录包 sha256）
```

- 用 `verify_package` 不用 `load_package`：失败时磁盘上不产生信任记录。
- `assets_root = <data_dir>/plugins/assets`；资源路径由 NSH 校验（无 `..`）。
- 同 id 已存在时进入更新：要求 `build` 递增（NSH 语义）且 `version` 不降低；否则 E712。
- 失败回滚：删除 staging、撤销本次 Grant、保留旧目录与旧 View。

## 5. 信任根与撤销

- 生产信任根：`<data_dir>/plugins/trust/root.pub` + `revoked.json`（包 sha256 列表）。
- 开发信任根：`<data_dir>/plugins/dev/`，仅在 `debug` 构建或显式 `--dev-plugins` 时启用；界面标注「开发签名」。
- 发布密钥分发策略另立 ADR（阶段文档 §4 遗留）；本设计只定义信任根的存放与读取。
- 撤销两层：NSH 撤销列表（包级）+ timo `plugins.json.revoked`（安装级）；任一命中即拒载并记 E711。
- 私钥不进入仓库与日志；devkit 输出只打印公钥指纹。

## 6. 热更、回退与卸载

| 场景 | 行为 |
|---|---|
| 更新成功 | 新 `AppView` 健康检查 1s → 交换 → drop 旧 View → 替换目录与清单 |
| 更新失败 | 保留旧目录、旧 View、旧 Grant；新资源放 trash 后清理 |
| 降级包 | `verify_package`/版本比较拒绝（E712），当前版本继续运行 |
| 撤销授权 | 停止 View/宿主 → 删除 Grant → 插件保留但不可启用 |
| 卸载 | 停 → 撤销 → 删目录 → 删 state/revoked 条目；失败逐项重试 |

- 健康检查沿用 Phase 3 PoC：`on_app_error` + 1s 无错误 + 首帧回调近似。
- 崩溃/死循环：`AppView` 错误回调 + 宿主侧超时（05 详设）；主应用不受影响。

## 7. 目录与原子写入

```text
plugins/
  <id>/manifest.json  <id>/plugin.nshpkg
  assets/<nsh-app-id>/<sha256>/   # verify_package 的 assets_root，按包哈希分目录
  .staging/<id>-<rand>/     # 安装/更新暂存，成功后 rename
  .trash/<id>-<rand>/       # 旧版本，异步清理
  trust/  dev/
plugins.json                # { schema, plugins: {id: {enabled,tier,package_sha256,installed_at}}, revoked: [] }
```

- `plugins.json` 与 `permissions.json` 都用「临时文件 + rename」；Unix 0600。
- 清单里的相对路径只允许 `Normal` 组件；符号链接一律拒绝（复用 A 轨规则）。

## 8. 错误码

| 码 | 含义 |
|---|---|
| E710 | 包格式非法/无法解析 |
| E711 | 签名、证书或撤销校验失败 |
| E712 | 版本降级或 build 未递增 |
| E713 | 清单与包元数据不一致（id/version） |
| E720 | 清单字段非法 |
| E721 | API 版本不兼容 |
| E722 | 申请挡位/能力超出允许范围 |
| E730 | 安装/更新落盘失败（可重试） |

## 9. 验收断言

- 未签名、篡改一字节、撤销列表命中的包：均停在 E711，`plugins/` 无新目录。
- 清单 `nsh_id` 与包 id 不一致：E713，不落盘。
- 更新包 build 不递增：E712，旧版本继续可用。
- 安装成功后断网重启：已安装插件正常加载，不做后台更新。
- 旧 A 轨扩展文件一字不改仍可加载（schema 1 路径回归）。
