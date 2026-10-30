# P-023 NSH 的 2s JS 调用预算会先于宿主看门狗中断死循环回调

## 现象

为验证设计 05 §4 的看门狗（`__ping` 2s 一次、1s 等待、连续 2 次无 `__pong` → E742），
在插件入口写：

```js
host.on("__ping", () => {
  for (;;) {
    // spin: never posts __pong
  }
});
```

真机上容器确实被移除、诊断确实是 E742，但消息是
`Error: interrupted`（日志为 `the script was interrupted: a call into JS exceeded its budget of 2000 ms`），
而不是看门狗自己的 `unresponsive: no __pong from the plugin`。

## 根因

NSH 0.3.0 对一次「宿主调用进 JS」设有 **2s 预算**，超时由引擎强制中断脚本，
`on_app_error` 被调用后走与看门狗相同的失败路径（stop → Error → E742 → 记一次崩溃）。
所以死循环回调在 2s 就被引擎掐断，宿主看门狗的 3 次 ping（约 4~5s）根本没有机会判定无响应。
两条路径最终都落 E742，只看错误码无法区分。

## 验证与修复

无需改代码——设计意图（隔离死循环）已由引擎预算满足。看门狗本身用**静默插件**验证：
入口不导入 `host`、不注册 `__ping`，则宿主 ping 无人应答，连续两次 miss 后
诊断 `E742 unresponsive: no __pong from the plugin`（真机 11:16:24 面板出现、11:16:31 前移除）。

设计 05 §4/§8 已加实现注记；验收与回归测试按「不回 `__pong`」构造看门狗用例。

## 预防

1. 测看门狗不要用死循环回调——那验的是引擎预算；用「不注册 `__ping`」的插件。
2. 断言要贴错误消息（`no __pong`），不能只看 E742；否则两条路径会互相冒充。
3. 依赖安装的引擎超时要写进运行时设计文档，避免把引擎行为误记成自己的实现。
