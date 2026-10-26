# 工作上下文交接卡

- 更新：2026-10-26（会话主题：**Phase 2 收尾：全链路接入 + 真机验收**）

## 当前阶段与状态

- **Phase 1 ✅ 完成**（tag `phase-1.end`）。
- **Phase 2 ✅ 完成**（tag `phase-2.end`）：A 轨扩展宿主全链路打通，B1~B6 真机（DeepSeek）验收通过，四道门禁全绿（185 个测试）。
- 验收报告：`docs/design/phase-2/report.md`（证据、偏差 D27/D28/D29、限制、收尾清单）。
- 看板：`docs/roadmap/00-roadmap.md` §1；阶段文档：`docs/roadmap/phase-2-extension-host.md`。
- 远端：`github.com/qingbird1983/timo-pi`（私有）。

## 上一轮完成了什么

- **内核装载**：`ensure_ready` 完成"发现 → 打包 → 授权 → 装载"，扩展工具带完整元数据（暴露面/注释/执行模式）注册进工具表，内核事件转发给扩展；`resume` 也会先装载。
- **CLI 面**：`timo ext list/add/remove/trust/doctor`；`--extension`、`--no-extensions`、`--ext-flag`；REPL `/ext` 与扩展斜杠命令（`plugin_<id>_` 后缀匹配）。
- **诊断归属**：钩子超时/处理器抛错按扩展 id 记录，运行结束汇总；无订阅者的事件不再入队。
- **真机 B1~B6**：hello（钩子改写参数/系统提示）、guard（否决 `rm -rf`）、net（E402 → 授权放行）、exec（E401 → 授权成功）、fs（E403 / `node:fs` E104）、slow（E301 后会话继续）。
- **收尾**：设计 00/02/04 回填 D28/D29 与实测结论；CHANGELOG 0.1.19；看板与阶段文档置完成；本卡重写。

## 下一步（按优先级）

1. **Phase 3 开工前**：读 `docs/roadmap/phase-3-desktop.md`，按"文档先行"先出桌面阶段设计（gpui-kit 技能已 vendored）。
2. 债务按需排期（不阻塞 Phase 3 启动）：
   - D29：扩展事件通知队列无上界（极端刷屏时内存增长）。
   - D23：`registerVirtualModel` 只支持同 provider 路由。
   - `fs.*` 能力只有声明校验，无运行期 API。
3. `target/` 体积接近软阈值；需要时安装 `cargo-sweep` 清理（不影响门禁）。

## 未决问题

- `message_end` / `input` 等可改写事件是否升级为支持：等真实扩展需求驱动。
- 扩展 UI（`ui.*`）只做惰性降级 + 诊断，完整 TUI 桥留待后续阶段。

## 关键提醒

- **接缝 #2**：`timo-agent` 循环只调用注入的 `LoopHooks`，禁止出现扩展名分支；`None` 时与 Phase 1 逐字节等价。
- **不得复制 pi 源码**（铁律 7）：仓内夹具一律自写；GPL 项目只读思路。
- **权限强制点唯一**：QuickJS 无 OS 能力，exec/net/fs 只能走宿主函数。
- **产物是 CJS**：内嵌 QuickJS 不加载 ESM；顶层 await 记 E106（D27），改夹具/文档前先看设计 02。
- **门禁四连**：`cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && bash scripts/check-size`。
- **500 行 gate**：大模块按职责拆（`run/`、`extensions/` 已有先例），纯测试放 `tests/`。
- **写中文文件后扫隐形字符**：`grep -nP '[\x{3040}-\x{30ff}\x{200b}-\x{200f}\x{feff}]'`。
- **收尾四件事**：重写本卡、归档 handoff、沉淀 pitfalls/skills、体量核对。
- 踩坑索引：P-001 流事件静默回归；P-002 编辑重复行；P-003 Windows bash 是 WSL；P-004 上游摘要当合同；P-005 设计格式与运行时能力不符。
