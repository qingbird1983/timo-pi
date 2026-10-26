// NSH 预览面板源码（v1/v2 共用；scripts/build-nsh-poc 只改下面两个常量）。
// 这份文件是 PoC 输入，不进入内核，也不代表插件系统（设计 03 §5）。

import { mount, signal, Text, View } from "nsh";

const VERSION = "1.0.0";
const ACCENT = "#2563eb";

function App() {
  const count = signal(0);
  return (
    <View class="flex-1 items-center justify-center gap-4 bg-background">
      <Text style={{ fontSize: 22, fontWeight: 600, color: ACCENT }}>
        Timo · NSH 预览
      </Text>
      <Text class="text-sm text-muted-foreground">
        已加载签名包 v{VERSION}
      </Text>
      <View
        onPress={() => count.update((c) => c + 1)}
        role="button"
        class="rounded-(--radius) bg-primary px-5 py-3 active:bg-primary/90"
      >
        <Text class="text-lg text-primary-foreground">点击计数：{count()}</Text>
      </View>
      <Text class="text-xs text-muted-foreground">
        这块区域由 NSH AppView 绘制，主界面的对话框仍画在它上方。
      </Text>
    </View>
  );
}

mount(App);
