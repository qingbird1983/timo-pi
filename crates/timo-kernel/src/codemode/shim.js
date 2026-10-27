// codemode 沙箱全局（设计 phase-3.5/11 §4.3）。
// 由 Rust 先注入 __timo_decls_json / __timo_limits_json / __timo_post，再执行本文件。
(function () {
  "use strict";

  var decls = JSON.parse(__timo_decls_json);
  var limits = JSON.parse(__timo_limits_json);
  var pending = new Map();
  var nextId = 1;
  var items = [];
  var consoleLines = [];
  var chars = 0;
  var calls = 0;
  var byId = new Map();
  var byRaw = new Map();

  function valueText(value) {
    if (typeof value === "string") return value;
    try {
      var json = JSON.stringify(value);
      return json === undefined ? String(value) : json;
    } catch (_error) {
      return String(value);
    }
  }

  function addChars(text) {
    chars += text.length;
    if (chars > limits.max_chars) {
      throw new Error("Script output limit exceeded (" + limits.max_chars + " characters)");
    }
  }

  function countCall() {
    calls += 1;
    if (calls > limits.max_calls) {
      throw new Error("Script output limit exceeded (" + limits.max_calls + " output calls)");
    }
  }

  globalThis.__timo_error = null;
  globalThis.text = function (value) {
    countCall();
    var text = valueText(value);
    addChars(text);
    items.push(text);
  };

  function consoleLine() {
    countCall();
    var line = Array.prototype.map.call(arguments, valueText).join(" ");
    addChars(line);
    consoleLines.push(line);
  }
  globalThis.console = {
    log: consoleLine,
    info: consoleLine,
    warn: consoleLine,
    error: consoleLine,
    debug: consoleLine,
  };

  globalThis.exit = function () {
    var error = new Error("exit");
    error.__timo_exit = true;
    throw error;
  };

  decls.forEach(function (decl) {
    byId.set(decl.id, decl);
    byRaw.set(decl.name, decl);
  });

  globalThis.ALL_TOOLS = decls.map(function (decl) {
    return { name: decl.name, description: decl.declaration };
  });

  globalThis.describeTool = function (name) {
    var decl = byId.get(name) || byRaw.get(name);
    return decl ? decl.declaration : undefined;
  };

  function callTool(name, decl) {
    return function (args) {
      if (!decl) {
        return Promise.reject(new Error('Unknown tool "' + name + '"'));
      }
      return new Promise(function (resolve, reject) {
        var id = nextId++;
        var text;
        try {
          text = JSON.stringify({ id: id, name: decl.name, args: args === undefined ? {} : args });
        } catch (error) {
          reject(new Error("arguments are not JSON-serializable: " + error));
          return;
        }
        pending.set(id, { resolve: resolve, reject: reject });
        __timo_post(text);
      });
    };
  }

  globalThis.tools = new Proxy(
    {},
    {
      get: function (_target, property) {
        if (typeof property !== "string") return undefined;
        return callTool(property, byId.get(property) || byRaw.get(property));
      },
    },
  );

  globalThis.__timo_describe_error = function (error) {
    if (error === null || error === undefined) {
      return "InternalError: the sandbox ran out of memory (or the script threw null)";
    }
    var head = error.message
      ? (error.name ? error.name + ": " + error.message : String(error.message))
      : String(error);
    return error.stack ? head + "\n" + error.stack : head;
  };

  globalThis.__timo_settle = function (id, ok, payload) {
    var entry = pending.get(id);
    if (!entry) return;
    pending.delete(id);
    if (ok) {
      var value;
      try {
        value = JSON.parse(payload);
      } catch (_error) {
        entry.reject(new Error("tool result is not valid JSON"));
        return;
      }
      entry.resolve(value);
    } else {
      entry.reject(new Error(payload));
    }
  };

  globalThis.__timo_pending_count = function () {
    return pending.size;
  };

  globalThis.__timo_take_output = function () {
    return JSON.stringify({
      items: items,
      console: consoleLines,
      error: globalThis.__timo_error,
    });
  };
})();
