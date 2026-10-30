# H-015 Phase 4 P2：devkit 与自证插件

- 日期：2026-10-29
- 范围：Phase 4 P2（设计 06 devkit / 07 自证插件）

## 本轮做了什么

1. **`crates/timo-devkit`**（新 bin crate，只调用 `nsh` CLI，不链接 NSH 类型）
   - `new`：生成插件骨架（`timo-plugin.json`、`nsh.json`、`tsconfig.json`、`App.tsx`、
     `.gitignore`、`README.md`）；`--slot` 决定 slots 骨架，`--tool` 用于 toolCard。
   - `check`：一次收全部问题——清单字段（E720）→ 挡位（E722）→ API（E721）、入口存在、
     `nsh.json` id/版本一致、每条 `queries`/`actions`/`theme.update` 都被
     `capabilities.host` 精确或通配授权，然后 `nsh build` 到临时目录；`--types` 另跑
     `nsh check`（要联网拉 TypeScript 7）。
   - `pack`：源码指纹 → 构建号（`--build` 或 git 提交数）→ 拒绝不可验旧包与非递增构建号 →
     临时目录 `nsh publish` → 校验包元数据与签名指纹 → 原子改名落盘 → 把
     `source_hash`/`bundle` 写回 `timo-plugin.json` 与 `manifest.json`。
   - `verify`：解析 `nsh verify` 摘要，拒载时退出码 2。
   - `keygen`：`nsh keygen` 后对公钥取 SHA-256，回报前 16 位指纹。
2. **入口约定改为插件根 `App.tsx`**：NSH 把入口所在目录当 app 目录，`src/App.tsx` 布局
   下 `nsh build .`、`nsh check src/...` 都失败（见 P-020）。设计 06 §2/07 同步补实现注记。
3. **`examples/plugins/session-search`**：只读挡位的会话搜索面板——最近 10 条 / 搜索 20 条、
   200ms 防抖、序号守卫丢弃过期响应、`sessions.changed` 刷新、Enter 打开首条、`×` 清空、
   相对时间本地格式化（宿主只给 `updatedAt`）。
4. **文档**：00-overview §4 记 P2 完成；roadmap 00 与 phase-4 状态行前移；CHANGELOG 0.1.116。

## 验证

- 四连全绿：fmt / clippy `-D warnings` / **524 tests** / `check-size`；`scripts/clean-build`
  把 `target/` 从 21.4GiB 降到 14.0GiB。
- 临时目录全链路冒烟（一次性密钥，未入库）：`check` 通过 → 非 git 目录不传 `--build` 被拒 →
  build 42 成功 → 重打 41 被拒 → `verify` 元数据与 sha256 正确 → 篡改包退出码 2 →
  build 43 成功；多次重打源码指纹不变、包 sha 变化。
- 真机安装与四种容器渲染尚未验（需 NSH 运行时人工验收）。

## 遗留与下一步

- **D1~D6 验收 + `docs/design/phase-4/report.md` + tag `phase-4.end`**。
- `check --types` 离线会挂起（拉 TypeScript 7），默认不开；文档已注明。
- 健康门无首帧信号、`stop()` 同步 join 不可超时、`composerControl` 未实例化（前轮遗留）。
- 本地提交尚未 push；推送后核远端 CI。

## 已提取的沉淀

- 新增 [P-020](../pitfalls/P-020-nsh-app-dir-is-entry-dir.md)：NSH 的 app 目录 = 入口文件所在目录。
