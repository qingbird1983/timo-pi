---
description: worker 实现 → reviewer 审 → worker 按意见改
argument-hint: <要做什么>
---
用 subagent 工具的 `chain` 参数跑三步：

1. 先让 worker 完成「$@」。
2. 再让 reviewer 审阅上一步的改动（任务里用 `{previous}` 占位），列出阻断项与建议项。
3. 最后让 worker 按 reviewer 的意见修改（任务里用 `{previous}` 占位），并回报最终改了哪些文件。

三步成链，逐步传递输出；reviewer 的阻断项没有被处理完就不算做完。
