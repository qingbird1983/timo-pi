# 设计 03：NSH 嵌入与签名包 PoC

- 状态：定稿
- 日期：2026-10-26
- 关联：ADR-0002/0004；research/03 §2；设计 00/01

## 1. 目标与边界

只回答一个问题：**NSH 的 AppView 能否作为普通 GPUI view 稳定嵌入 gpui-kit 窗口，并按签名包完成一次热更？** 不做插件系统、不做权限 UI、不做插件市场。

成功标准（C4）：

1. 静态 TSX 面板渲染在 gpui-kit 窗口内，跟随窗口尺寸，主题明暗跟随主应用；
2. 替换为 v2 签名包后，面板内容/版本号变化，主应用不重启；
3. 未签名包、被篡改包、被撤销/降级包都被拒绝，当前面板保持可用；
4. 以上过程不引入第二套事件循环或第二个窗口。

## 2. 依赖与版本

| crate | 版本 | feature | 备注 |
|---|---|---|---|
| nsh-platform-gpui | 0.3.0 | 默认关闭，启用 `js` | 不启用 `webview`，减少 Windows 构建面 |
| nsh-engine | 0.3.0（传递） | `js` | 提供 `Bundle`、`verify_package`、`accept_package` |
| nsh-package | 0.3.0（传递） | — | `TrustStore`、`SecretKey`、`PublicKey`、`Certificate` |
| nsh-cli | 0.3.0（仅开发期） | — | `keygen` / `certify` / `publish` / `verify`，不进入运行时依赖 |
| gpui-fast / gpui-fast-platform | 0.1.10（版本钉） | — | NSH 0.3 声明 `^0.1.9`；锁 0.1.10，Cargo 解析器选择 gpui-kit 0.8.0 家族 |

- 桌面壳的 `nsh-poc` feature 才引入 NSH；默认构建不编译 NSH。
- 版本升级独立成提交，并按 standards/08 §5 窗口化。
- **编译探针已通过（2026-10-26）**：`gpui-kit 0.8.0` 家族 + `gpui-fast 0.1.10` + `nsh-platform-gpui 0.3.0` 的 `cargo check` 成功，临时 crate 同时实例化 `AppView::new` 与 `Button`。
- 失败路径：gpui-kit 0.8.1 / gpui-fast 0.1.12 下，NSH 0.2.0 与 0.3.0 都在 `chrome.rs` 报 E0716（`KeyBinding::new` 要求 `'static`）；0.1.10 的签名接受 `&str`，因此锁 0.1.10。
- 实现首日只复跑探针并记录版本；升级任一依赖时重跑，不允许用“看起来能跑”的截图冒充通过。

## 3. 隔离层接口

只有 `apps/timo-desktop/src/features/nsh_poc/` 可以 `use nsh_*`。其余模块只看三件事：

```rust
pub(crate) struct NshPreview {
    // 内部：Entity<AppView>、TrustStore、当前包元数据、错误处理器
}

impl NshPreview {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self;
    pub(crate) fn render(&self, cx: &mut Context<Self>) -> impl IntoElement;
    pub(crate) fn load_signed(&mut self, bytes: &[u8], window: &mut Window, cx: &mut Context<Self>) -> Result<PackageInfo, String>;
    pub(crate) fn load_unsigned_for_test(&mut self, bytes: &[u8]) -> Result<(), String>;
    pub(crate) fn current(&self) -> Option<&PackageInfo>;
    pub(crate) fn stop(&mut self);
}
```

- 对外只返回普通 Rust 数据（版本、id、sha256、错误字符串），不暴露 NSH 类型。
- `AppView` 放在页面内的固定区域（flex_1），不覆盖主界面；主应用的 Dialog/Sheet 仍画在它上方。
- 主窗口 `WindowOptions` 设置 `inactive_frame_interval: None`，否则失焦时 30fps 会让 NSH 动画减半（NSH 文档明确要求）。

## 4. 加载与信任流程

### 4.1 信任根

- PoC 使用**开发签名密钥**：首次运行由 `nsh-poc` 页面触发生成，私钥写入用户数据目录 `nsh-poc/dev-key.json`（权限 0600，等价），不写入仓库、不打印。
- 公钥注册到 `TrustStore`（目录 `<数据目录>/nsh-poc/trust`），后续只信该根。
- 生产分发密钥的策略不属于 Phase 3；Phase 4 另立 ADR。

### 4.2 校验顺序

```text
bytes
  → nsh_engine::verify_package(&trust, bytes, assets_root)   // 签名/证书/撤销/哈希/版本
  → Bundle::from_package
  → AppView 暂存（新 Entity）
  → 观察 1s + on_app_error
      ├─ 无错误 → accept_package(&mut trust, &loaded) → 交换显示
      └─ 有错误 → drop 新 View，保留旧 View，显示错误
```

- 用 `verify_package` 而非 `load_package`：先验证并运行，健康后才 `accept_package`，这样 v2 失败时 v1 仍可验证/回退。
- `accept_package` 之后，同 app id 的更旧 build 会被拒绝（NSH 语义）；这正是“降级包被拒”的来源。
- 未签名/坏签名/哈希不符：`verify_package` 返回 `Err`，立即显示“签名校验失败”，**不进入加载**。
- 证书/撤销列表：PoC 只使用本机根 + 空撤销表；不联网拉取。
- 资源目录：`<数据目录>/nsh-poc/assets/<app-id>/<sha256>/`；包只写相对、无 `..` 的路径（NSH 已校验）。

### 4.3 热更与回退

- v1 在显示；加载 v2 时新建第二个 `Entity<AppView>`（独立引擎线程），旧 View 保持运行。
- 1s 内无 `on_app_error` 且引擎已出帧（以计时器 + 首帧回调近似）→ 交换；drop 旧 View。
- 失败/超时 → drop 新 View，旧 View 不变；错误进内联 Alert，不弹模态。
- 交换只改一个字段并 `notify` 一次；不做“两个面板同时可见”的过渡。

## 5. TSX 面板与打包

- 源文件：`fixtures/nsh-poc/panel.tsx`（自写，v1/v2 共用一个源码 + 版本常量）。
- 打包使用 `nsh-cli 0.3.0` 官方命令（`keygen` → `certify` → `publish` → `verify`），版本写入脚本注释与验收报告；**不把 NSH 源码或生成包入库**。
- 生成两个包到本机数据目录：`poc-v1.nshpkg`、`poc-v2.nshpkg`；另复制一份改一字节的 `poc-tampered.nshpkg` 用于拒绝测试。
- `scripts/build-nsh-poc` 只做“调用官方 CLI + 传参 + 输出路径”，不实现第二套 TSX 工具链。
- 若官方 CLI 与 0.3.0 不匹配：记录阻塞与所需版本，不在本阶段改走自研打包器（避免把 PoC 变成第二条产品线）。

面板内容（方案级）：

- 标题“Timo · NSH 预览”；
- 当前包版本与 sha256 前 8 位；
- 一个计数按钮（证明交互在 NSH 内工作）；
- 一段说明文字；v2 把版本号与标题颜色/文案改一处，便于肉眼确认热更。

## 6. 能力与安全

- `Bundle` 不声明任何能力（`with_capabilities_json(None)` / 默认 Policy）；面板只读展示，不发网络、不访问文件。
- 不注册任何 native component；宿主只提供 `AppView` 本身。
- 错误回调只把 `AppError` 的 message 转成界面文本，不把内部路径/堆栈默认展示（可展开详情，脱敏）。
- TSX 代码不进入内核；不通过扩展宿主加载；两套 JS 运行时（timo-ext 的 rquickjs 与 NSH）互不共享状态。
- 私钥、测试密钥、生成包都不入库；CI 只做“无签名拒绝”的单元测试（临时生成密钥），渲染验收为人工。

## 7. 页面位置

- 设置 → 关于 → “插件预览（实验）”，或侧栏“插件预览”入口（仅 `nsh-poc` feature 编译时出现）。
- 页面顶部：当前包 id/version/sha；按钮“加载 v1”“加载 v2”“加载未签名包（测试）”“重置信任根”。
- 加载按钮读取本机数据目录中的包路径；不提供文件选择器，避免 PoC 变分发面。
- 页面底部：最近一次错误/成功的内联状态。

## 8. 测试与证据（C4）

| 检查 | 方式 | 通过标准 |
|---|---|---|
| 嵌入渲染 | 真机截图 | 面板在窗口内，随窗口缩放，不遮挡主 UI |
| 交互 | 点击计数按钮 | 数字变化，主应用不重绘异常 |
| 主题 | 切明/暗 | 面板背景/文字跟随 `set_appearance` + `set_theme` |
| 热更 | v1→v2 | 版本号/文案变化，无重启，旧引擎被 drop |
| 未签名 | 加载原始 bundle.js | 显示“签名校验失败”，当前面板不变 |
| 篡改 | 改一字节 | 校验失败，当前面板不变 |
| 降级 | 先 v2 后 v1 | 拒绝旧 build，当前面板不变 |
| 崩溃隔离 | v2 故意抛错 | 1s 内识别，回退 v1 或显示错误；主应用可用 |

- 证据：截图 + 每次操作的错误/成功文本；不记录密钥。
- CI：`nsh-poc` feature 默认不编译；`cargo test --workspace` 不依赖 NSH。至少有一个单元测试验证 `verify_package` 对未签名字节返回错误（用临时密钥/空字节，不生成包）。

## 9. 失败与降级

- 编译探针失败（升级后）：C4 阻塞，Phase 3 其余功能照常收尾；报告写明版本冲突与尝试记录。
- CLI/打包不可用：同上；禁止提交伪造的“热更成功”截图。
- 引擎崩溃：`on_app_error` + 超时回退；若主线程被拖死，提供“停止预览”按钮 drop AppView。
- Linux 无 GPU/字体差异：记录未验证面，Windows 为主证据。
