/**
 * subagent —— 用无头子进程实现"代理调代理"（设计 docs/design/phase-3.5/08-subagent.md）。
 *
 * 行为蓝本是 pi 官方 subagent 示例（MIT，只读不抄）：一次工具调用 → 起一个
 * `timo --mode json -p --no-session` 子进程 → 独立上下文 / 受限工具面 / 指定模型
 * → 结果与用量回到主会话。
 *
 * 宿主面约束（我们的沙箱没有 OS 面）：
 * - 读文件用 `pi.readFile` / `pi.listDir`（受 `fs.read` 作用域强制，D51）
 * - 系统提示走命令行文本，不写临时文件（D52）
 * - 没有流式进度：`pi.exec` 返回才有结果（D53）
 * - parallel 用宿主 `pi.execParallel`（≤8 任务、4 并发，D54）
 */
import { Type } from "@earendil-works/pi-ai";
import { defineTool, type ExtensionAPI } from "@earendil-works/pi-coding-agent";

type Agent = {
  name: string;
  description: string;
  tools: string[] | null;
  model: string | null;
  system: string;
  origin: string;
  level: "user" | "project";
};

type Step = { tool: string; detail: string };

type Run = {
  agent: string;
  ok: boolean;
  exitCode: number;
  turns: number;
  contextTokens: number;
  output: string;
  steps: Step[];
  usage: Usage;
  incomplete: boolean;
  error?: string;
};

type Usage = {
  input: number;
  output: number;
  cache_read: number;
  cache_write: number;
  total_tokens: number;
  cost: { input: number; output: number; cache_read: number; cache_write: number; total: number };
};

const MAX_CONTENT = 24 * 1024;
const DEFAULT_TIMEOUT_MS = 600_000;

/**
 * `pi` 只传给默认导出，不传进工具的 execute（shim 第 5 参数是 ctx）。
 * 每个扩展跑在自己独立的 QuickJS 上下文里，这里按实例存一份即可。
 */
let hostPi: any = null;

function hostApi(): any {
  if (!hostPi) {
    throw new Error("subagent: extension api is not bound yet");
  }
  return hostPi;
}

function emptyUsage(): Usage {
  return {
    input: 0,
    output: 0,
    cache_read: 0,
    cache_write: 0,
    total_tokens: 0,
    cost: { input: 0, output: 0, cache_read: 0, cache_write: 0, total: 0 },
  };
}

function addUsage(total: Usage, part: Usage | undefined): void {
  if (!part) {
    return;
  }
  for (const key of ["input", "output", "cache_read", "cache_write", "total_tokens"]) {
    total[key] += Number(part[key] || 0);
  }
  const src = part.cost || {};
  for (const key of ["input", "output", "cache_read", "cache_write", "total"]) {
    total.cost[key] += Number(src[key] || 0);
  }
}

function join(dir: string, name: string): string {
  if (!dir) {
    return name;
  }
  return dir.endsWith("/") || dir.endsWith("\\") ? dir + name : dir + "/" + name;
}

/** `---\nfrontmatter\n---\n正文`；没有围栏就不是代理定义。 */
function splitFrontmatter(text: string): { head: string; body: string } | null {
  const match = /^---[ \t]*\r?\n([\s\S]*?)\r?\n---[ \t]*\r?\n?([\s\S]*)$/.exec(text);
  return match ? { head: match[1], body: match[2] } : null;
}

function unquote(value: string): string {
  const trimmed = value.trim();
  if (trimmed.length > 1 && (trimmed[0] === '"' || trimmed[0] === "'") && trimmed[trimmed.length - 1] === trimmed[0]) {
    return trimmed.slice(1, -1);
  }
  return trimmed;
}

/**
 * 极简 frontmatter：只认 `key: value` 一行一条（值支持 `"quoted"`、`[a, b]` 与 `a, b`）。
 * 不引 YAML 库——沙箱里没有 node 模块，字段面也只有四个。
 */
function parseAgent(name: string, origin: string, level: "user" | "project", text: string): Agent | null {
  const parts = splitFrontmatter(text);
  if (!parts) {
    return null;
  }
  const fields: Record<string, string> = {};
  for (const line of parts.head.split(/\r?\n/)) {
    const index = line.indexOf(":");
    if (index > 0) {
      fields[line.slice(0, index).trim()] = unquote(line.slice(index + 1));
    }
  }
  const rawTools = fields.tools || "";
  const tools = rawTools
    ? rawTools.replace(/^\[|\]$/g, "").split(",").map((s) => s.trim()).filter((s) => s.length > 0)
    : null;
  return {
    name: fields.name || name,
    description: fields.description || "",
    tools,
    model: fields.model || null,
    system: parts.body.trim(),
    origin,
    level,
  };
}

/** 一个目录里的 `*.md`；作用域被拒（E401/E403/E404）就当没有，坏文件报一次通知后跳过。 */
async function loadFromDir(pi: any, dir: string, level: "user" | "project", notify: (msg: string) => void): Promise<Agent[]> {
  let entries;
  try {
    entries = await pi.listDir(dir);
  } catch (error: any) {
    if (String(error.code || error.message).startsWith("E40")) {
      notify(`subagent: cannot list ${dir} (${error.code || "denied"}) — grant the extension fs.read for it`);
    }
    return [];
  }
  const out: Agent[] = [];
  for (const entry of entries || []) {
    if (entry.isDir || !entry.name.endsWith(".md")) {
      continue;
    }
    const path = join(dir, entry.name);
    try {
      const agent = parseAgent(entry.name.slice(0, -3), path, level, await pi.readFile(path));
      if (agent) {
        out.push(agent);
      } else {
        notify(`subagent: skipped ${path} (missing --- frontmatter)`);
      }
    } catch (error: any) {
      notify(`subagent: skipped ${path} (${error.message})`);
    }
  }
  return out;
}

/** 用户级先、项目级后，同名后者覆盖（设计 08 §5）。 */
async function discover(pi: any, ctx: any): Promise<Agent[]> {
  const level = new Map<string, Agent>();
  const user = await loadFromDir(pi, join(pi.getAgentDir(), "agents"), "user", (m) => ctx.ui.notify(m, "warning"));
  const project = await loadFromDir(pi, join(ctx.cwd, ".timo/agents"), "project", (m) => ctx.ui.notify(m, "warning"));
  for (const agent of user.concat(project)) {
    level.set(agent.name, agent);
  }
  return [...level.values()];
}

function unknownAgent(name: string, agents: Agent[]): string {
  const listed = agents.map((a) => `${a.name} (${a.level})`).join(", ") || "none discovered";
  return `unknown subagent \`${name}\`; available: ${listed}`;
}

/** 只给 `--model <id>`：provider 留空由内核按目录唯一性判定（歧义会在子进程 stderr 列出候选）。 */
function modelArgs(agent: Agent): string[] {
  if (!agent.model) {
    return [];
  }
  const slash = agent.model.indexOf("/");
  if (slash > 0) {
    return ["--provider", agent.model.slice(0, slash), "--model", agent.model.slice(slash + 1)];
  }
  return ["--model", agent.model];
}

function buildArgs(pi: any, ctx: any, agent: Agent, task: string): string[] {
  // `-p` 在我们的 CLI 里取值（可重复）：任务文本必须紧跟其后，不能放末尾位置。
  const args = ["--mode", "json", "--no-session", "--thinking", ctx.thinkingLevel || "off"];
  if (agent.tools) {
    args.push("--tools", agent.tools.join(","));
  }
  args.push(...modelArgs(agent));
  args.push("--append-system-prompt", agent.system, "-p", `Task: ${task}`);
  return args;
}

function textOf(message: any): string {
  const blocks = (message && message.content) || [];
  return blocks
    .filter((b: any) => b && b.type === "text" && typeof b.text === "string")
    .map((b: any) => b.text)
    .join("");
}

/**
 * 一次子进程运行的读账。
 * 用量只认 `agent_end.messages`（一个出处）：`message` 事件与它内容重叠，两处累加会双计。
 * 子进程被 kill / 非 0 退出时可能没有 `agent_end`，才退回用 `message` 事件尽力统计。
 */
function consume(stdout: string, fallback: Run): Run {
  const lines = stdout.split(/\r?\n/).filter((l) => l.trim().length > 0);
  let sawEnd = false;
  const messages: any[] = [];
  const streamed: any[] = [];
  const steps: Step[] = [];
  let contextTokens = 0;
  for (const line of lines) {
    let event: any;
    try {
      event = JSON.parse(line);
    } catch {
      continue;
    }
    if (event.type === "message" && event.message) {
      streamed.push(event.message);
    } else if (event.type === "tool_execution_end") {
      const name = event.tool_name || event.toolName || "tool";
      steps.push({ tool: String(name), detail: String(event.is_error ? "failed" : "done") });
    } else if (event.type === "agent_end") {
      sawEnd = true;
      messages.push(...(event.messages || []));
    } else if (event.type === "error" && event.context) {
      steps.push({ tool: "error", detail: String(event.context) });
    }
  }
  const source = sawEnd ? messages : streamed;
  const usage = emptyUsage();
  let turns = 0;
  let output = "";
  for (const message of source) {
    if (message && message.role === "assistant") {
      turns += 1;
      addUsage(usage, message.usage);
      if (message.usage && message.usage.total_tokens !== undefined) {
        contextTokens = Number(message.usage.total_tokens);
      }
      const text = textOf(message);
      if (text) {
        output = text;
      }
    }
  }
  return {
    ...fallback,
    ok: sawEnd,
    turns,
    contextTokens,
    output,
    steps,
    usage,
    incomplete: !sawEnd,
  };
}

function failRun(agent: string, exitCode: number, stderr: string): Run {
  const tail = (stderr || "").slice(-4096);
  return {
    agent,
    ok: false,
    exitCode,
    turns: 0,
    contextTokens: 0,
    output: "",
    steps: [],
    usage: emptyUsage(),
    incomplete: true,
    error: `exit ${exitCode}: ${tail || "no stderr"}`,
  };
}

function renderRun(run: Run): string {
  const head = `${run.agent}: ${run.turns} turn(s), ${run.usage.total_tokens} tokens, $${run.usage.cost.total.toFixed(6)}`;
  if (!run.ok) {
    return `${head}\nFAILED ${run.error || "unknown failure"}`;
  }
  const trace = run.steps.length ? `\nsteps: ${run.steps.map((s) => s.tool).join(" → ")}` : "";
  return `${head}${trace}\n${run.output}`;
}

function cap(text: string): string {
  return text.length > MAX_CONTENT ? text.slice(0, MAX_CONTENT) + "\n[truncated]" : text;
}

function timeoutMs(pi: any): number {
  const raw = pi.getFlag ? pi.getFlag("timeout") : undefined;
  const parsed = Number(raw);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : DEFAULT_TIMEOUT_MS;
}

async function runOne(pi: any, ctx: any, agent: Agent, task: string): Promise<Run> {
  const bin = String(pi.getFlag("bin") || "timo");
  const fallback: Run = {
    agent: agent.name,
    ok: false,
    exitCode: -1,
    turns: 0,
    contextTokens: 0,
    output: "",
    steps: [],
    usage: emptyUsage(),
    incomplete: true,
  };
  let out;
  try {
    out = await pi.exec(bin, buildArgs(pi, ctx, agent, task), { timeoutMs: timeoutMs(pi) });
  } catch (error: any) {
    return { ...fallback, error: String(error.message || error) };
  }
  if (out.code !== 0) {
    return failRun(agent.name, out.code, out.stderr);
  }
  const run = consume(out.stdout, fallback);
  return { ...run, exitCode: out.code };
}

const subagentTool = defineTool({
  name: "subagent",
  label: "Subagent",
  description:
    "Run one or more subagents as headless child processes. Single: {agent, task}. Parallel: {tasks:[{agent,task}]}. Chain: {chain:[{agent,task}]}, where a task may use {previous}.",
  parameters: Type.Object({
    agent: Type.Optional(Type.String({ description: "Subagent name for a single run" })),
    task: Type.Optional(Type.String({ description: "Task text for a single run" })),
    tasks: Type.Optional(
      Type.Array(
        Type.Object({ agent: Type.String(), task: Type.String() }),
        { description: "Up to 8 tasks run in parallel" },
      ),
    ),
    chain: Type.Optional(
      Type.Array(Type.Object({ agent: Type.String(), task: Type.String() }), {
        description: "Steps run in order; `{previous}` is replaced with the prior answer",
      }),
    ),
  }),
  async execute(_toolCallId: string, params: any, _signal: any, _onUpdate: any, ctx: any) {
    const pi = hostApi();
    const agents = await discover(pi, ctx);
    const byName = new Map(agents.map((a) => [a.name, a]));
    const pick = (name: string): Agent => {
      const agent = byName.get(name);
      if (!agent) {
        throw new Error(unknownAgent(name, agents));
      }
      return agent;
    };

    const runs: Run[] = [];
    const single = params.agent ? [{ agent: params.agent, task: params.task || "" }] : null;
    const parallel = Array.isArray(params.tasks) ? params.tasks : null;
    const chain = Array.isArray(params.chain) ? params.chain : null;
    const chosen = single || parallel || chain;
    if (!chosen) {
      throw new Error("subagent needs one of: {agent, task}, {tasks:[...]}, {chain:[...]}");
    }

    if (chain) {
      let previous = "";
      for (const step of chain) {
        if (ctx.signal.aborted) {
          runs.push({ ...failRun(step.agent, -1, ""), error: "cancelled before step" });
          break;
        }
        const agent = pick(step.agent);
        const task = String(step.task || "").replace(/\{previous\}/g, previous);
        const run = await runOne(pi, ctx, agent, task);
        runs.push(run);
        if (!run.ok) {
          break;
        }
        previous = run.output;
      }
    } else if (parallel) {
      if (parallel.length > 8) {
        throw new Error("subagent parallel supports at most 8 tasks");
      }
      const bin = String(pi.getFlag("bin") || "timo");
      const requests = parallel.map((item: any) => ({
        command: bin,
        args: buildArgs(pi, ctx, pick(item.agent), item.task || ""),
        timeoutMs: timeoutMs(pi),
      }));
      const outputs = await pi.execParallel(requests);
      for (let i = 0; i < parallel.length; i += 1) {
        const out = (outputs && outputs[i]) || null;
        const agent = pick(parallel[i].agent);
        if (!out) {
          runs.push({ ...failRun(agent.name, -1, ""), error: "no result from host" });
        } else if (out.code !== 0) {
          runs.push(failRun(agent.name, out.code, out.stderr));
        } else {
          const run = consume(out.stdout, {
            agent: agent.name,
            ok: false,
            exitCode: out.code,
            turns: 0,
            contextTokens: 0,
            output: "",
            steps: [],
            usage: emptyUsage(),
            incomplete: true,
          });
          runs.push({ ...run, exitCode: out.code });
        }
      }
    } else {
      const item = chosen[0];
      runs.push(await runOne(pi, ctx, pick(item.agent), item.task || ""));
    }

    const total = emptyUsage();
    for (const run of runs) {
      addUsage(total, run.usage);
    }
    const failed = runs.filter((r) => !r.ok);
    return {
      content: [{ type: "text", text: cap(runs.map(renderRun).join("\n\n---\n\n")) }],
      details: {
        runs: runs.map((r) => ({
          agent: r.agent,
          source: r.agent,
          ok: r.ok,
          exitCode: r.exitCode,
          turns: r.turns,
          contextTokens: r.contextTokens,
          steps: r.steps,
          usage: r.usage,
          incomplete: r.incomplete,
          error: r.error || null,
        })),
        failed: failed.length,
      },
      // 子代理用量回到主会话的 Tools/summaries 桶（设计 08 §4.3、D55）。
      usage: total,
    };
  },
});

export default function (pi: ExtensionAPI) {
  hostPi = pi;
  pi.registerTool(subagentTool);
  pi.registerCommand("subagents", {
    description: "List discovered subagent definitions",
    async handler(_args: string, ctx: any) {
      for (const agent of await discover(pi, ctx)) {
        ctx.ui.notify(`${agent.name} [${agent.level}] ${agent.model || "default model"}: ${agent.description}`, "info");
      }
    },
  });
}
