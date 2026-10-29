# P-018 插件存储根与 NSH 私有数据目录别混用一个字段

## 现象

P1-c 把宿主 API 接到运行时后，`PluginStorage::new(&config.data_dir, id)` 收到的却是
`<data>/plugins/<id>/nsh`（NSH 的 AppOptions 私有目录），于是存储实际落在
`<data>/plugins/<id>/nsh/plugins/<id>/storage.json`——目录套目录，和设计 04 §4 的
`<data>/plugins/<id>/storage.json` 不一致。编译与单测都不报错，因为 `PluginStorage`
自己会在给定根下补 `plugins/<id>/`。

## 根因

一个含糊的字段名 `data_dir` 同时被用来表示两种语义：

- 内核数据根（存储 API 的基准，`PluginStorage` 期望它）；
- 某个实例的 NSH 私有目录（`AppOptions.data_dir`，要的是 `<data>/plugins/<id>/nsh`）。

类型完全相同（`PathBuf`），只有读设计文档才能发现错位。

## 修复

`HostConfig` 明确区分用途：`storage_root`（内核数据根）只给 `PluginStorage`；
NSH 私有目录在装载处单独算出并直接传给 `AppOptions`。`HostConfig` 不再携带
含义不明的 `data_dir`。

## 预防

1. 路径字段按**消费者**命名（`storage_root` / `app_dir` / `sessions_dir`），不要用
   `data_dir` 这种“谁都能塞”的名字。
2. 新接一个吃目录的 API 时，先在设计文档里抄下它期望的完整路径，再写一条断言该
   最终路径的测试（`PluginStorage` 的 `roundtrip` 已有路径断言，可作为模板）。
3. 拆分 impl 到子模块时，跨模块调用的固有方法要显式 `pub(super)`；父模块调用子模块
   的私有方法会得到 E0624，不要靠“同一个 impl”错觉。
