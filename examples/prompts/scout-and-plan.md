---
description: scout 取证 → planner 定计划，到计划为止不动代码
argument-hint: <要弄清什么>
---
用 subagent 工具的 `chain` 参数跑两步：

1. 先让 scout 找出与「$@」相关的代码位置与约束。
2. 再让 planner 依据上一步的取证（任务里用 `{previous}` 占位）为「$@」排出实施步骤。

只交回计划本身：不许改任何文件，也不要加第三步。
