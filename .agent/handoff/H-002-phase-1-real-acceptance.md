# H-002 Phase 1 真实验收（DeepSeek）

- 日期：2026-10-26
- 主题：用本机 DeepSeek 凭证真实执行 A1~A4；修复暴露的两个缺陷。

## 本轮做了什么

1. 查 DeepSeek 官方文档：OpenAI 兼容 `https://api.deepseek.com`，模型 `deepseek-flash` / `deepseek-v4-pro`，支持流式与工具调用；凭证 `$DEEPSEEK_API_KEY`。
2. 临时目录 `.llm-test/`（未入库）：`data/models.json` 自定义 provider、`data/mcp.json`、`project/notes.txt`。
3. A1 通过：read→edit→bash(cat)→汇报；首次运行暴露 `edit` 行重复，修复后重跑文件字节正确。
4. A2 通过：`sessions` 列出 3 个会话；`resume last --once` 续聊，entry 9→13、parent 链完整。
5. A3 通过：运行中 `taskkill /F /IM timo.exe` 强杀，会话 3 行全为合法 JSON、链完整。
6. A4 通过：真实模型发现并调用 `mcp__echo__echo`，返回 `echo: hello from mcp`。
7. 修复 Windows bash 落 WSL（探测原生 MSYS → 否则 PowerShell）。
8. 门禁 122 测试全绿；设计 03 回填、报告/CHANGELOG/CONTEXT 更新。

## 遗留与下一步

- A5：推送后看 GitHub Actions。
- 阶段收尾：报告转正、看板、tag `phase-1.end`。
- Ctrl+C 两段语义未做信号自动化；真实 HTTP MCP server 未验。

## 已提取的沉淀

- P-002：edit 精确替换重复后续行。
- P-003：Windows bash 落到 WSL 启动器。
- 复用：S-001（挂起测试定位）、P-001（流式事件静默缺失）。
