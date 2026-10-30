# P-021 NSH 组件体只执行一次，顶层的 if/return 不会随信号更新

## 现象

自证插件 v1.0.3 的搜索面板永远停在「载入中…」：宿主日志里 `sessions.list` 已正常返回、
插件自己的 `ready.set(true)` 也执行了，界面却不重绘。v1.0.4 把条件判断挪进 JSX 后，
同一份数据立即显示出列表或空态。

## 根因

NSH 采用 Solid 式细粒度渲染：组件函数体（以及其中的 `if`/`return`）**只在首帧执行一次**，
之后只重跑读取了信号的 JSX 表达式。写成

```tsx
function Body() {
  if (!ready()) return <Text>载入中…</Text>;
  if (failure() !== "") return <ErrorView />;
  return <List each={rows()} />;
}
```

时，`ready()`/`failure()` 只在首帧读了一次；信号变化不会让函数体重新执行，
顶层分支因此永远停在首帧的结果上。列表内部能更新，是因为 `each={rows()}` 写在 JSX 里。

## 修复

把条件全部放进 JSX 三元表达式，保证每个信号都在 JSX 求值路径上读取：

```tsx
return (
  <View>
    {failure() !== "" ? <ErrorView /> : ready() ? <List each={rows()} /> : <Text>载入中…</Text>}
  </View>
);
```

组件函数体只做常量与 JSX 组装，不放依赖信号的分支或提前返回。

## 预防

1. 写 NSH 组件前先确认渲染模型（Solid 式），把「函数体只跑一次」当成硬约束。
2. 任何随状态变化的分支、空态、错误态都写在 JSX 里；用截图/日志核对首帧之后的更新，
   不能只看编译通过。
3. 自证插件与骨架模板保持一致；新增示例时先跑一次「先空后满」的数据切换。
