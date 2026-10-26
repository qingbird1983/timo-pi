# P-001 适配器流式事件静默缺失

## 现象

协议回放测试 `openai_cancel_midstream` 永远挂起；全量 `cargo test` 也挂起。单跑该测试没有报错，只是不结束。

## 根因

OpenAI 适配器的 `OpenAiStream::on_sse` 对所有 chunk 返回 `Ok(None)`——只更新 partial，不产出任何 `AssistantStreamEvent`。因此上层永远收不到 `Delta`；取消测试等待 Delta 才触发 cancel，于是永久等待。Anthropic 适配器同位置返回事件，所以只有 OpenAI 暴露。

## 修复

`on_sse` 改为返回 `Vec<AssistantStreamEvent>`：首次内容块发 `BlockStart`，每次增量发 `Delta`，`finish_reason` 时发 `BlockEnd`（工具参数在 BlockEnd 前解析）。`openai/mod.rs` 的消费点同步发送全部事件。见提交 `2f8570d`。

## 预防

- 每条协议的事件通路必须在回放测试里断言 `BlockStart/Delta/BlockEnd` 的存在与顺序，不能只断言终态消息。
- 取消类测试若以"收到某事件后 cancel"为触发条件，必须同时对该事件做存在性断言并加超时保护（否则缺失时表现为挂起）。
