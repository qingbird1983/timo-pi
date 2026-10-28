# 设计 01：统一插件管理器

- 状态：草稿（2026-10-28）
- 关联：roadmap/phase-4；ADR-0004/0015；architecture/04；design/phase-4/00、02
- 原型：`docs/design/phase-4/prototype.html`

## 1. 目标与边界

左栏「插件」页管理已安装的 A 轨兼容扩展与 B 轨界面插件：列出、启用/禁用、更新、卸载、权限挡位、诊断；发现与安装入口在「插件市场」（08）。管理器只做「清单 + 生命周期 + 授权」，不负责插件运行时代码；A 轨仍由 `ExtensionHost` 执行，B 轨由 NSH `AppView` 执行。

不做：插件市场、评分/评论、跨插件通信、按插件定制工作区布局。

## 2. 统一模型

```rust
pub enum PluginKind { Orchestration, Gui }        // A 轨 / B 轨
pub enum PluginOrigin { BuiltInConfig, Experimental, Installed }
pub enum PermissionTier { ReadOnly, Workspace, Trusted }
pub enum PluginState { Enabled, Disabled, Error, UpdateAvailable }

pub struct PluginEntry {
    pub id: String,              // 命名空间 id，如 session-search / subagent
    pub kind: PluginKind,
    pub origin: PluginOrigin,    // 内置配置 / 实验性开关 / 已安装插件
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub tier: PermissionTier,    // 实际授予档位（旧 A 轨按声明推断）
    pub declared: Declared,      // 清单声明的能力/slot/api
    pub state: PluginState,
    pub source: PluginSource,    // BuiltIn / LocalFile / Url / Market
    pub source_hash: String,     // A 轨源码指纹 / B 轨包 sha256
    pub configurable: bool,      // 有配置页（内置插件）
    pub diagnostics: Vec<PluginDiagnostic>,
}
```

- `PluginEntry` 只含普通 Rust 数据，桌面不接触 `nsh_*` 类型。
- `declared` 是「申请」；`tier` 是「实际授予」。有效能力 = 声明 ∩ 挡位上限。
- 排序：名称升序；同 id 冲突时 A 轨优先并产生 E7xx 诊断（两轨目录独立，正常不会冲突）。

## 3. 目录与状态

```text
<data_dir>/
  extensions/<id>/          # A 轨（现状，不改布局）
    manifest.json  bundle.cjs  source/
  plugins/<id>/             # B 轨
    manifest.json           # timo 清单（schema 2）
    plugin.nshpkg           # NSH 签名包
    assets/<app-id>/<sha256>/   # verify_package 落盘资源
  permissions.json          # 授权（现状，新增可选 tier 字段）
  plugins.json              # 统一启停状态（新）
```

- 清单以**目录扫描**为准，不另存副本；`plugins.json` 只存 `id → { enabled, tier, installed_at }`。
- 旧 A 轨扩展没有状态记录 → 默认 `enabled = true`、挡位按声明推断（有 exec/net → Trusted；有写删 → Workspace；否则 ReadOnly）。
- 删除插件 = 删目录 + 删状态 + 撤销授权；三步任一失败都要可重试，最终以目录不存在为准。

## 4. 内核 API（`timo-kernel::plugins`）

```rust
pub fn list_plugins(data_dir: &Path, work_dir: &Path) -> Vec<PluginEntry>;
pub fn install_plugin(data_dir: &Path, path: &Path, tier: PermissionTier) -> Result<InstalledPlugin>;
pub fn set_enabled(data_dir: &Path, id: &str, enabled: bool) -> Result<()>;
pub fn set_tier(data_dir: &Path, id: &str, tier: PermissionTier) -> Result<()>;
pub fn revoke_plugin(data_dir: &Path, id: &str, work_dir: &Path) -> Result<()>;
pub fn uninstall_plugin(data_dir: &Path, id: &str, work_dir: &Path) -> Result<()>;
```

- `install_plugin` 负责：识别包类型（A 轨源码目录 / B 轨 `.nshpkg`）→ 校验 → 计算挡位上限 → 落盘 → 写授权 → 返回摘要。
- 市场卡片安装先按 08 下载到临时文件，再调用同一个 `install_plugin`；不新增第二个安装器。
- `set_tier` 只允许收紧；放宽必须重新走安装向导的能力确认（防止静默提权）。
- 运行期加载由现有装配链负责：A 轨 `load_extensions`；B 轨在桌面 `plugins` 页初始化时按启用列表创建 AppView。
- 所有操作返回 `PluginError`（E7xx），桌面只做展示与重试。

## 5. 页面信息架构（「插件」页）

- 入口：侧栏「插件」（固定存在；puzzle 图标）。实验页 `nsh-poc` 在 P1 并入后删除。
- 顶部：标题「插件」+ 副标题「安装、启用和配置插件」；右侧刷新按钮与「添加插件」下拉（从文件安装 / 从链接安装 / 打开插件目录）。
- 页签：**已安装 / 插件市场 / 发现**；后两个跳到市场页（08 §3），已安装为默认。
- 「已安装」分三组，行式列表（图标 + 名称/徽标 + 描述 + 右侧控件）：
  1. **内置插件配置**：内核功能入口，无开关，点击进入对应设置（子智能体、网页搜索、Agent 循环、终端限制等）；清单由内核静态提供，`origin=BuiltInConfig`。
  2. **实验性插件**：产品功能开关（`origin=Experimental`），右侧开关写 `settings.json`；带「实验性」徽标；不加载外部代码。
  3. **已安装插件**：A/B 两轨第三方插件（`origin=Installed`）；右侧开关为启停；点击进入详情。
- 行内徽标：轨道（A 轨 / B 轨）、挡位（只读/读写/执行）、实验性、更新可用、有诊断。
- 详情抽屉/页：挡位明细、界面槽位（B 轨）、API 版本、来源与哈希、最近诊断、按钮组（检查更新 / 调整挡位 / 撤销授权 / 卸载 / 打开目录）。
- 空态：说明两轨区别 + 「添加插件」+ 「去插件市场」。
- 诊断：错误码 + 一句原因 + 时间；可复制；不默认展示堆栈与绝对路径。
- 卸载确认：说明会删除目录与授权；B 轨正在显示时先停 AppView。
- 内置配置与实验性开关**不写插件目录、不产生授权记录**；它们的清单由内核提供，卸载/禁用语义不适用。

## 6. 安装向导状态机

```text
选择包 → 识别与校验 → 选择挡位 → 确认声明 → 落盘与授权 → 启用
   │         │              │           │
   │         ├─ 签名/指纹失败 → 终止（E71x）
   │         ├─ 清单非法 → 终止（E720）
   │         └─ 已存在同 id → 进入更新分支（版本比较）
   └─ 取消任意一步 → 不留文件
```

- 挡位选项只展示**包申请档位及以下**的档位；插件申请只读时不能选执行。
- 确认页展示：包 id/版本/类型/签名者、声明能力明细、将写入的目录。
- 安装完成默认启用；失败回滚已写文件与授权（临时目录 + rename）。
- URL 安装：先下载到临时文件，后续与本地包同一路径；不做后台自动更新。

## 7. 启停、更新、卸载

| 操作 | 语义 | 失败处理 |
|---|---|---|
| 禁用 | A 轨不再加载、B 轨停掉 AppView；保留文件与授权 | 开关回到原状态并提示 |
| 实验性开关 | 写 `settings.json`，下一轮请求/界面刷新生效 | 回滚开关并提示 |
| 内置配置 | 跳转对应设置页；保存后重建相关组件 | 保留原设置 |
| 启用 | 重新走该轨加载；失败标记 `Error` 并展示诊断 | 保持禁用 |
| 更新 | 同 id 新包校验通过后替换；B 轨要求 build 递增 | 旧包与旧 AppView 保留 |
| 调整挡位 | 只允许收紧；收紧即时生效（重新加载） | 保留原挡位 |
| 撤销授权 | 删除 `Grant`，立即停用插件 | 提示重启或重试 |
| 卸载 | 停 → 撤销 → 删目录 → 删状态 | 逐项可重试；以目录消失为准 |

## 8. 挡位映射（有效能力 = 声明 ∩ 挡位上限）

| 能力 | ReadOnly | Workspace | Trusted |
|---|---|---|---|
| 只读会话/模型快照 | 允许 | 允许 | 允许 |
| `fs.read` | 声明作用域 | 声明作用域 | 声明作用域 |
| `fs.write` / `delete` | 拒绝 | 声明作用域（禁全树） | 声明作用域（禁全树） |
| `exec` | 拒绝 | 拒绝 | 允许 |
| `net.domains` | 拒绝 | 拒绝 | 声明白名单 |
| `actions.runTool` | 只读工具 | 只读 + 写类工具 | 全部工具 |
| slot 渲染 | 允许（白名单内） | 允许 | 允许 |

- 硬拒路径（`.env*`、`.ssh`、`.git`）对任何挡位都生效。
- `actions.runTool` 写类工具默认不弹逐次审阅；全局审阅开关开启时才弹（附带功能）。
- 挡位变更后重载插件，避免旧实例持有更高权限。

## 9. 验收断言

- 同一列表同时出现 A/B 两轨；筛选与搜索不丢条目。
- 安装未签名/篡改包：向导停在第二步，文件不落盘，无授权记录。
- 撤销授权后 1 秒内插件停止响应，列表显示未授权。
- 卸载后目录、状态、授权三处均无残留；宿主线程与 AppView 已释放。
- 关闭并重启应用后，启停状态、挡位与实验性开关恢复。
- 三组同页显示：内置配置无开关、实验性有开关、第三方插件有开关与详情；「添加插件」下拉三项都可达。
- 旧 A 轨扩展（无状态文件、无 tier）显示为启用、按声明推断挡位，行为不变。
