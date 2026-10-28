# 设计 06：devkit 与分发

- 状态：草稿（2026-10-28）
- 关联：roadmap/phase-4；ADR-0004；design/phase-4/02、04、07
- 原则：**不自研签名与打包器**。devkit 只做脚手架、校验与调用官方 `nsh` CLI（Phase 3 PoC 已验证 `keygen/certify/publish/verify`）。

## 1. 交付物

| 命令 | 作用 | 退出码 |
|---|---|---|
| `timo-devkit new <id>` | 生成插件骨架（manifest、TSX、SDK 类型、脚本） | 0/1 |
| `timo-devkit check <dir>` | 校验 manifest、slot/action 白名单、TSX 编译、API 版本 | 0/1（列出全部问题） |
| `timo-devkit pack <dir> --out <file>` | 调 `nsh publish` 生成签名包 + 写出 manifest | 0/1 |
| `timo-devkit verify <pkg>` | 调 `nsh verify` 并打印包元数据（id/version/build/sha） | 0/2 |
| `timo-devkit keygen <path>` | 调 `nsh keygen`；只打印公钥指纹 | 0/1 |

- `pack` 需要 `--key`、`--cert`（或从 `--profile` 读取）；私钥路径不打印、不写日志。
- `pack` 的 `--build` 默认取 `git rev-list --count HEAD`；非 git 目录要求显式传入，且必须大于已有包。
- 失败时不给半成品：先写临时文件，`nsh publish` 成功后 rename 到 `--out`。
- `check` 不联网、不执行插件代码；只做静态校验与编译。

## 2. 插件骨架

```text
my-plugin/
  timo-plugin.json        # manifest（schema 2）
  src/App.tsx             # 默认导出根组件
  src/sdk.ts              # 生成时固定版本的类型副本
  package.json            # 仅开发期（rolldown/tsc）
  README.md
```

- `new` 默认生成 `panel` 示例与 `read-only` 挡位；`--slot toolCard` 可换。
- SDK 类型随 devkit 版本生成，`check` 时与宿主能力表比对；不一致直接失败（防止插件按旧 API 发布）。
- 不提交 `node_modules`、私钥、签名包到插件源码库（`.gitignore` 由脚手架生成）。

## 3. 打包与清单一致性

`pack` 顺序：

```text
check → 计算源码指纹 → 写 manifest.source_hash
     → nsh publish（TSX + 资源）→ 读取包元数据
     → 校验 manifest.nsh_id/version 与包一致
     → 写 manifest.bundle = 包文件名
     → 输出 <out>.nshpkg 与 manifest.json
```

- 包内代码由 NSH 签名；manifest 不签名（02 §1）。
- 资源：TSX 引用的图片/JSON 由 `nsh publish` 收集；devkit 不二次打包。
- `pack` 结束打印：id、version、build、sha256 前 12 位、公钥指纹；不打印私钥或完整证书。

## 4. 分发

- 本地文件：用户选择 `.nshpkg` + 同名 `manifest.json`（或目录）；管理器走 01 §6。
- URL：下载到临时文件，校验 SHA-256 与签名后进入同一安装流程；重定向最多 3 次，仅 HTTPS。
- 不做后台自动更新；「检查更新」只在用户点击时查询同一来源（本地目录/URL 清单）。
- A 轨兼容扩展仍按现有方式安装（源码目录 + rolldown）；管理器调用同一内核 API，不新增第二套安装器。

## 5. 版本与兼容

- `version` 语义化；`build` 单调递增（NSH 拒绝同 app id 的旧 build）。
- manifest `api` 必须等于 devkit 支持的宿主主版本；`check` 发现更高主版本 → E721。
- devkit 与宿主发版解耦：devkit 0.x 可打 api 1 的包；宿主升级不要求重打包（除非主版本变更）。

## 6. 测试与证据

- 单元测试：manifest 校验、指纹计算、build 递增、`check` 对非法 slot/action 的拒绝。
- 打包冒烟：临时密钥生成 → pack → verify → 未签名/篡改包被拒（不提交测试包）。
- CI：devkit 的 `check` 自身测试；不依赖真实签名密钥。

## 7. 验收断言

- `new` 生成的骨架 `check` 通过、`pack` 可产出可安装包。
- 非法 slot（如 `window`）在 `check` 阶段被拒，错误信息指向字段。
- 篡改包在 `verify`/安装阶段被拒，devkit 不回显私钥。
- 同一源码重复 pack 得到相同源码指纹、递增 build、不同包 sha256（签名/时间差异允许）。
