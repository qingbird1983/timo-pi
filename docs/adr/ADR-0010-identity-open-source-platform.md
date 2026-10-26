# ADR-0010：产品身份与发布策略——命名 Timo-pi、开源（MIT）、Windows 优先

- 状态：定稿
- 日期：2026-10-26
- 关联：ADR-0002；roadmap/phase-3；README

## 背景

Phase 0 收尾时所有者对三个遗留决策项拍板：正式产品名、是否开源及许可证、目标平台优先级。

## 决策

1. **产品正式命名 Timo-pi**（仓库同名，放弃此前的工作代号 "Timo" 单用）。
2. **项目开源，许可证 MIT**（LICENSE 已建于仓库根，版权行 "Timo-pi contributors"）。
3. **首发平台 Windows 优先**：Phase 3 桌面壳主攻 Windows；macOS/Linux 保证可构建、不保证首发打磨度。

## 理由

- MIT 最宽松简单，与"吸引使用者与贡献者"的开源目标匹配；本项目未复制 pi/PI-Desktop 代码，无许可证传染障碍。
- 所有者主力环境为 Windows，先深耕单平台可显著收窄 Phase 3 的适配面（路径/钥匙串/终端/打包签名差异）。

## 后果

- 平台细节排期变化：Windows 成为一等公民——bash 工具的 PowerShell 支持在 Phase 1 即为一等路径（已在 phase-1 范围内）、凭证存储先做 Windows 凭据管理器、CI 先行 windows workflow；macOS 钥匙串/Linux keyring 在 Phase 3 后补齐。
- 仓库根新增 LICENSE；README 顶部声明许可证。
- MIT 无专利条款：若未来有专利诉求需重新评估（届时另立 ADR；当前项目无已知专利暴露）。
- 产品名变更影响面仅文档措辞（此前未写代码，无命名迁移成本）。
