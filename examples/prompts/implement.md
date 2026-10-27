---
description: scout 取证 → planner 定计划 → worker 落代码
argument-hint: <要做什么>
---
用 subagent 工具的 `chain` 参数把下面三步串起来跑：

1. 先让 scout 找出与「$@」相关的代码位置与约束。
2. 再让 planner 依据上一步的取证（任务里用 `{previous}` 占位）为「$@」排出实施步骤。
3. 最后让 worker 照上一步的计划落地（任务里用 `{previous}` 占位），并回报改了哪些文件。

三步必须按顺序成链，每步都要接住前一步的输出；不要改成 parallel。
