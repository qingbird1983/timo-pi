# H-016 Phase 4 真机验收（D1~D6）：D4 无障碍崩溃阻塞

- 日期：2026-10-30
- 主题：所有者不在场，由 AI 会话代理执行 Phase 4 真机验收；产出阶段报告与两条 pitfalls。

## 本轮做了什么

- **环境**：调试构建；Windows 10.0.26300；临时数据目录 `%TEMP%\timo-accept-data`；临时签名密钥（私钥只传路径）；所有截图/日志只在 `%TEMP%\timo-accept*`，未入库、未触碰真实数据。
- **D1**：devkit `new/check/keygen/certify/pack/verify` 全链；安装 `session-search 1.0.5` build 7（read-only）；侧栏面板渲染、空查询 3 条、`parser` 1 条、`zzz` 空态、点击/回车打开；连续安装 v1.0.3→v1.0.4→v1.0.5，v1.0.4 修掉「永远载入中」（P-021）；build 2 降级被拒（E712），账本保持 build 7。
- **D2**：内置侧栏与插件面板并排同源同结果；插件只用 `sessions.list/search/openSession`。
- **D3**：篡改包拒载（`the signature does not verify`）；未签名拒载；越挡按钮置灰、E722；未声明调用 E740/E741 + 诊断卡；撤销 E711；沙箱逃逸仅能力面 + 测试覆盖。
- **D5**：接缝核对（事件总线/工具元模型/嵌套执行/统一入口/API v1/NSH 隔离）；fmt、clippy `-D warnings`、workspace **526 tests**、check-size 全绿。
- **D6**：市场页签、空态与刷新按钮；离线保留缓存、收藏/屏蔽持久化测试；卡片安装与本地安装共用 `verify_dir`（代码路径 + UI 测试），无真实索引源。
- **D4 未通过**：面板在场 + 无障碍激活（原生选目录、UIA 连接/断开）→ 重绘 panic `TreeUpdate includes duplicate child`（`accesskit_consumer-0.38.0/src/tree.rs:225`），5 次复现；定位到 NSH 0.3.0 焦点代理 + gpui-fast/accesskit 组合缺陷（P-022），未改第三方代码。

## 遗留与下一步

1. 所有者定夺 D4 处置：上报上游 / `[patch]`（须 ADR）/ 临时规避；修后复验 D4。
2. 补 D1 的「无重启即时替换」干净录像与「卸载后无残留进程」读数。
3. D4 修复后复跑 D1~D4，再打 `phase-4.end`；当前不 tag。
4. 提交/推送 P0~P2 与验收文档；推后核远端 CI。
5. 生产签名密钥分发策略、真实市场索引源仍待定。

## 已提取的沉淀

- P-021 NSH 组件体只执行一次，顶层 if/return 不随信号更新（自证插件 v1.0.3 卡在「载入中」）。
- P-022 NSH 焦点代理在无障碍激活时产生重复 child，宿主重绘即 panic（D4 阻塞根因）。
- 阶段报告：`docs/design/phase-4/report.md`。
