// 扩展运行时 pi shim（设计 01/04/05）：API 面与派发语义在这里，Rust 只提供宿主桥。
globalThis.__timo_state = {
  tools: Object.create(null),
  handlers: Object.create(null),
  commands: Object.create(null),
  lastError: undefined,
  lastResult: undefined,
  diagOnce: new Set(),
};

function __timo_parse(text, fallback) {
  if (text === undefined || text === null || text === "") {
    return fallback;
  }
  try {
    return JSON.parse(text);
  } catch {
    return fallback;
  }
}

globalThis.__timo_reset_error = function () {
  globalThis.__timo_state.lastError = undefined;
  globalThis.__timo_state.lastResult = undefined;
};

function __timo_error_text(error) {
  if (error === undefined || error === null) {
    return String(error);
  }
  const message = error.message !== undefined ? String(error.message) : "";
  const stack = error.stack !== undefined ? String(error.stack) : String(error);
  if (!message || stack.includes(message)) {
    return stack;
  }
  return message + (stack ? "\n" + stack : "");
}

function __timo_diag(code, message) {
  globalThis.__timo_native.diag(String(code), String(message));
}

function __timo_diag_once(key, code, message) {
  const state = globalThis.__timo_state;
  if (state.diagOnce.has(key)) {
    return;
  }
  state.diagOnce.add(key);
  __timo_diag(code, message);
}

function __timo_make_ctx(config) {
  const native = globalThis.__timo_native;
  const base = {
    mode: "print",
    hasUI: Boolean(config.hasUI),
    cwd: String(config.cwd || ""),
    signal: {
      get aborted() {
        return native.isAborted();
      },
      addEventListener() {},
      removeEventListener() {},
    },
    abort: () => native.abort(),
    isIdle: () => !native.isRunning(),
    hasPendingMessages: () => false,
    getSystemPrompt: () =>
      String(
        globalThis.__timo_system_prompt !== undefined
          ? globalThis.__timo_system_prompt
          : config.systemPrompt || "",
      ),
    getContextUsage: () => undefined,
    compact: () => __timo_diag_once("ctx.compact", "E202", "ctx.compact is not implemented"),
    shutdown: () => __timo_diag_once("ctx.shutdown", "E202", "ctx.shutdown is not implemented"),
    isProjectTrusted: () => true,
    model: config.model ?? null,
    scopedModels: [],
    thinkingLevel: config.thinkingLevel ?? "off",
    getSystemPromptOptions: () => ({}),
    waitForIdle: async () => {},
    sessionManager: new Proxy(
      {},
      {
        get: () => () => {
          __timo_diag_once("ctx.sessionManager", "E202", "ctx.sessionManager is not implemented");
          throw new Error("sessionManager is not implemented");
        },
      },
    ),
    modelRegistry: new Proxy(
      {},
      {
        get: () => () => {
          __timo_diag_once("ctx.modelRegistry", "E202", "ctx.modelRegistry is not implemented");
          throw new Error("modelRegistry is not implemented");
        },
      },
    ),
    ui: null,
  };
  const uiBase = {
    notify: (message, type) => native.notify(String(message), String(type || "info")),
    select: async (title, options) => {
      const raw = native.select(String(title), JSON.stringify(options || []));
      return raw === null || raw === undefined ? undefined : raw;
    },
    confirm: async (title, message) => native.confirm(String(title), String(message)),
    input: async (title, placeholder) => {
      const raw = native.input(String(title), String(placeholder || ""));
      return raw === null || raw === undefined ? undefined : raw;
    },
    custom: () => {
      __timo_diag_once("ctx.ui.custom", "E302", "ctx.ui.custom is a no-op");
      return new Promise(() => {});
    },
  };
  base.ui = new Proxy(uiBase, {
    get(target, prop) {
      if (prop in target) {
        return target[prop];
      }
      if (typeof prop === "string") {
        __timo_diag_once("ctx.ui." + prop, "E302", "ctx.ui." + prop + " is a no-op");
        return () => undefined;
      }
      return undefined;
    },
  });
  return base;
}

function __timo_make_tool_ctx(config) {
  const ctx = __timo_make_ctx(config);
  ctx.tools = [];
  ctx.executeTool = async () => {
    __timo_diag_once("ctx.executeTool", "E202", "ctx.executeTool is not implemented");
    return {
      isError: true,
      content: [{ type: "text", text: "ctx.executeTool is not implemented" }],
    };
  };
  return ctx;
}

globalThis.fetch = async function (url, options) {
  const raw = globalThis.__timo_native.fetch(
    JSON.stringify({ url: String(url), options: options || {} }),
  );
  const out = __timo_parse(raw, { ok: false, error: "fetch bridge failed" });
  if (!out.ok) {
    const error = new Error(out.error || "fetch failed");
    error.code = String(out.error || "").split(" ")[0];
    throw error;
  }
  const headers = {};
  for (const pair of out.headers || []) {
    headers[String(pair[0])] = String(pair[1]);
  }
  return {
    ok: out.status >= 200 && out.status < 300,
    status: out.status,
    headers,
    async text() {
      return out.body || "";
    },
    async json() {
      return JSON.parse(out.body || "null");
    },
  };
};

globalThis.__timo_make_api = function (config) {
  const native = globalThis.__timo_native;
  const state = globalThis.__timo_state;
  const prefix = String(config.prefix || "");
  const noop = (name) => () => {
    __timo_diag_once("api." + name, "E202", "pi." + name + " is not implemented");
  };
  return {
    on(event, handler) {
      const key = String(event);
      (state.handlers[key] || (state.handlers[key] = [])).push(handler);
      native.hookRegistered(key);
      return () => {};
    },
    registerTool(def) {
      const meta = {
        name: prefix + def.name,
        jsName: def.name,
        label: def.label || def.name,
        description: def.description || "",
        parameters: def.parameters || {},
        promptSnippet: def.promptSnippet ?? null,
        promptGuidelines: def.promptGuidelines || [],
        outputSchema: def.outputSchema ?? null,
        exposure: def.exposure || "direct",
        annotations: def.annotations ?? null,
        executionMode: def.executionMode ?? null,
      };
      state.tools[meta.name] = { def };
      native.toolRegistered(JSON.stringify(meta));
    },
    registerCommand(name, options) {
      const full = prefix + name;
      state.commands[full] = options;
      native.commandRegistered(full);
    },
    registerFlag() {},
    getFlag(name) {
      const flags = config.flags || {};
      return Object.prototype.hasOwnProperty.call(flags, String(name))
        ? flags[String(name)]
        : undefined;
    },
    registerShortcut() {},
    registerMessageRenderer() {},
    registerMarkdownTransformer() {},
    registerEntryRenderer() {},
    registerToolRenderer() {},
    sendMessage: noop("sendMessage"),
    sendUserMessage(content, options) {
      native.action("sendUserMessage", JSON.stringify({ content, options: options || {} }));
    },
    appendEntry: noop("appendEntry"),
    exec: async (command, args, options) => {
      const raw = native.exec(
        JSON.stringify({ command: String(command), args: args || [], options: options || {} }),
      );
      const out = __timo_parse(raw, { ok: false, error: "exec bridge failed" });
      if (!out.ok) {
        throw new Error(out.error || "exec failed");
      }
      return { stdout: out.stdout || "", stderr: out.stderr || "", code: out.code ?? 0 };
    },
    getActiveTools: () => __timo_parse(native.action("getActiveTools", "{}"), []),
    getAllTools: () => __timo_parse(native.action("getAllTools", "{}"), []),
    setActiveTools: (names) => {
      native.action("setActiveTools", JSON.stringify(names || []));
    },
    getSettings: () => __timo_parse(native.action("getSettings", "{}"), {}),
    getCommands: () => __timo_parse(native.action("getCommands", "{}"), []),
    setModel: async (model) => __timo_parse(native.action("setModel", JSON.stringify(model)), false),
    getThinkingLevel: () => __timo_parse(native.action("getThinkingLevel", "{}"), "off"),
    setThinkingLevel: (level) => {
      native.action("setThinkingLevel", JSON.stringify(level));
    },
    registerProvider: noop("registerProvider"),
    unregisterProvider: noop("unregisterProvider"),
    registerMcpServer: noop("registerMcpServer"),
    unregisterMcpServer: noop("unregisterMcpServer"),
    getMcpServers: () => [],
    registerVirtualModel: noop("registerVirtualModel"),
    unregisterVirtualModel: noop("unregisterVirtualModel"),
    events: {
      on() {
        return () => {};
      },
      off() {},
      emit() {},
    },
  };
};

globalThis.__timo_run_factory = async function (configJson) {
  globalThis.__timo_reset_error();
  try {
    const config = __timo_parse(configJson, {});
    const pi = globalThis.__timo_make_api(config);
    await globalThis.__timo_factory(pi);
  } catch (error) {
    globalThis.__timo_state.lastError = __timo_error_text(error);
  }
};

globalThis.__timo_call_tool = async function (name, argsJson) {
  globalThis.__timo_reset_error();
  try {
    const entry = globalThis.__timo_state.tools[name];
    if (!entry) {
      throw new Error("tool not registered: " + name);
    }
    const config = globalThis.__timo_config || {};
    const ctx = __timo_make_tool_ctx(config);
    const args = __timo_parse(argsJson, {});
    const result = await entry.def.execute(
      "toolcall",
      args,
      ctx.signal,
      (partial) => __timo_native.toolUpdate(JSON.stringify(partial)),
      ctx,
    );
    globalThis.__timo_state.lastResult = JSON.stringify(result ?? null);
  } catch (error) {
    globalThis.__timo_state.lastError = __timo_error_text(error);
  }
};

globalThis.__timo_run_command = async function (name, argsJson) {
  globalThis.__timo_reset_error();
  try {
    const entry = globalThis.__timo_state.commands[name];
    if (!entry || typeof entry.handler !== "function") {
      throw new Error("command not registered: " + name);
    }
    const config = globalThis.__timo_config || {};
    const args = __timo_parse(argsJson, []);
    await entry.handler(args, __timo_make_ctx(config));
  } catch (error) {
    globalThis.__timo_state.lastError = __timo_error_text(error);
  }
};

globalThis.__timo_dispatch = async function (event, payloadJson) {
  globalThis.__timo_reset_error();
  const state = globalThis.__timo_state;
  const handlers = state.handlers[event] || [];
  let current = __timo_parse(payloadJson, null);
  let merged = null;
  try {
    for (const handler of handlers) {
      if (globalThis.__timo_native.isAborted()) {
        break;
      }
      try {
        const result = await handler(
          current,
          __timo_make_ctx(globalThis.__timo_config || {}),
        );
        if (event === "tool_call") {
          if (result && result.block) {
            merged = { ...(merged || {}), ...result };
            break;
          }
          if (result) {
            merged = { ...(merged || {}), ...result };
            if (result.input !== undefined) {
              current = { ...current, input: result.input };
            }
          }
        } else if (event === "tool_result") {
          if (result) {
            current = { ...current, ...result };
            merged = result;
          }
        } else if (event === "context" || event === "context_with_system") {
          if (result && result.messages) {
            current = { ...current, messages: result.messages };
          }
        } else if (event === "before_agent_start") {
          if (result && result.systemPrompt !== undefined) {
            current = { ...current, systemPrompt: result.systemPrompt };
          }
        } else if (result !== undefined && result !== null) {
          merged = result;
        }
      } catch (error) {
        __timo_diag(
          "E302",
          "handler " + event + " threw: " + __timo_error_text(error),
        );
      }
    }
    if (event === "tool_call") {
      merged = { ...(merged || {}), input: current ? current.input : undefined };
    } else if (event === "context" || event === "context_with_system") {
      merged = { messages: current ? current.messages : undefined };
    } else if (event === "before_agent_start") {
      merged = { systemPrompt: current ? current.systemPrompt : undefined };
    }
    state.lastResult = JSON.stringify(merged ?? null);
  } catch (error) {
    state.lastError = __timo_error_text(error);
  }
};
