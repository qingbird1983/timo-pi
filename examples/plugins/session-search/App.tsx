// 会话搜索面板（设计 07）：只用只读查询与 openSession，不读正文、不跨工作区。
// 键盘限制见 README：NSH v1 没有方向键/Esc 事件，回车打开首条，清空用「×」。

import { computed, host, List, mount, signal, Text, View } from "nsh";
import { Input } from "nsh/base";

/** `sessions.list` / `sessions.search` 的一行（内核 `SessionSummary`）。 */
type SessionRow = {
  id: string;
  title: string;
  updatedAt: number;
  cwdLabel: string;
  messageCount: number;
};

const RECENT_LIMIT = 10;
const SEARCH_LIMIT = 20;
const DEBOUNCE_MS = 200;

const query = signal("");
const rows = signal<SessionRow[]>([]);
const failure = signal("");
const ready = signal(false);

const emptyText = computed(() =>
  query().trim() === "" ? "还没有会话" : "没有匹配的会话",
);

let sequence = 0;
let timer: number | undefined;

/** 宿主拒绝的文案形如 `E740: …`；保留原文便于对照错误码。 */
function reason(error: unknown): string {
  const text = error instanceof Error ? error.message : String(error);
  return text.trim() || "E742: host call failed";
}

async function load(text: string): Promise<void> {
  const current = ++sequence;
  const trimmed = text.trim();
  try {
    const reply =
      trimmed === ""
        ? await host.call("sessions.list", { limit: RECENT_LIMIT })
        : await host.call("sessions.search", { query: trimmed, limit: SEARCH_LIMIT });
    if (current !== sequence) return;
    rows.set(Array.isArray(reply) ? (reply as SessionRow[]) : []);
    failure.set("");
  } catch (error) {
    if (current !== sequence) return;
    failure.set(reason(error));
  } finally {
    if (current === sequence) ready.set(true);
  }
}

function schedule(text: string): void {
  if (timer !== undefined) clearTimeout(timer);
  timer = setTimeout(() => {
    timer = undefined;
    void load(text);
  }, DEBOUNCE_MS);
}

function onChange(text: string): void {
  query.set(text);
  schedule(text);
}

function clear(): void {
  if (timer !== undefined) {
    clearTimeout(timer);
    timer = undefined;
  }
  query.set("");
  void load("");
}

function retry(): void {
  void load(query());
}

async function open(sessionId: string): Promise<void> {
  try {
    await host.call("openSession", { sessionId });
  } catch (error) {
    failure.set(reason(error));
  }
}

/** Enter 打开首条：NSH v1 没有方向键事件，无法逐条移动。 */
function submit(): void {
  const first = rows()[0];
  if (first !== undefined) void open(first.id);
}

function pad(value: number): string {
  return value < 10 ? `0${value}` : `${value}`;
}

/** 相对时间（设计与主应用同一层级，不引入第二套日期库）。 */
function relative(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return "";
  const delta = Date.now() - ms;
  const minute = 60_000;
  const hour = 3_600_000;
  const day = 86_400_000;
  if (delta < minute) return "刚刚";
  if (delta < hour) return `${Math.floor(delta / minute)} 分钟前`;
  const date = new Date(ms);
  if (date.toDateString() === new Date().toDateString()) {
    return `今天 ${pad(date.getHours())}:${pad(date.getMinutes())}`;
  }
  if (delta < 7 * day) return `${Math.floor(delta / day)} 天前`;
  return `${date.getMonth() + 1} 月 ${date.getDate()} 日`;
}

function meta(row: SessionRow): string {
  const parts = [relative(row.updatedAt)];
  if (row.messageCount > 0) parts.push(`${row.messageCount} 条`);
  if (row.cwdLabel !== "") parts.push(row.cwdLabel);
  return parts.filter((part) => part !== "").join(" · ");
}

function Row({ row }: { row: SessionRow }) {
  return (
    <View
      class="gap-0.5 rounded-(--radius) px-2 py-1 hover:bg-accent active:bg-accent"
      role="button"
      onPress={() => void open(row.id)}
    >
      <Text class="text-sm text-foreground" numberOfLines={1} ellipsizeMode="tail">
        {row.title !== "" ? row.title : row.id}
      </Text>
      <Text class="text-xs text-muted-foreground" numberOfLines={1}>
        {meta(row)}
      </Text>
    </View>
  );
}

function Body() {
  // 条件必须在 JSX 里读信号：组件函数体只在首帧执行一次（Solid 式细粒度渲染），
  // 顶部的 if/return 分支不会随信号变化重新求值（pitfall P-021）。
  return (
    <View class="flex-1">
      {failure() !== "" ? (
        <View class="items-center gap-2 px-2 py-6">
          <Text class="text-xs text-destructive" selectable>
            {failure()}
          </Text>
          <View
            class="rounded-(--radius) bg-secondary px-3 py-1 active:bg-secondary/80"
            role="button"
            onPress={retry}
          >
            <Text class="text-xs text-secondary-foreground">重试</Text>
          </View>
        </View>
      ) : ready() ? (
        <List
          class="flex-1"
          each={rows()}
          keyExtractor={(row: SessionRow) => row.id}
          empty={
            <Text class="px-2 py-6 text-center text-xs text-muted-foreground">
              {emptyText()}
            </Text>
          }
        >
          {(item: () => SessionRow) => <Row row={item()} />}
        </List>
      ) : (
        <Text class="px-2 py-6 text-center text-xs text-muted-foreground">载入中…</Text>
      )}
    </View>
  );
}

function App() {
  return (
    <View class="flex-1 gap-1 bg-background p-1">
      <View class="flex-row items-center gap-1">
        <Input
          class="h-8 flex-1 rounded-(--radius) border border-border bg-input px-2 text-sm text-foreground"
          value={query()}
          placeholder="搜索会话"
          autoFocus
          returnKeyType="search"
          submitBehavior="submit"
          onChange={onChange}
          onSubmit={submit}
        />
        {query() !== "" ? (
          <View
            class="h-6 w-6 items-center justify-center rounded-(--radius) active:bg-accent"
            role="button"
            onPress={clear}
          >
            <Text class="text-sm text-muted-foreground">×</Text>
          </View>
        ) : null}
      </View>
      <Body />
    </View>
  );
}

// 会话集合变化（新增/删除）时静默重跑；输入防抖期间不打断。
host.on("sessions.changed", () => {
  if (timer !== undefined) return;
  void load(query());
});

// 宿主看门狗每 2s 发 __ping；nsh 0.3.0 没有内置回应，插件必须显式回 __pong（设计 05 §4）。
host.on("__ping", (data: { t?: number }) => {
  host.post("__pong", { t: data?.t });
});

// 空查询显示最近会话。
void load("");

mount(App);
