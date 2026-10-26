# 设计 03：扩展权限能力清单 v1（双闸与强制点）

- 状态：定稿
- 日期：2026-10-26
- 关联：architecture/04 §3；standards/06；roadmap/phase-2；本目录 00/02/05
- 参照（只借思路）：research/04 §3 的权限双闸与 fs/net 细则

## 1. 原则

1. **双闸**：manifest 声明"需要什么" → 用户授予"允许什么"；两者都有才生效。
2. **默认拒绝**：未声明、未授予、非 TTY 无法询问 → 一律拒绝并在调用点抛错。
3. **沙箱兜底**：QuickJS 无 OS 能力；扩展只能经宿主函数触达文件/网络/进程，**强制点唯一**。
4. **最小授权**：能力按调用点逐次校验，不做"授予后全局放行"。
5. **可撤销、可失效**：授权绑定源码哈希；源码变化自动失效并重新询问。
6. **留痕**：每次拒绝产生诊断条目（码 + 能力 + 调用点），并对用户可见一次摘要。

## 2. 能力清单 v1

| 能力 | 风险 | 默认 | 声明格式 | 强制点 | 生效范围 |
|---|---|---|---|---|---|
| `exec` | 高 | 拒 | `"exec": true` | `pi.exec()`；stdio MCP server 注册 | 子进程，cwd 固定为工作目录 |
| `net` | 高 | 拒 | `"net": {"domains": ["api.example.com", "*.corp.example"]}` | 全局 `fetch`；http MCP server 注册 | 仅出网；域名单 fail-closed |
| `fs.read` | 中 | 拒 | `"fs": {"read": ["./docs/**"]}` | 预留宿主文件 API（本版 A 轨无直接暴露面） | 读 |
| `fs.write` | 高 | 拒 | `"fs": {"write": ["./out/**"]}` | 预留宿主文件 API | 写；**禁止全树** |
| `fs.delete` | 高 | 拒 | `"fs": {"delete": ["./tmp/**"]}` | 预留宿主文件 API | 删；**禁止全树** |

- **本版实际可触达**：`exec` 与 `net`（经 `pi.exec`/`fetch`/MCP 注册）。`fs.*` 先定义契约与校验，防止后续 API 落地时重开权限模型；B4 的"全树声明被拒"在本版即可验收。
- 不设 `env`/`shell`/`clipboard`/`audio` 等能力：没有宿主 API 就不发能力（最小面）。

### 2.1 域名规则（`net`）

- 允许项：ASCII 主机名，可选端口（`api.example.com`、`api.example.com:8443`）；唯一通配是**最左** `*.`（`*.example.com` 匹配一级及以上子域，不匹配 `example.com` 本身）。
- 禁止：scheme、路径、通配在中间、`*` 单独项、IDN；解析失败 → manifest 非法（E110）。
- 匹配：URL 的 `host`（小写、去尾点）+ 端口精确匹配；IP 字面量必须显式声明。
- 重定向：逐跳校验目标域名；任一跳不在白名单 → 拒绝（E402）；最多 5 跳。
- DNS/连接层不做额外判定；判定发生在发起前（host 已知）。

### 2.2 文件作用域规则（`fs.*`）

- 相对路径以**工作目录**为根解析；绝对路径必须显式声明且不得越过工作目录（越界 → E403）。
- 规范化顺序：展开 `~` → 转绝对 → 解析 `.`/`..` → 符号链接解析 → Windows 下大小写折叠 → 匹配。
- glob：`*` 单段、`**` 任意深度、`?` 单字符；目录前缀等价 `dir/**`。
- **写/删全树禁令**：规范化后等于文件系统根、盘符根、`**`、`/**`、`~` 或空的声明 → E403，直接拒绝该扩展的 manifest（不是运行期拒绝）。
- 硬拒（无论声明）：`.env*`、`**/.ssh/**`、SSH 私钥/凭证文件、`**/.git/**`（读/写/删全禁）。命中 → E404。

## 3. manifest 能力字段

```json
{
  "capabilities": {
    "exec": false,
    "net": { "domains": [] },
    "fs": { "read": [], "write": [], "delete": [] }
  }
}
```

- 缺省等价于全拒（字段可省略）。
- 校验顺序：schema 合法 → 名称在清单内 → 各能力值域合法（域名/作用域）→ 全树写删检查。
- 校验失败 → E110（manifest 非法）；全树写删 → E403；两者都阻断该扩展装载并给出字段定位。
- 静态扫描（02 §3.5）只生成**建议**；不写回 manifest、不构成授权。

## 4. 授予与撤销

### 4.1 授予

```bash
timo ext trust <id>            # 逐项询问 manifest 声明
timo ext trust <id> --revoke   # 全部撤销
timo ext list                  # 显示 declared / granted 差异
```

- 首次装载且存在未授予能力时：TTY 输出一张清单（能力、风险、范围），一次性 y/N；N 或非 TTY → 该扩展以**全拒**状态装载（E405），每次被拒调用再记 E401。
- 授予写入 `<agent_dir>/permissions.json`：

```json
{
  "schema": 1,
  "grants": {
    "auto-commit": {
      "sourceHash": "sha256:…",
      "exec": true,
      "net": [],
      "fs": { "read": [], "write": [], "delete": [] },
      "grantedAt": "2026-10-26T12:00:00Z"
    }
  }
}
```

- 授予只收紧不放大：granted 必须是 declared 的子集；手工编辑越权项 → 忽略并诊断。
- 文件权限：Unix 下 `0600`；Windows 使用用户目录默认 ACL。

### 4.2 失效

- `sourceHash`（manifest 记录）与当前源码不一致 → 该 id 的全部授权作废（E406），下次装载重新询问。
- `timo ext add` 覆盖安装 = 源码变化 = 授权失效（防止"先批小扩展、后换恶意包"）。
- 卸载扩展时删除其授权条目（`timo ext remove`）。

## 5. 强制点与实现

| 宿主函数 | 检查 | 拒绝行为 |
|---|---|---|
| `pi.exec(cmd, args, opts)` | `grants.exec == true` | JS 抛 `PermissionError("exec not granted")`；E401；不启动进程 |
| `fetch(url)` | `net` 白名单匹配 host(+port) | JS 抛 `PermissionError("net denied: <host>")`；E401/E402；不发请求 |
| stdio MCP 注册 | `grants.exec` | 注册失败 + E401 |
| http MCP 注册 | `net` 含该 host | 注册失败 + E401/E402 |
| 预留 fs API | 对应 `fs.*` 作用域 + 硬拒路径 | JS 抛 `PermissionError`；E401/E403/E404 |

- 拒绝异常是**普通 JS Error**（含 `code` 属性），扩展可捕获；不打断会话。
- 宿主函数参数先做类型/边界校验，再做权限校验（避免权限判断本身被畸形参数绕过）。
- 子进程：固定 `cwd=work_dir`，清空继承的环境变量（仅保留 PATH/SystemRoot 等启动必需项），超时与取消由 run 令牌控制。
- 网络：只允许 http/https；禁止本地回环与私有网段地址？**本版不额外禁止**（用户已显式授权域名；回环域名需显式声明）——记录为待决。

## 6. 留痕与可观测

- 每次拒绝：诊断条目 `{code, capability, callSite, host?, timestamp}`；同一扩展同一能力同一调用点只保留首条 + 计数。
- 用户可见：stderr 首次一行摘要（如 `[ext] auto-commit: net denied (api.x.com) — run 'timo ext trust auto-commit'`）。
- `timo ext doctor` 展示：`declared / granted / denied 次数`。
- 拒绝不写会话文件（不污染上下文）。

## 7. 验收映射（B4/B5）

| 验收点 | 断言 |
|---|---|
| 未声明网络 → 访问被拒并留痕 | `net_like.ts` 调 fetch：Promise reject，`code==="E401"`；诊断出现 `{capability:"net", callSite:"fetch"}`；不发网络请求（用本地端口探针证明 0 连接） |
| 域名不匹配 → 拒绝 | 声明 `api.example.com`，实际请求 `other.example.com` → E402 |
| 全树 fs 写/删 → 拒绝 | `manifest_wide_fs` 声明 `write:["**"]`（及 `delete:["C:\\"]` 变体）→ 装载被拒 E403；`timo ext list` 标记 failed |
| `exec` 未授予 | `exec_like.ts` 调 `pi.exec` → PermissionError；授予后同一夹具成功执行 |
| `node:fs` 装载失败透明（B5） | 打包期 E104，报告含模块名与位置 |

## 8. 待决问题

- **回环/私网地址**：是否默认禁止（SSRF 面）。当前按"显式声明即可"；Phase 3 图形授权时可加"仅公网"选项。
- **`exec` 命令白名单**：v1 只有布尔。是否需要 `exec.commands` 允许列表，待真实扩展样本驱动。
- **MCP 注册的权限归属**：注册经由内核发请求/起进程，但风险来自扩展；当前按扩展能力校验，需在实现时确保"内核侧不再有第二条放行路径"。
- **授权 UI**：v1 为 y/N；图形化属 Phase 4。
