# P-020 NSH 的 app 目录就是入口文件所在目录

## 现象

P2 按设计（`src/App.tsx` 作入口）搭插件骨架后，devkit 的 `check` 连编译前两步都过不去：

- `nsh build .` → `no App.tsx in .: add one, or pass the entry module`
- `nsh check src/App.tsx` → `src has no tsconfig.json`

把 `tsconfig.json` 留在插件根、`src/` 下只放 `App.tsx` 时，两条路都失败：
传目录则找不到 `App.tsx`，传入口文件则它去入口**旁边**找 tsconfig。

## 根因

NSH CLI（0.3.0）把「入口文件的所在目录」当作 app 目录：

- `nsh build|check|dev|publish <dir>` 只在 `<dir>/App.tsx` 认默认入口；
- `nsh check <file>` 在 `<file>` 所在目录找 `tsconfig.json`，并把生成的 `.nsh/`
  （tsconfig + 类型声明）写在那里。

因此 app 目录只能是「入口 + tsconfig + .nsh」三者同级的那个目录。设计里假设的
`src/App.tsx` + 根级配置这种普通前端布局，在 NSH 下不成立。

## 修复

入口统一放插件根：`App.tsx` 与 `tsconfig.json`、`.nsh/` 同级；`src/` 目录不再存在。
devkit 的 `nsh build|check` 显式传入口文件（`App.tsx`），骨架、示例插件、设计 06/07
同步改口径（06 §2 实现注记）。

## 预防

1. 给插件定目录约定时，先跑一次 `nsh build <dir>` 确认 CLI 认哪个入口，再写进设计。
2. `nsh` 相关命令一律显式传入口文件而不是目录，报错信息会指向真实规则。
3. 别把 `.nsh/`、`*/.nsh/` 留在源码树：它属于生成物，骨架的 `.gitignore` 已覆盖。
