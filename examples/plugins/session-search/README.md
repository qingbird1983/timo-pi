# 会话搜索（自证插件）

设计 07 的 B 轨自证插件：只用 v1 公共 API（`panel` slot、只读查询、`openSession`）
搜索历史会话，不依赖任何内置特权。

## 文件

| 文件 | 作用 |
|---|---|
| `timo-plugin.json` | Timo 清单（schema 2）：`read-only` 挡位、`panel` slot、两条查询、一个动作 |
| `nsh.json` | NSH 应用元数据；`capabilities.host` 放行清单声明的宿主函数 |
| `App.tsx` | 入口组件（NSH 以入口所在目录为应用目录，入口必须在根） |
| `tsconfig.json` | 继承 `.nsh/tsconfig.json`（类型由 `nsh` 生成） |

## 权限

- `queries`：`sessions.list`（空查询显示最近 10 条）、`sessions.search`（关键词，上限 20 条）。
- `actions`：`openSession`（点击行或回车打开首条）。
- 无 `fs`/`net`/`exec`；不读消息正文、不跨工作区。

## 交互

- 输入 ≥1 字符后 200ms 防抖搜索；`sessions.changed` 到达时静默重跑。
- 结果按 `updatedAt` 降序，显示标题、相对时间、消息数、`cwdLabel`。
- 点击行打开会话；回车打开首条；`×` 清空并回到最近会话。
- 入口注册 `__ping` → `__pong`（看门狗，设计 05 §4）；nsh 0.3.0 不会自动回应，且 `__pong` 必须列进
  `nsh.json` 的 `capabilities.host`，缺了会被停用。
- 键盘限制：NSH v1 没有方向键/Esc 事件，无法逐条移动或按 Esc 退出（见设计 07 实现注记）。
- 错误态显示宿主返回的 `E74x` 文案与「重试」；未声明查询会得到 `E740`。

## 打包与安装

```bash
timo-devkit check .                     # 清单 + nsh.json + nsh build 编译校验
nsh keygen /path/to/keys/release        # 首次：生成发布密钥（私钥不入库）
nsh certify --root /path/to/keys/root.secret \
  --key /path/to/keys/release.pub \
  --apps com.timo.session-search --days 365 \
  --out /path/to/keys/release.cert.json # 首次：签发发布证书
timo-devkit pack . \
  --key /path/to/keys/release.secret \
  --cert /path/to/keys/release.cert.json \
  --build 1                             # 写出 plugin.nshpkg 与 manifest.json
```

`pack` 会把源码指纹与包文件名写回 `timo-plugin.json`，并把同一份清单另存为
`manifest.json`（与包构成安装对）。私钥、证书、`.nshpkg`、`manifest.json` 均不入库
（见 `.gitignore`）。
