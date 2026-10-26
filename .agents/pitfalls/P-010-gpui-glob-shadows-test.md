# P-010：`use gpui_kit::*` 会把 `#[test]` 属性宏带进子模块

- 日期：2026-10-27
- 场景：`apps/timo-desktop`，`nsh-poc` feature 下的普通 `#[test]`
- 现象：`error: recursion limit reached while expanding #[test]`；按提示把 `recursion_limit` 从 256 提到 512，错误仍然逐级要求 1024——不是深度不够，是自递归。

## 根因

`gpui_kit::*` 重新导出了 gpui 的 `test` 属性宏。父模块写了 glob 导入后，子模块里的 `#[test]` 解析到该宏，而不是内建测试属性；宏自身又展开出 `#[test]`，于是无限递归。

## 处置

- 该模块改为显式导入所需项（`AnyElement`、`Context`、`Entity`、`Window`、`div`、`AppContext`、`Styled`、`IntoElement`、`ParentElement`、`FluentBuilder`），不再 glob。
- 移除为绕开问题加的 `recursion_limit`；根因不在递归深度。

## 识别口令

- 看到 `recursion limit reached while expanding #[test]` 且**提高上限只会等比例要更高** → 先查宏作用域，不要继续调上限。
- 同类风险：任何 `use 某框架::*` 之后写内建属性/宏名（`test`、`main`、`bench`）。

## 同批经验

- NSH `TrustStore::open(dir)` 会把“已接受构建”持久化到 `nsh-trust.json`；先 `verify_package`、健康后再 `accept_package`，降级拒绝即来自这里。
- 暂存 v2 时新建第二个 `Entity<AppView>`，旧 View 不 drop；健康检查通过再交换、drop 旧 View。AppView 的引擎在独立线程，未渲染也照常报错，1 秒计时器足以做回退判定。
- 主窗口 `WindowOptions.inactive_frame_interval = None`，否则失焦时 30fps 会让 NSH 动画减半。
