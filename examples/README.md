# examples —— D6 subagent 验收夹具

这里的东西**不进 workspace 构建**，只作扩展写法样例与阶段验收夹具（设计见
`../docs/design/phase-3.5/08-subagent.md`）。

## 目录

- `extensions/subagent/source/index.ts`：subagent 扩展本体（TypeScript 源；装机时由
  `timo ext add` 经 rolldown 打成 `bundle.cjs`）。
- `agents/scout.md` / `agents/reviewer.md`：代理定义。两份故意把 frontmatter 的两种
  写法各用一次——`tools` 一个写逗号串、一个写数组；`model` 一个给裸 id、一个省略以继承默认。

## 装起来跑

```powershell
$data = Join-Path $env:TEMP "timo-subagent"
New-Item -ItemType Directory -Force "$data\agents" | Out-Null
Copy-Item examples\agents\*.md "$data\agents\"
# settings.json / models.json 里配好供应商；api_key 写成 "$DEEPSEEK_API_KEY" 这类引用，别写明文
$env:TIMO_AGENT_DIR = $data
cargo build -p timo-cli
Copy-Item examples\extensions\subagent\source\index.ts "$data\subagent.ts"
.\target\debug\timo.exe ext add "$data\subagent.ts"
.\target\debug\timo.exe ext trust subagent
.\target\debug\timo.exe --ext-flag "bin=$PWD\target\debug\timo.exe" -p "用 subagent 跑 scout，task：'…'"
```

入口文件要复制成一个带名字的文件再 `ext add`：`ext add` 拿**文件名**当扩展 id，
`index.ts` 会装出一个叫 `index` 的扩展。

两个 `--ext-flag` 是传给扩展的运行参数（不是内核开关）：`bin` 指定子进程用哪个可执行
文件（缺省按 PATH 找 `timo`），`timeout` 指定单次子进程超时毫秒（缺省 600000）。我们的
沙箱里没有 `process.env`，所以这两项只能走 flag。

## 授权提醒

`ext trust subagent` 会把装机探测到的建议能力一并授予：**exec** 与 **fs.read 整树**
（代理定义可能落在数据目录，相对 cwd 表达不了）。读取仍有硬护栏：`.git`、`.ssh`、
`.env*`、私钥类文件一律 `E404` 拒绝，与作用域无关。不想给整树读，就在 `trust` 之后手工
把 `$data\extensions\subagent\manifest.json` 里的 `fs.read` 收窄。
