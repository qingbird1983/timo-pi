# examples —— subagent 与 prompt 模板的验收夹具

这里的东西**不进 workspace 构建**，只作扩展写法样例与阶段验收夹具（设计见
`../docs/design/phase-3.5/08-subagent.md` 与 `09-prompt-templates.md`）。

## 目录

- `extensions/subagent/source/index.ts`：subagent 扩展本体（TypeScript 源；装机时由
  `timo ext add` 经 rolldown 打成 `bundle.cjs`）。
- `agents/*.md`：代理定义，四份故意把写法铺开——`tools` 一个写逗号串、一个写数组、
  worker 省略以继承默认；`model` 一个给裸 id（scout）、一个省略（reviewer）、
  planner/worker 各指定一个模型。
- `prompts/*.md`：prompt 模板（`/implement`、`/implement-and-review`、`/scout-and-plan`），
  正文用 `$@` 承接参数，靠 chain 把上面的代理串成工作流（09 §5）。

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

## 模板怎么生效

`--prompt-template examples\prompts` 指一次即可；或把 `prompts\*.md` 复制进
`$data\prompts`（用户级）与 `<项目>\.timo\prompts`（项目级），内核自己发现。同名时
**用户级那份先生效**（蓝本语义），后来的只在告警里出现。`--no-prompt-templates` 只关掉
自动发现，`--prompt-template` 显式给的路径仍然加载。

装好后在交互模式里直接打 `/scout-and-plan README 里的安装步骤有没有写错`（只读到计划为止）；
无头模式同样可用：

```powershell
.\target\debug\timo.exe --prompt-template examples\prompts -p "/scout-and-plan examples/agents 里四份定义的 frontmatter 写法各是什么"
```

`/implement` 与 `/implement-and-review` 会派 worker **真改文件**，先把它的工作目录指到沙盒
（`--cwd`，或直接在该目录里启动）再跑。

扩展自带的资源目录（pi 的 `resources_discover`）还没接，所以现在得靠上面两种装法；
接完之后 `examples/extensions/subagent/prompts/` 这类随扩展打包的模板会自动生效
（09 §7）。

## 授权提醒

`ext trust subagent` 会把装机探测到的建议能力一并授予：**exec** 与 **fs.read**（两条作用域：
cwd 相对的 `**` 整树，加上 `<数据目录>/**` 绝对路径——用户级 `agents/`、`prompts/` 不在 cwd 之下，
`**` 够不着，缺这条就会在派子代理时报 `E403`）。读取仍有硬护栏：`.git`、`.ssh`、`.env*`、
私钥类文件一律 `E404` 拒绝，与作用域无关。不想给整树读，就在 `trust` 之后手工把
`$data\extensions\subagent\manifest.json` 里的 `fs.read` 收窄。
