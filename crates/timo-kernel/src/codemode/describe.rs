//! codemode 的工具声明渲染与描述预算（设计 phase-3.5/11 §4.2）。

use std::collections::HashSet;

use serde_json::Value;
use timo_tools::{CallableTool, Exposure, Namespace};

/// 可调工具的脚本声明。
#[derive(Debug, Clone)]
pub(super) struct Decl {
    /// 脚本里的标识符（消毒后）。
    pub id: String,
    /// 注册表原名。
    pub name: String,
    /// `name(arg: type) -> result`。
    pub declaration: String,
    /// 暴露面（`Deferred` 可调但默认不列进描述）。
    pub exposure: Exposure,
}

/// 默认描述预算（估计 token；pi 同值）。
pub(super) const DEFAULT_INLINE_BUDGET: usize = 3_000;
/// 每 token 估算字符数。
const CHARS_PER_TOKEN: usize = 4;
/// 工具描述在声明里的最大字符数（取首行）。
const DESCRIPTION_CHARS: usize = 160;

const INTRO: &str = "Run JavaScript that calls other tools. The input is raw JavaScript (not JSON, no code fence), run as an async function body in a QuickJS sandbox: top-level await and return work. No Node, file system, network, or timers.\n- `await tools.<name>({ ...args })` resolves to the tool's result (a string unless its declaration says otherwise) and rejects with an Error on failure.\n- Optional first line: `// @options: {\"max_output_tokens\": 10000, \"timeout_ms\": 60000}`";

const GLOBALS: &str = "Globals:\n- `text(value)`, `console.log(...)`, and top-level `return` add output; `exit()` ends the script. With several text items, each starts with a `==> text N/M <==` line, and `console` lines follow in one `<console_output>` block.\n- `ALL_TOOLS` and `describeTool(name)`: find tools that are not listed below.";

const UNLISTED_NOTE: &str =
    "Some tools are callable but not listed here; inspect ALL_TOOLS or call describeTool(name).";

/// 工具名转脚本标识符（非法字符变 `_`；首位数字补 `_`）。
pub(super) fn sanitize(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for (index, ch) in name.chars().enumerate() {
        let valid = ch.is_ascii_alphanumeric() || ch == '_' || ch == '$';
        if valid && index == 0 && ch.is_ascii_digit() {
            out.push('_');
            out.push(ch);
        } else if valid {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push('_');
    }
    out
}

/// 可调集合的声明（去掉 `Hidden`；消毒后重名先到先得）。
pub(super) fn declarations(catalog: &[CallableTool]) -> Vec<Decl> {
    let mut seen = HashSet::new();
    let mut decls = Vec::new();
    for tool in catalog {
        if tool.exposure == Exposure::Hidden {
            continue;
        }
        let id = sanitize(&tool.name);
        if !seen.insert(id.clone()) {
            continue;
        }
        decls.push(Decl {
            id,
            name: tool.name.clone(),
            declaration: declaration(tool),
            exposure: tool.exposure,
        });
    }
    decls
}

/// 单个工具的紧凑声明。
pub(super) fn declaration(tool: &CallableTool) -> String {
    let mut params = Vec::new();
    let required: HashSet<&str> = tool
        .parameters
        .get("required")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if let Some(properties) = tool.parameters.get("properties").and_then(Value::as_object) {
        // 按名字排序：`serde_json::Map` 的顺序取决于特性开关，声明必须稳定。
        let mut names: Vec<&String> = properties.keys().collect();
        names.sort();
        for name in names {
            let schema = &properties[name];
            let optional = if required.contains(name.as_str()) {
                ""
            } else {
                "?"
            };
            params.push(format!("{name}{optional}: {}", type_of(schema)));
        }
    }
    let mut out = format!(
        "{}({}) -> {}",
        tool.name,
        params.join(", "),
        result_type(tool)
    );
    let summary = tool
        .description
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .take(DESCRIPTION_CHARS)
        .collect::<String>();
    if !summary.is_empty() {
        out.push_str(": ");
        out.push_str(&summary);
    }
    out
}

fn type_of(schema: &Value) -> String {
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        let labels: Vec<String> = values
            .iter()
            .map(|value| match value {
                Value::String(text) => format!("\"{text}\""),
                other => other.to_string(),
            })
            .collect();
        if !labels.is_empty() {
            return labels.join(" | ");
        }
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("string") => "string".into(),
        Some("number" | "integer") => "number".into(),
        Some("boolean") => "boolean".into(),
        Some("array") => "array".into(),
        Some("object") => "object".into(),
        _ => "any".into(),
    }
}

fn result_type(tool: &CallableTool) -> String {
    let Some(schema) = &tool.output_schema else {
        return "string".into();
    };
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return match schema.get("type").and_then(Value::as_str) {
            Some("string") => "string".into(),
            _ => "any".into(),
        };
    }
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return "object".into();
    };
    if properties.is_empty() {
        return "object".into();
    }
    let required: HashSet<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut names: Vec<&String> = properties.keys().collect();
    names.sort();
    let fields: Vec<String> = names
        .into_iter()
        .map(|name| {
            if required.contains(name.as_str()) {
                name.clone()
            } else {
                format!("{name}?")
            }
        })
        .collect();
    format!("{{ {} }}", fields.join(", "))
}

/// 工具面描述：脚本形态、全局、按命名空间分组的声明（受预算约束）。
pub(super) fn render(catalog: &[CallableTool], budget: usize) -> String {
    // `Deferred` 仍可调（ALL_TOOLS/describeTool），但默认不占描述预算。
    let decls: Vec<Decl> = declarations(catalog)
        .into_iter()
        .filter(|decl| decl.exposure != Exposure::Deferred)
        .collect();
    let mut sections = vec![INTRO.to_string(), GLOBALS.to_string()];
    if decls.is_empty() {
        return sections.join("\n\n");
    }

    // 无命名空间的组在前，其余按名字排序。
    let mut groups: Vec<Group> = vec![Group {
        namespace: None,
        entries: Vec::new(),
    }];
    for decl in &decls {
        let namespace = catalog
            .iter()
            .find(|tool| tool.name == decl.name)
            .and_then(|tool| tool.namespace.clone());
        let index = match &namespace {
            None => 0,
            Some(ns) => {
                if let Some(index) = groups.iter().position(|group| {
                    group
                        .namespace
                        .as_ref()
                        .is_some_and(|item| item.name == ns.name)
                }) {
                    index
                } else {
                    groups.push(Group {
                        namespace: Some(ns.clone()),
                        entries: Vec::new(),
                    });
                    groups.len() - 1
                }
            }
        };
        let text = section_of(decl);
        let cost = text.chars().count().div_ceil(CHARS_PER_TOKEN);
        groups[index].entries.push(Entry { cost, text });
    }
    groups[1..].sort_by(|a, b| {
        a.namespace
            .as_ref()
            .map(|ns| ns.name.as_str())
            .cmp(&b.namespace.as_ref().map(|ns| ns.name.as_str()))
    });
    for group in &mut groups {
        group.entries.sort_by_key(|entry| entry.cost);
    }

    let shown = select(&groups, budget);
    let mut body = Vec::new();
    let mut unlisted = 0usize;
    for (group_index, group) in groups.iter().enumerate() {
        let visible: Vec<&Entry> = group
            .entries
            .iter()
            .enumerate()
            .filter(|(entry_index, _)| shown.contains(&(group_index, *entry_index)))
            .map(|(_, entry)| entry)
            .collect();
        unlisted += group.entries.len() - visible.len();
        if visible.is_empty() && group.namespace.is_none() {
            continue;
        }
        if let Some(ns) = &group.namespace {
            let listing = if visible.is_empty() {
                " (tools not listed)"
            } else if visible.len() < group.entries.len() {
                " (some tools not listed)"
            } else {
                ""
            };
            let mut heading = format!("## {}{listing}", ns.name);
            if let Some(description) = ns
                .description
                .as_deref()
                .map(str::trim)
                .filter(|description| !description.is_empty())
            {
                heading.push('\n');
                heading.push_str(description);
            }
            body.push(heading);
        }
        for entry in visible {
            body.push(entry.text.clone());
        }
    }
    if unlisted > 0 && !body.is_empty() {
        body.push(UNLISTED_NOTE.to_string());
    }
    sections.push(body.join("\n\n"));
    sections.join("\n\n")
}

struct Entry {
    cost: usize,
    text: String,
}

struct Group {
    namespace: Option<Namespace>,
    entries: Vec<Entry>,
}

/// 预算选择：每组每轮放最便宜的一个；放不下的组退出，其余组继续。
fn select(groups: &[Group], budget: usize) -> HashSet<(usize, usize)> {
    let mut queues: Vec<Vec<usize>> = groups
        .iter()
        .map(|group| (0..group.entries.len()).collect())
        .collect();
    let mut remaining = budget;
    let mut shown = HashSet::new();
    let mut active: Vec<usize> = (0..groups.len())
        .filter(|index| !queues[*index].is_empty())
        .collect();
    while !active.is_empty() {
        let mut next = Vec::new();
        for &group in &active {
            let Some(&entry) = queues[group].first() else {
                continue;
            };
            let cost = groups[group].entries[entry].cost;
            if cost > remaining {
                continue;
            }
            remaining -= cost;
            shown.insert((group, entry));
            queues[group].remove(0);
            if !queues[group].is_empty() {
                next.push(group);
            }
        }
        active = next;
    }
    shown
}

fn section_of(decl: &Decl) -> String {
    let heading = if decl.id == decl.name {
        format!("### `{}`", decl.id)
    } else {
        format!("### `{}` (`{}`)", decl.id, decl.name)
    };
    format!("{heading}\n{}", decl.declaration)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool(name: &str, exposure: Exposure) -> CallableTool {
        CallableTool {
            name: name.into(),
            description: format!("{name} does things"),
            parameters: json!({"type": "object", "properties": {"path": {"type": "string"}, "n": {"type": "number"}}, "required": ["path"]}),
            output_schema: Some(
                json!({"type": "object", "properties": {"output": {"type": "string"}, "exit_code": {"type": "integer"}}, "required": ["output"]}),
            ),
            namespace: None,
            prompt_guidelines: vec![],
            execution_mode: timo_tools::ExecutionMode::Parallel,
            exposure,
        }
    }

    #[test]
    fn sanitize_keeps_identifier_shape() {
        assert_eq!(
            sanitize("mcp__dev-radius__search"),
            "mcp__dev_radius__search"
        );
        assert_eq!(sanitize("9lives"), "_9lives");
        assert_eq!(sanitize("a.b c"), "a_b_c");
        assert_eq!(sanitize(""), "_");
    }

    #[test]
    fn declaration_lists_arguments_and_result_fields() {
        let text = declaration(&tool("read", Exposure::Direct));
        assert_eq!(
            text,
            "read(n?: number, path: string) -> { exit_code?, output }: read does things"
        );
    }

    #[test]
    fn hidden_and_duplicate_names_are_dropped() {
        let mut a = tool("a.b", Exposure::Direct);
        let mut b = tool("a b", Exposure::Direct);
        a.description = "first".into();
        b.description = "second".into();
        let decls = declarations(&[tool("hidden", Exposure::Hidden), a, b]);
        assert_eq!(decls.len(), 1);
        assert!(decls[0].declaration.contains("first"), "{:?}", decls[0]);

        let deferred = tool("lazy", Exposure::Deferred);
        let decls = declarations(&[deferred.clone(), tool("read", Exposure::Direct)]);
        assert_eq!(decls.len(), 2, "Deferred 仍可调（进 ALL_TOOLS）");
        let text = render(&[deferred, tool("read", Exposure::Direct)], 10_000);
        assert!(!text.contains("### `lazy`"), "{text}");
        assert!(text.contains("### `read`"), "{text}");
    }

    #[test]
    fn budget_keeps_at_least_one_tool_per_group_and_reports_unlisted() {
        let names: Vec<CallableTool> = (0..20)
            .map(|index| tool(&format!("tool{index}"), Exposure::Direct))
            .collect();
        let text = render(&names, 120);
        assert!(text.contains("### `tool"), "{text}");
        assert!(text.contains(UNLISTED_NOTE), "{text}");
        let shown = text.matches("### `tool").count();
        assert!(shown > 0 && shown < 20, "{shown}");
    }

    #[test]
    fn namespaces_group_and_sort_after_unqualified_tools() {
        let mut namespaced = tool("mcp__demo__search", Exposure::Direct);
        namespaced.namespace = Some(Namespace {
            name: "mcp__demo".into(),
            description: Some("Demo server".into()),
        });
        let text = render(&[tool("read", Exposure::Direct), namespaced], 10_000);
        let read_at = text.find("### `read`").expect("read listed");
        let group_at = text.find("## mcp__demo").expect("namespace listed");
        assert!(read_at < group_at, "{text}");
        assert!(text.contains("Demo server"), "{text}");
    }
}
