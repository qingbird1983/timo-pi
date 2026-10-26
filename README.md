# Timo-pi

**A Rust-native coding agent: the kernel capabilities of [pi](https://github.com/earendil-works/pi), rebuilt in Rust — with a native desktop app instead of a terminal UI.**

[中文说明](README.zh-CN.md) · License: [MIT](LICENSE)

## What is Timo-pi?

Timo-pi reimplements the kernel of [pi](https://github.com/earendil-works/pi) — agent loop, LLM provider layer, tools, sessions, and the extension system — in Rust, and replaces the terminal UI with a **native desktop application** built on gpui-kit + NSH. Windows first; macOS and Linux stay build-ready. Open source under the MIT license.

## Highlights

- **Rust kernel** — streaming LLM providers (MVP: Anthropic Messages & OpenAI-compatible), an agent loop with an event bus, built-in tools (read / write / edit / shell / search), and crash-safe, append-only sessions.
- **MCP out of the box** — via the official `rmcp` SDK; MCP tools flow through the same pipeline as built-in tools (hooks, permissions, namespacing).
- **Dual-track plugin system**
  - *Track A — compatibility*: runs **orchestration-level pi extensions** (custom tools, commands, event hooks, providers) inside an embedded QuickJS sandbox. TUI-specific extension members degrade gracefully to no-ops with per-extension diagnostics.
  - *Track B — native*: a GUI plugin system authored in **TSX**, executed in a capability-based sandbox and distributed as **signed hot-update packages**, rendered through NSH's GPUI backend.
- **Docs-first engineering** — design docs, ADRs, standards and a phased roadmap were committed before any code; every phase ships with plain-language acceptance scenarios.

## Status

Pre-code. Phase 0 (repository, standards & design baseline) is complete; implementation begins with Phase 1 (kernel MVP).

- **Roadmap & progress board**: [docs/roadmap/00-roadmap.md](docs/roadmap/00-roadmap.md)
- Changelog: [CHANGELOG.md](CHANGELOG.md)

## Documentation

Project documentation is maintained in **Chinese** under [`docs/`](docs/README.md) (this README is the bilingual entry point):

| Directory | Contents |
|---|---|
| [`docs/research/`](docs/research/) | research notes: pi kernel & extension system, gpui-kit + NSH stack, reference project analysis |
| [`docs/adr/`](docs/adr/) | architecture decision records (why things are the way they are) |
| [`docs/architecture/`](docs/architecture/) | system design, layering rules, critical seams, plugin system, session format |
| [`docs/roadmap/`](docs/roadmap/) | phases, scope, acceptance scenarios; `00-roadmap.md` hosts the progress board |
| [`docs/standards/`](docs/standards/) | repo, git, documentation, coding, testing, security and AI-collaboration rules |
| [`docs/glossary.md`](docs/glossary.md) | plain-language glossary |

## License

[MIT](LICENSE) © 2026 Timo-pi contributors
