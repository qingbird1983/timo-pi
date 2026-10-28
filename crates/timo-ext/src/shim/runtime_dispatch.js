globalThis.__timo_route_virtual = async function (provider, id, payloadJson) {
  globalThis.__timo_reset_error();
  try {
    const byProvider = globalThis.__timo_state.virtualModels[String(provider)];
    const def = byProvider && byProvider[String(id)];
    if (!def || typeof def.route !== "function") {
      throw new Error("virtual model not registered: " + provider + "/" + id);
    }
    const request = __timo_parse(payloadJson, {});
    const config = globalThis.__timo_config || {};
    const ctx = __timo_make_ctx({
      ...config,
      model: request.model,
      thinkingLevel: request.thinkingLevel,
    });
    const result = await def.route(request, ctx);
    globalThis.__timo_state.lastResult = JSON.stringify(result ?? null);
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
        } else if (event === "project_trust") {
          // 第一个给出 yes/no 的扩展拥有决定；undecided 继续问下一个。
          if (result && (result.trusted === "yes" || result.trusted === "no")) {
            merged = result;
            break;
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
