# HANDOFF — nanocodex (Rust 线)

## 2026-09-01：MCP 安全回归、发现容错与测试门禁

- MCP 副作用调用测试已与 compaction recovery 守卫解耦：在
  `approval_policy=never` 且未开启 compaction recovery 时，副作用调用必须
  命中明确的 `denied by approval policy 'never'` 拒绝文案；只读 LLM Wiki
  查询仍可执行。
- Codex 兼容插件发现按 server 隔离错误：非法 command/arg 只跳过当前
  server，不再让整个插件目录或会话组装失败。裸 MCP 参数不再因为插件根下
  存在同名文件而被改写；显式相对路径才解析到插件资源根。
- Windows Hook 测试超时调整为 20 秒；只读并发回归改用 in-flight 峰值断言；
  Agent Loop、MCP、Sandbox、Tool Scheduler 测试临时目录加入进程 ID。
- 修复本次触及的 Rust Clippy 与 Python lint 问题，并将可靠性规则、验证命令
  和 Tauri 本地启动方式同步写入 `README.md` 与 `README.zh-CN.md`。
- 当前验证证据：`cargo fmt --all -- --check`、
  `cargo clippy --workspace --all-targets -- -D warnings`、
  `cargo test --workspace`（全量通过）以及 `python -m pytest -q`
  （601 passed）。未提交、未推送状态需以当前 Git 状态重新核对。

## 2026-09-02：GUI 工作区改动面板与异步状态隔离

- 修复右侧“工作区改动”面板的条目高度与双重滚动冲突：文件行现在有稳定的最小高度和行高，面板主体是唯一滚动容器；大量变更文件不会再相互遮挡、裁切文字或形成嵌套滚动区。
- 前端 Runtime 显式保存并释放 Tauri 事件监听器；应用卸载或启动过程被中断后，过期异步任务不再写入 UI 状态。
- 文件浏览、Git 改动/分支详情、检查点、项目记忆及 Forge 状态观察器在工作区切换后使用 generation 拒绝旧请求结果，并清理旧工作区投影；Forge 后台进程不被切换取消，会自动刷新新项目投影，避免慢请求把前一项目的数据带入当前面板或让控件停在未初始化状态。启动期会先从 `runtimeStatusRead` 写入工作区，且 Ready 把空工作区到真实路径视为投影变更，因此 Forge 首读不会因 Ready 到达较晚而停在未初始化状态。
- 只有工作区确实改变时，才会在修改进程 CWD 前取消模型记忆整理；取消与草稿提交共用互斥围栏，因此旧工作区的准备结果不能在切换后写入，而同项目内 Resume/Fork 不会中断整理。空 Thread 的连续 Harness Profile 选择按 set/activate 整段串行，最后选择才会持久化；Profile 写入与首个 Turn 在 Thread Store 内原子互斥，已在飞的旧选择也不能越过首轮锁定。
- Memory Merge 与 Forge 的状态/取消请求现在同样带工作区快照；Coordinator 保存任务 owner，非 owner 工作区只能看到 idle，取消还须匹配精确 generation，因此跨工作区或同一工作区的新任务都不会被延迟旧取消误伤。
- Resume、Fork、新建和权限模式重建显式使用持久 Thread 的 workspace；App Server 等待宿主切换完成后才返回，worker 不再延迟读写进程级 CWD，因此跨项目恢复不会把前一个工作区带进新会话。
- 当前验证：`npm.cmd run build`、`cargo check --manifest-path rust\\gui\\src-tauri\\Cargo.toml`、`cargo test --manifest-path rust\\gui\\src-tauri\\Cargo.toml --lib`（129 passed）、`cargo test --manifest-path rust\\Cargo.toml -p ncx-protocol --lib`（12 passed）、`cargo test --manifest-path rust\\Cargo.toml -p ncx-app-server --lib`（31 passed）、`cargo test --manifest-path rust\\Cargo.toml -p ncx-thread-store --lib`（18 passed）和 `cargo fmt --manifest-path rust\\Cargo.toml --all -- --check` 均通过；真实 Tauri 开发窗口已用含 30+ 改动条目的工作区验证列表显示与滚动。

## Same-session durable Goal：领域与原子存储（2026-08-27）

- 参考 DeepSeek Harness 固定提交的 `goal`、`tool-goal` 和
  `goal-round-driver`，已在 `docs/deepseek-harness-adoption.md` 固定持久状态、
  process-local activation、authority、自动续轮竞争/取消/费用边界和交付顺序。
- `ncx-protocol` 新增 `GoalId`、`GoalPhase`、`GoalBlockReason`、
  `GoalSnapshot`、`GoalRef`，以及 read/create/edit/pause/resume/block/complete/
  clear 请求、Goal 响应和 `GoalChanged` 事件。持久快照不包含 armed 权限。
- `JsonThreadStore` 增加独立 `goals` 持久域和真正原子的
  `compare_and_set_goal`：比较、写入发生在同一个进程 Mutex、跨进程文件锁、
  最新磁盘 reload 与原子 save 事务内。旧 `threads-v2.json` 缺少 `goals` 时
  默认空；Fork 在同一事务复制 durable snapshot。
- App Server 已拥有 Goal 领域状态机：目标/轮数校验、单调 revision、合法
  phase transition、只有 complete 可被新 Goal 替换；clear 作为显式取消入口。
  stale revision 和非法 transition 都保持零 Goal 写入。
- 当前验证：Protocol 7/7、Thread Store 15/15、App Server 19/19。覆盖旧文件、
  reopen、Fork、stale CAS 字节不变、生命周期与替换规则。尚未接模型工具、
  process-local arm/disarm、三轮 blocked 门禁、自动续轮和 GUI，不能默认启动
  任何可能产生费用的 continuation。

### 后续：process-local activation 与模型工具（2026-08-27）

- 协议增加 `GoalActivation` / `GoalView`，App Server 用进程内集合持有 armed
  权限；Create 默认 disarmed，pause/block/complete/clear 自动 disarm，显式
  resume 才 arm。活动 Goal 在 restart 后恢复为 disarmed，Fork 只复制 durable
  snapshot 且目标 Thread disarmed；相关 App Server 测试已覆盖。
- `ncx-core::goal_tools` 新增 `get_goal`、`create_goal`、`update_goal`。工具只在
  Host 注入 thread-bound `GoalToolService` 时注册；GUI adapter 的所有 mutation
  都回到 App Server 状态机，不直接操作 Thread Store。
- `AgentLoop` 为每轮设置 host-attested `GoalTurnAuthority`：普通 `run_turn` 是
  direct-human；保留给驱动的 `run_goal_round` 携带 goal ID/revision/round；结束后
  清除。无 authority、旧 turn、Goal round 冒充 human、错误 ID/revision/round
  均在访问服务前或 mutation 前 fail-closed。
- edit/pause/resume 只允许 direct-human；complete/blocked 允许 direct-human 或
  精确当前 admitted Goal round。模型自动上报 blocked 至少需要 3 个 admitted
  rounds，并必须提供具体原因。
- Goal 工具加入 always-visible schema；GUI system prompt 写入 exact ref、恢复后
  disarmed 和 blocked 下限规则。Orchestrator worker 不注入 Goal 服务，因此不会
  继承顶层人类权限。
- 新验证：Core 全量 260/260，GUI Tauri 后端 102/102，GUI 独立目标
  `cargo check` 通过；其中新增 authority、stale identity、blocked 下限及 GUI
  adapter 测试。下一步是把 admitted round 与 Turn/UserMessage 在 Store 内原子
  落库，再实现 checkpoint、竞争用户输入、取消和 queue failure 围栏。

### 后续：admitted Goal round 原子事务（2026-08-27）

- 协议新增隐藏的 `ThreadItem::GoalMessage` 与 `GoalRoundStart`。GoalMessage
  保留 goal ID/revision/round，进入模型恢复上下文但不进入可见历史，避免用户
  会话里出现伪造的自动“用户消息”。
- `ThreadStore::claim_goal_round` 在同一个进程 Mutex、跨进程文件锁、最新磁盘
  reload 和原子 save 内完成：精确 Goal CAS 身份、active phase、连续 round、
  round limit、Turn 唯一性/占用、Turn lease、匹配 synthetic prompt、Turn 写入和
  `roundsStarted` 增加。任何校验失败时 Turn 与 round counter 都不变。
- App Server 在持有 process-local activation lock 时调用该事务，disarmed Goal
  无法 admission；成功后同时发布 `TurnStarted` 与带最新计数的 `GoalChanged`。
  普通 Turn 与 Goal round 仍共享同一 Thread lease，无法重叠执行。
- 验证更新：Protocol 7/7、Thread Store 17/17、App Server 20/20；新增 GUI
  replay/visible-history 定向测试通过。尚未把 scheduler 接到真实 GUI 自动调用，
  因此当前不会因为这项改造自行产生模型费用。下一步继续 checkpoint 后重检、
  ordinary-user-wins 队列、取消/queue failure/round-limit fail-closed 与 driver dispose。

### 后续：GoalRoundDriver checkpoint/race 围栏（2026-08-27）

- `ncx-app-server::GoalRoundDriver` 已抽成不调用模型的 scheduler coordinator。
  `reserve_next` 先读取 exact armed Goal，再执行 Host checkpoint，之后重新读取
  完整 `GoalView`；任意 revision/phase/activation/round 变化均放弃旧 reservation。
- checkpoint 失败会立即 disarm 且不 admission。checkpoint 期间普通用户 Turn
  若先取得 Thread lease，返回 `CompetingTurn`，Goal counter 保持不变；不会让
  自动任务抢在用户输入前面。
- round limit 在 admission 前转为结构化 `round-limit` blocked 并 disarm。
  已 reservation 的取消会完成 Turn 为 cancelled、pause 并 disarm；queue failure
  会以固定安全错误完成 Turn，再写结构化 `queue-failed` blocker，原始异常不进入
  Thread/UI。
- App Server 验证更新为 26/26，覆盖 checkpoint failure、checkpoint mutation、
  ordinary-user-wins、cancel、round limit 和 queue failure。该 coordinator 尚未接
  GUI 的真实模型 worker，因此仍不会触发付费自动调用。下一步把它接到每会话
  串行 task lifecycle，并增加 driver dispose/进程关闭等待与 GUI 显式控制。

### 后续：GUI Goal 状态与费用确认（2026-08-27）

- 新增 `GoalController` 和前端 `ProtocolGoalView` 类型。当前会话切换、首次绑定、
  Turn 从 busy 回到 idle 后都会回读 App Server；旧异步响应通过 generation +
  thread ID 双重检查丢弃，避免串会话状态。
- Composer 只在当前 Thread 存在 Goal 时显示状态胶囊，展示 phase、
  `roundsStarted/maxGoalRounds`、armed/disarmed、剩余轮数和 blocker。用户可显式
  pause；resume 前必须确认“使用当前模型商、可能产生模型费用”和剩余上限。
- pause/resume 请求携带 exact Goal ID/revision，不做乐观切换；失败后重新读取
  服务端状态。切换会话、恢复和 Fork 后 process-local activation 的真实 disarmed
  状态会直接显示。
- Vite 生产构建通过（147 modules）；GUI Rust 源码门禁验证可见状态、费用确认、
  exact ref 和失败回读通过。真实自动 Worker 仍未启用，当前 UI 的“继续”只设置
  App Server activation，不会自行发起模型调用。

## Provider Catalog Harness 服务化（2026-08-27）

- 新增 `ncx-provider::model_catalog`：统一 `/models` 端点构造、OpenAI Bearer 鉴权、Anthropic `x-api-key`/版本头、15 秒默认超时、禁止重定向、1 MiB 响应上限、1000 模型上限和安全 ID 校验。
- 目录解析统一兼容 OpenAI/Anthropic 常见 `data`、`models` 和根数组格式；OpenRouter 的名称、上下文长度、输入/输出价格也归一为中立模型元数据。
- `ncx-core::ProviderCatalogService` 是可注入客户端的只读服务；Harness `base` bundle 新增 `ncx.provider-catalog`，依赖 `provider.directory` 并发布 `provider.catalog`。
- GUI 原有三套重复 HTTP 路径（自定义模型商发现、设置页自动发现、云末刷新）和 OpenRouter 刷新已迁入该服务；异步刷新通过 blocking worker 执行，不阻塞 Tauri async runtime。
- 发现操作不写 `config.toml`/`providers.json`；服务测试对失败前后两个文件做逐字节比较，确认目录请求失败不会回滚或改变当前 Provider Route。
- 安全测试验证 OpenAI/Anthropic 请求头、禁止错误正文与 Token 回显；真实接口抽样：OpenRouter 返回 417 个模型，云末返回 13 个模型且包含 `gpt-5.6-sol`（只输出数量/布尔值，未输出 Token）。
- 验证：Provider 41、Core 239、Config 36、Protocol 5、App Server 12、独立目标目录 GUI Rust 86 项通过；CLI check 与 Vite build 通过。GUI 默认 target 曾因开发服务器与测试并发产生 MSVC 旧增量对象链接错误，独立目标目录从零编译、链接和测试通过，证明不是源码失败。
- 本地开发版已改用干净的 `rust/target-codex-check/gui-catalog` 目标目录启动，当前 GUI PID `3912`（仅为交接快照）；默认 `src-tauri/target` 未删除，避免破坏用户构建缓存。
- 后续进展：Provider 激活已改为候选目录验证后提交，Prompt 每轮读取新 Route，不再重建整套 Harness；并发 generation、失败保留旧 Route、最近激活诊断和真实 WebView 会话保留 E2E 已完成，详见文末 2026-08-27 记录。

## Provider Directory Host/Harness 服务化（2026-08-27）

- `ncx-config::ProviderDirectory` 已抽取为 Provider Route 的持久化唯一所有者；协议、Base URL、Token、模型目录和当前模型作为一条 Route 整体保存、校验和激活。
- `ncx-core::ProviderDirectoryService` 统一封装 list/get/save/delete/activate/select/diagnostics；GUI 不再在各命令中自行构造或读写 `providers.json`。
- Harness `base` bundle 新增 `ncx.provider-directory`，向所有内置 Profile 发布 `provider.directory` 服务；Harness 诊断增加 `provider_directory` 挂载状态。
- GUI `AppState` 在进程启动时持有单一 `ProviderDirectoryService`，App Server Adapter 的模型商保存、删除、发现、激活，以及顶部模型切换和目录预设切回均消费该服务。
- 设置 → 插件 → Harness 运行诊断现在显示当前 Provider ID、协议、请求模型、Base URL、Token 是否已配置和自定义模型商数量；诊断结构不会序列化 Token。
- 新增服务级测试，真实写入隔离配置、激活 Route，并验证诊断不泄露凭据。当前验证：`ncx-config` 36、`ncx-provider` 37、`ncx-protocol` 5、`ncx-app-server` 12、`ncx-core` 238、GUI Rust 88 项通过；CLI check 与 Vite build 通过。
- 后续进展：动态发现已迁入可替换 Provider Catalog；Provider 切换采用候选验证、完整 Route 提交和 per-turn runtime 读取，并记录脱敏激活状态。该条早期“仍需继续”已完成。

## Provider Route 原子化（2026-08-26）

- `config.toml` 新增 `active_provider_id`；`legacy` 表示内置/云末等目录预设，自定义模型商使用稳定 Provider ID。
- `ncx-config` 在激活自定义模型商时从同一条 `providers.json` 记录整体解析协议、Base URL、Token、选中模型和模型列表；记录缺失或不完整时明确失败，不再回退拼接旧配置。
- 自定义模型商激活会保存完整路由；顶部模型切换同步其 `selected_model`；切回目录预设会写回 `legacy` 并清除旧 `active` 标记。
- 同名模型不再按目录顺序猜模型商；快捷切换必须匹配当前 Base URL，避免 `gpt-5.6-sol` 从云末误切到其他 Provider。
- 验证：`ncx-config` 33 项通过，GUI Rust 87 项通过，Tauri `cargo check` 通过，Vite 生产构建通过。本地开发版已重启，当前 GUI PID `36572`（PID 仅为交接时快照）。
- 自定义模型商的 list/save/delete/discover/activate 已从直接 Tauri invoke 迁入 `ncx-protocol` + `ncx-app-server::AppServerAdapter`；Token 只作为保存请求输入，不进入响应或测试调用记录。
- 迁移后验证：`ncx-app-server` 12 项、`ncx-protocol` 5 项、GUI Rust 88 项通过，Vite 生产构建通过；热更新后的 GUI PID `32516`（PID 仅为交接时快照）。
- Agent `AssistantText` 事件现在携带发起该条回答时的 Provider 模型 ID；前端将其保留在回答对象并显示“请求模型 · …”标签。运行中切模型时旧回答仍标旧模型，新回答标新模型，不再用模型正文自报身份判断版本。
- 模型归属改造后 `ncx-core` 237 项、GUI Rust 88 项及 Vite 构建通过；本地热更新 GUI PID `13004`（PID 仅为交接时快照）。当前标签是“请求模型”而非“服务端响应模型”；若要声称服务端确认，后续需扩展 `ModelResponse` 解析 OpenAI/Anthropic 返回的 `model` 字段并持久化到 ThreadItem。
- 上述后续已完成：OpenAI Compatible 流式/非流式与 Anthropic Messages 都会捕获响应 JSON 的 `model`；Provider 每次请求前清空旧确认值，避免错误沿用上一轮。`ThreadItem::AssistantMessage` 持久化请求型号和服务端确认型号，历史恢复后标签仍存在，内部元数据不会传回第三方模型 API。
- UI 展示规则：响应与请求一致显示“响应确认”；服务端未返回 `model` 只显示“请求模型”；两者不一致显示橙色“请求 A → 响应 B”警告。验证：Provider 37、Protocol 5、App Server 12、Core 237、GUI 88 项通过，CLI check 与 Vite 构建通过；完整重启后的 GUI PID `22232`（PID 仅为交接时快照）。

## 组件化工作树（2026-08-21）

- 独立目录：`D:\github_dgy\nanocodex\.worktrees\deepseek-harness-components`
- 分支：`feat/deepseek-harness-components`，基线 `b867466`；组件版后续只在此目录开发。
- 新增 Harness 插件清单：稳定 ID、名称、版本、能力类型、依赖和默认启用状态。
- 新增 `HarnessProfile`：`Full`、`Coding`、`ReadOnly`、`Minimal`，运行时按能力选择组件；真正的只读限制仍由 sandbox/permission policy 执行。
- CLI、CLI orchestrator worker 和 GUI 后端已改为显式通过 `HarnessRuntimeBuilder` 装配；`ToolRegistry::new()` 仅保留兼容入口。
- 验证：上一阶段 `cargo test -p ncx-core --lib` 204 项通过；`cargo check -p ncx-cli` 通过。

### 官方 DeepSeek Harness 对照

- 官方源码：`D:\deepseek-harness-master\deepseek-harness-master`，上游 `deepseek-ai/deepseek-harness`，MIT。
- 官方“一切皆插件”不是工具分组：Cordis Context 是共享服务容器；插件用 `inject` 声明服务依赖；服务就绪后挂载，依赖变化后卸载/重挂；所有注册是可逆 effect，并按逆序释放。
- 官方 Profile 是 bundle + `cordis.patch.yml` 的分层组合，不是代码里的固定模式枚举。模型、工具注册表、会话日志、agent loop、压缩、Skills、MCP 均是可替换插件。
- nanocodex 已开始纠偏：`HarnessPlugin` 增加服务依赖和失败返回；`PluginHost` 增加类型化服务发布/读取和可逆 effect；注册器按服务依赖激活插件；运行时销毁时逆序执行 disposer。工具插件现在只是插件 Consumer 的一种。
- 纠偏后验证：`cargo test -p ncx-core --lib` 205 项通过；`cargo check -p ncx-cli` 与 GUI Tauri 后端均通过。
- 后续必须继续拆除固定 `HarnessProfile` 枚举，改成可加载的 bundle/overlay；再把 Provider、Session、AgentLoop、Memory、Skills、MCP、Compaction 逐个拆成 Service Definition / Provider / Consumer。

### 目标架构方案：对齐 DeepSeek Harness 的“一切皆插件”

#### 1. 总体原则

- Rust 实现不照搬 Cordis TypeScript 代码，但必须保持相同架构语义：共享 Context、服务依赖注入、按依赖激活、可逆 effect、作用域隔离、配置化组合。
- Runtime 只负责加载配置、创建 Context、挂载插件、调度生命周期和输出诊断，不直接拥有模型、工具、会话、记忆或压缩业务。
- 每项能力拆成三个角色：Service Definition 定义稳定接口；Provider 提供实现；Consumer 把能力接入 Agent、工具或 UI。禁止把 Provider 私有细节泄漏到公共接口。
- 新行为优先通过插件、服务、事件或中间件扩展；只有插件扩展点本身不足时才修改 AgentLoop 核心。
- 所有模型可见内容必须可以从会话事件重建；工具、上下文注入、压缩摘要、计划和最终结果不能只存在于临时内存。

#### 2. 运行时分层

```text
CLI / GUI / Headless
        │
        ▼
Profile Loader ── Bundle / Overlay / 用户配置
        │
        ▼
Harness Runtime
├── Context：作用域化服务容器
├── Plugin Registry：发现、校验、依赖解析
├── Lifecycle：mount / ready / reload / dispose
├── Effects：注册、诊断、逆序释放
└── Events：类型化事件与可拦截流水线
        │
        ▼
Service Definition ← Provider Plugins ← Consumer Plugins
```

#### 3. 插件契约

- 每个插件声明稳定 ID、名称、版本、配置 schema、提供的服务、注入的必需/可选服务和兼容版本。
- `inject` 面向服务名，不面向插件 ID；运行顺序由服务依赖决定，不能靠手写注册顺序。
- 必需服务未满足时插件保持 Pending；服务就绪后挂载。服务消失或实现被替换时，依赖插件先卸载，再按新依赖重挂。
- 插件安装失败必须返回明确错误并回滚本次已注册 effect；不得半安装继续运行。
- 每个工具、事件监听器、中间件、后台任务、进程、文件监听器和服务注册都必须返回 disposer，并随所属插件逆序释放。
- Runtime dispose 必须等待后台任务和子进程真正停稳，不能只发送取消信号就返回。

#### 4. Context 与作用域

- Root Context 保存进程级能力和插件清单；Workspace Context 保存项目目录、沙箱和项目配置；Session Context 保存会话、费用、计划和取消状态；Turn Context 保存单轮请求与临时注入。
- 子 Context 可继承父服务，但允许对指定服务做 isolate/intercept；新会话共享项目文件，不继承其他会话聊天内容、未完成计划或取消状态。
- 所有 GUI 事件、工具事件和流式响应绑定 `session_id + turn_id`；前端拒绝旧会话或旧轮次事件，但后台会话继续独立运行。
- Provider、Memory、Skills 等可按 Workspace 或 Session 覆盖，禁止使用进程级可变单例导致串会话。

#### 5. Profile、Bundle 与 Overlay

- 删除代码内固定的 `HarnessProfile` 枚举，改为配置文件驱动的 Profile。
- Profile 只声明需要叠加的 Bundle；Bundle 是一组带稳定 entry ID 的插件配置，不包含运行时硬编码分支。
- 建议内置 Profile：`full`、`coding`、`readonly`、`minimal`、`gui`、`headless`；它们与用户自定义 Profile 使用同一加载路径。
- 配置叠加顺序：基础 Bundle → Profile Bundle → 工作区 Overlay → 用户 Overlay → CLI 临时 Overlay。
- Overlay 按稳定 entry ID 启用、禁用、替换 Provider 或覆盖配置；未知 ID、重复服务 Provider、依赖环和无效配置必须启动失败并给出中文诊断。
- 权限是独立 Policy 服务，不依赖“某个 Profile 名称”。`readonly` 通过替换/配置权限 Provider 实现，而不是仅隐藏写工具。

#### 6. 计划拆分的能力插件

1. `service.tools`：工具注册、schema、执行和展示意图；各工具包作为 Consumer 注册工具。
2. `service.llm`：文本/视觉 Provider 接口、模型能力、流式响应、reasoning 和 usage；DeepSeek、百炼等作为 Provider 插件。
3. `service.session`：事件日志、快照、索引、恢复、归档、标题和费用投影；持久化实现独立 Provider。
4. `service.agent`：消息队列、并发会话、取消、steering 和状态；不能绑定某个 GUI。
5. `service.agent-loop`：只编排模型请求、工具执行和继续/结束，不直接实现 Memory、Skills、压缩或 PDF 规则。
6. `service.context`：项目说明、附件、文件引用、长期记忆和单轮上下文注入，以 Consumer 方式进入模型请求。
7. `service.compaction`：触发阈值、摘要生成、工具结果裁剪、持久化和恢复；压缩策略可替换。
8. `service.memory`：检索、写入、合并和项目隔离；本地 MemoryStore 是一个 Provider。
9. `service.skills`：发现、目录、加载和执行；内置、用户级、工作区级技能作为不同 Provider/Overlay。
10. `service.mcp`：连接生命周期、工具同步、失败恢复和权限；MCP 仅是外部能力 Provider，不直接修改 AgentLoop。
11. `service.interaction`：审批、询问用户、权限策略和 session grants；CLI 与 GUI 提供不同交互 Provider。
12. `service.media`：附件解析、视觉理解、生图和视频；阿里百炼技能/接口作为 Provider，并由独立 Cost 服务提供价格估算。
13. `service.telemetry`：token、费用、耗时和错误事件；UI 只消费投影结果，不自行累计核心数据。

#### 7. 配置与安全边界

- 插件配置先反序列化和校验，验证通过后才能 mount；部署可变参数不能散落为源码常量。
- API Key 只从凭据服务读取，插件配置保存引用或 provider ID，不保存明文；日志、诊断、session 和 handoff 一律脱敏。
- 文件、Shell、网络和进程权限统一经过 Policy/Sandbox 服务执行，隐藏工具或 prompt 提示不能代替真正的执行拒绝。
- 外部动态插件默认不可信：第一阶段只支持编译时内置 Rust 插件；后续动态插件需独立进程/WASM 边界、协议版本、签名/来源和资源限制，不直接加载任意 DLL。

#### 8. 迁移顺序

- M0（已完成地基）：插件 Manifest、服务发布/读取、`inject`、按依赖激活、失败返回、effect 逆序释放、CLI/GUI 显式 Runtime 装配。
- M1（已完成）：实现可加载 Profile/Bundle/Overlay、结构校验、缺失服务诊断；删除固定 Profile 枚举。插件专属配置 schema 随 M2 的 Service Definition/Provider 契约实现。
- M2：拆 `Tools`、`LLM Provider`、`Interaction/Policy` 三条最小完整能力链，证明 Definition/Provider/Consumer 可替换。
- M3：拆 Session 与 Agent 生命周期，完成 Workspace/Session/Turn 作用域，保证多会话并发和事件不串线。
- M4：拆 Context、Memory、Skills、Compaction，让长会话记忆和自动压缩由插件组合完成。
- M5：拆 MCP、附件、图片、视频与 Cost/Telemetry，并接入 GUI 插件设置和运行诊断。
- M6：增加外部插件发现、安装、启停、升级和隔离；完成 `full`、`minimal`、`headless` 的真实组合测试。

#### 9. 兼容与回滚

- 迁移期间 `ToolRegistry::new()` 保留为兼容门面，但内部必须委托默认 Bundle；生产入口不得新增对该门面的依赖。
- 每完成一条能力链后再删除旧直连路径，禁止同时维护两套状态源。
- Session 日志和配置格式变化必须有版本号、显式迁移或清晰拒绝；不得静默按新格式误读旧数据。
- 每个里程碑保持可独立回滚的提交；不混入 UI 美化、模型目录或其他无关改动。

#### 10. 验证门禁

- 单元测试：依赖等待、缺失依赖、依赖环、重复服务、安装失败回滚、effect 逆序释放、重复 dispose。
- 组合测试：从真实 Profile 文件启动 Runtime，验证 Provider 替换、Overlay 禁用和错误配置失败。
- 生命周期测试：服务替换触发依赖插件卸载/重挂；取消和 dispose 后无后台任务、子进程或事件泄漏。
- 会话测试：多个会话并发执行、切换 UI 不停止后台任务、事件按 `session_id + turn_id` 隔离。
- 回归测试：每个阶段至少运行 `cargo test -p ncx-core --lib`、`cargo check -p ncx-cli`、GUI Tauri 后端检查；涉及前端时再运行 Vite 构建和关键交互测试。
- 交付标准：相关测试有证据、`git diff --check` 通过、`rust/Cargo.lock` 无无关变化、HANDOFF 更新、工作树只包含本阶段文件。

### M1 完成记录：配置驱动的 Profile / Bundle / Overlay（2026-08-21）

- 新增 `rust/harness/` 配置源：内置 `full`、`coding`、`readonly`、`minimal` Profile，以及 `base`、`search`、`workspace`、`process`、`session` Bundle。
- 新增 `plugins/composition.rs`：从 TOML 加载 Profile 和 Bundle，按 Bundle 顺序组合稳定 entry；Overlay 按 entry ID 覆盖 plugin、enabled 或完整 config。
- Profile/Bundle 名称不一致、重复 entry、未知 Overlay entry、未知插件、无效字段和缺失服务依赖均明确失败，不静默回退。
- 插件配置随注册记录保存并传入 `HarnessPlugin::install`，Overlay 修改后的 config 能到达实际插件安装阶段。
- 删除 `HarnessProfile` 固定枚举和 `for_profile` 分支；默认 Runtime 也从内嵌 TOML `full` Profile 构建。
- CLI 主运行、CLI orchestrator worker 和 GUI Agent 构建均调用 `HarnessRuntimeBuilder::configured(workspace)`。
- 外部选择：`NANOCODEX_HARNESS_PROFILE` 选择 Profile；`NANOCODEX_HARNESS_ROOT` 指向外部根目录；工作区 `.ncx/harness/` 可提供 Profile/Bundle；`.ncx/harness.overlay.toml` 和 `NANOCODEX_HARNESS_OVERLAYS` 叠加 Overlay。
- 动态下载或加载任意 DLL 不属于 M1；外部配置只能组合当前二进制编译进来的 Rust 插件，避免在没有隔离与签名机制前扩大信任边界。
- 验证：`cargo test -p ncx-core --lib` **211 通过，0 失败**；`cargo check -p ncx-cli` 通过；GUI Tauri 后端 `cargo check` 通过；`rg "HarnessProfile|for_profile" rust` 无匹配。

### M2–M4 当前推进记录（2026-08-23）

- 已把 LLM、Interaction、Policy、Context、Memory、Compaction 纳入可组合的插件清单和内置 Bundle；这些能力现在有独立 Manifest、稳定 entry ID、服务名和可替换挂载点。
- `ncx.llm`、`ncx.interaction`、`ncx.policy`、`ncx.context`、`ncx.memory`、`ncx.compaction` 已通过 `PluginHost::provide()` 发布服务，默认 `full/coding/readonly/minimal` Profile 均可按配置启停。
- 这一步完成的是 M2–M4 的运行时装配地基，不等同于所有业务实现已经从 AgentLoop 拆出：LLM 的真实 Provider 请求、Interaction 的审批实现、Session/Context/Memory/Compaction 的核心状态仍在迁移中，后续必须把现有直连逻辑改为读取这些服务，补充真实组合测试后才能宣布 M4 完成。
- 当前验证：核心插件相关测试 13 项通过；全 `ncx-core` 回归此前 211 项通过；CLI 和 GUI Tauri 后端检查通过。此次修改尚未提交或推送，下一步先完成服务 Consumer 接入，再提交 M2–M4 阶段提交。

> 新接手的 agent：先读完再动手。与上一级 `D:\agent_prac\HANDOFF.md`（面试准备）是两条独立线。
> Python 时代历史在 git 历史 + SESSION_MEMORY.md。

## 当前进度（2026-08-18）

### 当前工作线

- 工作树：`D:\github_dgy\nanocodex\.worktrees\model-provider-catalog`
- 分支：`feat/model-provider-catalog`
- 最新提交：`19e296b fix(agent): preserve long task deliverables`
- 本轮用户原有文件必须保留：`rust/Cargo.lock`（已修改）、`parse_xlsx.py`（未跟踪）。两者均未纳入本轮提交，也不得覆盖或清理。
- 主工作树 `D:\github_dgy\nanocodex` 另有用户未提交的 GUI 修改；继续开发应留在上述独立工作树，避免混入主工作树改动。

### 本轮问题与根因

- 相同标题的连续会话实际落在不同 session；程序启动时总建新会话，导致新会话无法继承旧会话的 PDF 任务状态。
- 长链路上下文裁剪只保留近期消息，较早的用户目标会被工具日志挤掉，模型因此持续研究或回答已有 PDF，而不是完成用户要求的新 PDF。
- Agent 以前没有交付物完成闸门：用户明确要求生成 PDF 时，即使没有创建或更新 PDF，也能用普通文本结束任务。
- 强制收敛逻辑可能过早移除工具，导致尚未完成的计划或 PDF 任务无法继续执行。

### 已完成修复

- `ncx-core/src/session_index.rs`：按规范化工作目录查找最近一个未归档、且存在快照的可恢复会话。
- `gui/src-tauri/src/bridge.rs`：启动时恢复当前工作目录最近会话；监听器就绪后发送 `Loaded` 事件，把恢复的历史同步到前端。
- `ncx-core/src/session.rs`：裁剪长上下文时，额外保留最多 8 条历史用户消息作为“任务历史锚点”，丢弃旧的助手和工具噪声。
- 新增 `ncx-core/src/agent_loop/deliverable.rs`：识别明确的 PDF 创建请求，记录执行前 PDF 快照，并检查本轮是否创建或更新了有效 PDF。
- PDF 有效性至少检查 `%PDF-` 文件头和尾部 `%%EOF`；只写文件头、沿用旧 PDF 或只回复路径均不能算完成。
- `ncx-core/src/agent_loop/turn.rs`：PDF 未交付时禁止文本提前结束，并明确要求模型停止继续研究、实际生成文件；未完成计划或交付物时，长链路收敛不能撤掉工具。
- PDF 只读、查找或询问类请求不会误触发“必须生成 PDF”的闸门。

### 验证证据

- `cargo test -p ncx-core`：**191 通过，0 失败**。
- `cargo test -p ncx-gui --manifest-path gui/src-tauri/Cargo.toml`：**25 通过，0 失败**。
- `npm run build`：成功，Vite 共处理 114 个模块。
- Windows GNU 正式版构建成功：
  - 程序：`rust\gui\src-tauri\target\x86_64-pc-windows-gnu\release\ncx-gui.exe`
  - 安装包：`rust\gui\src-tauri\target\x86_64-pc-windows-gnu\release\bundle\nsis\nanocodex_0.1.0_x64-setup.exe`
- 最新正式版已在本地启动并验证窗口响应正常；启动时进程号为 `28924`（进程号仅是当次运行状态，不应作为后续判断依据）。
- `git diff --check` 无格式错误，仅有 Windows 换行提示。

### 后续接手建议

1. 用现有长会话继续追问，确认恢复的是同一个 session，且较早的用户目标仍进入模型上下文。
2. 实测一句明确的“根据以上资料生成一个 PDF”，确认只有新建或更新有效 PDF 后任务才结束。
3. 实测“PDF 去哪了”“查找已有 PDF”等只读请求，确认不会被创建闸门拦截。
4. 若继续修改，先确认 `git status --short`，不要提交或还原用户的 `rust/Cargo.lock` 与 `parse_xlsx.py`。

### 追加修复：模型连接中断恢复（2026-08-18）

- 复现现象：DeepSeek 流式请求在底层重试耗尽后返回 `RequestError: error sending request`，Agent 过去会把英文底层错误直接当最终回答并结束当前问题。
- 当前网络与 `api.deepseek.com:443` 已实测可达，接口返回 401（未携带鉴权的预期响应），因此截图属于短时发送链路失败，不是域名、端口或 Base URL 配置错误。
- `ncx-core/src/agent_loop/turn.rs` 现在对响应到达前的 `RequestError`/`TimeoutError` 做本轮级恢复：保留同一条用户问题并继续请求，不要求用户重复描述。
- 连续 3 次本轮级恢复仍失败时才结束，并显示中文可恢复提示；底层英文 `RequestError` 不再作为正常助手回答写入会话。
- 新增 2 个回归测试，覆盖“首个请求失败、随后成功”和“连续失败后中文结束且不无限重试”。
- 验证：`ncx-core` **193 通过**；GUI 后端 **25 通过**；Vite 正式构建成功（114 模块）；GNU 正式版和 NSIS 安装包重新生成成功。
- 最新正式版已启动并确认窗口响应正常；当次进程号为 `38580`。

### 全局技能：阿里百炼文生图（2026-08-18）

- 技能位置：`C:\Users\25376\.ncx\skills\aliyun-image-generation`，属于 nanocodex 全局技能，打开任意工作区都可发现。
- 技能名：`aliyun-image-generation`；触发场景包括画图、文生图、生成图片、海报、插画、产品图和视觉素材。
- 执行脚本：`scripts/generate_image.py`，纯 Python 标准库实现；默认模型 `qwen-image-3.0`，高质量可选 `qwen-image-3.0-pro`。
- 密钥从 `C:\Users\25376\Desktop\qw_key.txt` 自动选取第二个 `sk-ws-...` 工作空间密钥；第一行 `sk-sp-...` Token Plan 密钥不用于文生图，完整密钥不会进入日志、仓库或命令参数。
- 第二个密钥已确认能枚举中国区百炼 239 个模型，包含 `qwen-image-3.0`、`qwen-image-3.0-pro`、`wan2.7-image`、`z-image-turbo` 等图片模型。
- 接口约束：该密钥拒绝异步任务调用，且百炼兼容接口没有 `/images/generations`；脚本使用已验证成功的中国区百炼同步原生多模态生成接口。
- 验证：4 项离线测试通过；真实执行脚本生成 879264 字节 PNG，文件头和实际渲染均正常。测试图：`artifacts\aliyun-skill-live-test.png`。
- nanocodex 已在技能创建后重启，新进程号为 `38516`，窗口响应正常。

### 追加修复：任务化会话标题（2026-08-18）

- 根因：会话索引直接截取第一条用户消息，长背景会变成冗长标题；模型生成标题后 GUI 顶部也没有从当前会话索引同步。
- `ncx-core/src/agent_loop.rs` 新增独立标题概括：不携带会话历史或工具，只让当前模型输出约 6–18 字的动宾任务标题；失败不影响正文任务。
- `ncx-core/src/session_index.rs` 将标题上限收紧到 36 字，支持持久化模型标题并在后续轮次保持；模型失败时使用本地任务片段作为后备。
- GUI 仅在首轮任务正常完成后生成标题；取消、错误和后续轮次不会重复触发。后端发送 `session_title` 事件，前端同步当前顶部标题并刷新侧栏。
- 已将两个历史异常长标题（`18cccdb67fce9ddcb3a01`、`18ccc576322ce5047e281`）迁移为“整理大模型架构资料 PDF”，并确认会话索引仍可完整解析。
- 验证：`ncx-core` **196 通过**；GUI 后端 **27 通过**；Vite 正式构建成功（114 模块）；Windows GNU 正式版与 NSIS 安装包生成成功。
- OpenAI 官方文档只公开 Codex/ChatGPT 桌面端用于在项目和长期任务间切换，没有公开会话标题生成算法；本实现对齐其用户可见的短任务标题体验，不声称复刻内部实现。

### 追加修复：流式响应解码错误恢复（2026-08-18）

- 复现：模型流式响应中途损坏时，provider 返回 `StreamError: error decoding response body`；旧恢复逻辑只识别 `RequestError` 与 `TimeoutError`，因此英文底层错误直接显示为最终回复。
- `ncx-core/src/agent_loop/turn.rs` 已将 `StreamError` 纳入同一轮有限恢复：保留当前用户请求并自动重试，最多 3 次；仍失败时只显示中文可恢复提示。
- 新增回归测试验证流解码错误后第二次请求成功，且 `StreamError` 不会写入会话。
- 验证：`ncx-core` **197 通过，0 失败**。

### 追加调整：完成后隐藏工具过程（2026-08-18）

- 用户最终确认的展示规则：任务执行中允许实时显示工具名称、参数和输出；任务结束后不保留任何工具过程记录，历史会话也不回放工具过程。
- `gui/src/App.svelte` 新增完成态清理：收到 `done`、`error` 或 `loaded` 后过滤全部 `tool_group`，只保留用户消息、最终助手结果和必要错误提示；磁盘会话日志不删除，仍可用于审计和调试。
- GUI system prompt 要求最终回复只包含执行结果和简短的下一步建议，不复述工具调用、日志或中间过程。
- 验证：GUI 后端 **27 通过**；Vite 正式构建成功（114 模块）。

### 追加修复：累计用量与费用跨重启保留（2026-08-18）

- 根因：顶部累计费用依赖前端内存中的 `tokIn/tokOut`，重启后归零；旧版本未将 usage 写入会话日志，因此已丢失的历史数无法准确反推。
- 现在每轮完成后按 `session_id` 持久保存输入/输出 token；应用启动、恢复历史会话时自动加载，新建会话和切换工作区时正确清零，避免串会话。
- 费用仍按当前配置的每百万 Token 输入/输出单价和币种计算；从本版本开始的数据可跨重启保留。
- 验证：GUI 后端 **28 通过**；Vite 正式构建成功（114 模块）。

## 元信息
- 最后更新：2026-08-18（顶部“当前进度”为现行状态；下方 2026-06-29 内容保留作历史背景）
- 分支：**`rust-capability`**（整合线，推 **`origin/gui-merge-featgui`**；`origin/rust-capability` = codex
  的独立 GUI 线，**勿覆盖**）。Python 树 `nanocodex/*.py` 不动。
- remote：`origin` → https://github.com/dgy-github/nanocodex.git（凭据已配）
- 路径：crates `rust/crates/`，GUI `rust/gui/`，基准 `bench/`。
- 工具链：无 MSVC，用 `x86_64-pc-windows-gnu`；每条 cargo 前 `export PATH="$HOME/.cargo/bin:$PATH"`。
- ✅ **`feat/train` 已并入 rust-capability**（merge `a26793b`）：ncx-forge 训练框架全部回灌 —— `genome.rs` 读 `NCX_GENOME` 覆盖 prompt/工具描述、`--dump-genome`/`--from-genome` CLI、`train/` 纯 Python 框架。详见下节 + `train/DESIGN.md`。

## 最近改动（2026-06-29，已测，已推 `origin/gui-merge-featgui`）
> ⚠️ push 坑：`rust-capability` 默认 upstream 误指 `origin/rust-capability`（codex 线）。**推必须显式**
> `git push origin rust-capability:gui-merge-featgui`，别裸 `git push`。

- **合入 feat/gui 完整前端**（merge `9d623cc`）：feat/gui 的 1490 行 GUI（侧栏会话列表/最近会话、resume+fork、
  git·diff·记忆·文件·checkpoints 面板、slash 面板、token 流式+用量、4 模式权限、中文化）并入整合线。
  **之前给精简 GUI 加的 token 条（b78dfba）被 feat/gui 自带 usage 取代**。6 文件冲突已解（GUI 文件取 feat/gui，
  lib.rs/main.rs/.gitignore 取并集）；`.gitignore` 现忽略 `.nanocodex/`、`.ncx/`。
- **CLI slash 扩展**（早于合并）：`/export`（会话→md，动态围栏、拒覆盖/拒目录）、`/review`·`/security-review`·
  `/verify`、`/docx·pdf·pptx·xlsx`（prompt+shell 调后端，未装给 pip 并征询）、别名 `/update-config`·`/usage-credits`。
- **eval 数据持续更新机制**（`26b9b76`）：`templates/eval-data-pipeline/`——可移植模板（自定义命令 + analyze
  skill + 日报 + gate 文档）+ `example/` 自包含可跑参考（`run_pipeline.py --self-check` + `eval.py`，纯 stdlib，
  13 行合成快照→4 候选）。原则：生产只采集、本地 agent 分析提案、CI/人审做 gate；daily loop 对 eval 集**只读**。
- **与 codex 线对齐**（codex 独有项全部摘到整合线）：
  - `c167348` **per-prompt 记忆召回**（agent_loop 按 prompt 召回为 per-turn note；去掉 main/runner/bridge 的启动静态召回）+ **`--mcp` 启动门**（默认不起 MCP，加 `--mcp` 才连）。
  - `20cd53b` GUI **打开会话日志/快照/记忆文件**（`open_session_log`/`open_session_snapshot`/`open_memory_file`）。
  - `056a622` GUI **自定义命令面板**：把自定义命令引擎抽到 **`ncx-core::custom_commands`**（CLI+GUI 共享、去重）+ `get_custom_commands`/`expand_custom_command` + App.svelte slash 面板并入自定义命令（选中展开进输入框；`runSlash` 清空前先捕获尾随参数）。
  - 跳过 codex 的 `remember_note`（本线 `memory_add` 已覆盖）。
- 测试：全 rust 工作区 **273 绿**；GUI 后端 `cargo check` + 前端 `vite build` 均过；GUI 已实跑（含自定义命令面板）。
- 坑：`tauri dev` 报「Port 5179 already in use」= 上次残留 vite 孤儿占端口；`taskkill //F //PID <node>` 后重启即可。

## ncx-forge 训练框架（分支 `feat/train`，已推 origin）— 当前活跃工作线
目标：让强模型当"教师"迭代优化 agent 骨架（system_prompt + 工具描述），用 bench 通过率当
fitness 做闭环进化。**只训 Rust 版 `ncx.exe`**；权重不动，纯 API。完整设计见 `train/DESIGN.md`。

- **隔离开发**：在独立 worktree `D:/agent_prac/ncx-train` 上做（主 worktree 有并行会话在
  thrash + 一个 Codex agent 重置 cwd）。接手请 `git worktree add <dir> feat/train` 后在其中干，
  **用绝对路径 / `git -C` / `--manifest-path`**，别依赖 cwd。
- **M0a ✅（地基）**：
  - P1 `NCX_GENOME` 注入（`f1af9ce`）：`ncx-core/src/genome.rs` 读 TOML 覆盖 system_prompt +
    工具描述；覆盖在注册层应用（`schema_for`/catalog），空 genome **字节等价**。
  - P2 失败轨迹采集（`train/evaluator.py`）：跑 ncx 注入 genome，从 `<ws>/.nanocodex/session.jsonl`
    抽 agent 末条消息+工具调用，**剔除 grader 行**（check.py 不外泄）。
- **M0b ✅（最小闭环）**：
  - `ncx --dump-genome`（`90d0a20`）吐默认 genome → `train/genome.py` extract-current + 校验
    (size cap 从基线取) + round-trip。
  - `train/teacher.py` 可插拔 panel：**codex(GPT，模型从 `~/.codex/config.toml` 解析) + claude
    (Opus，按 `is_error` 判) + api(DeepSeek 地板)**。npm shim 用 `shutil.which` 解析 `.CMD`。
  - `train/forge.py`：`--self-check`（sentinel 注入门，确定性）/`--baseline`/`--train`（gen0→
    每代教师提议→评测→**接受门:train升+holdout不退**→JSON lineage + wall-clock governor）。
  - **live 验证**：codex(gpt-5.4) 与 api(deepseek) 都真实产出合法候选 genome（动 prompt/澄清
    shell，**不动 apply_patch**）；forge --train 端到端跑通；接受门 monkeypatch 单测 3/3；P2 单测 5/5。
- **M1 ✅（抗过拟合，`4e36738`）**：`splits.py`(task 级 train/val/test) + `taskgen.py`(教师造题，
  **自校验**：参考解过 check×2 + seed 态失败才入库，→ `bench/tasks/gen_*` gitignore) +
  forge 噪声感知接受(每代重评 incumbent + `--accept-margin` + test 末尾打无偏分)。
  live：api 造出 Unicode/ZWJ 重叠子串难任务并入库；trivial 任务被正确拒。6+3+5 单测全过。
- **临门一脚已做（真能训验证）**：workflow 12 个 Opus 并行造题 → 自校验门 **9/12 入库**
  （3 个"参考解过不了自己的 check"被正确拒）→ bench 现有 10 个 gen_* 难任务（gitignore）。
  baseline 扫：deepseek-v4-pro **9/10 全过**（仅 stable_topo 失败）→ 强基线，harness 余量薄。
  `forge --train`（train=stable_topo）**全闭环跑通**：gen0 0/1 → 教师(api)真提出合法变异
  (system_prompt 192→748 + web_fetch 描述) → 评测仍 0/1 → 噪声接受门**正确拒绝**(+0<margin) →
  无回归。**结论：框架真能训**（propose→validate→evaluate→accept 全活、不伪造提升）；本轮教师
  没抬升，因 codex/claude 当时不可用、教师=agent 同模型 + 硬推理任务 prompt 改不动（印证
  *model is the lever*）。
- **修了个 live bug**（`21400af`）：失败任务若 timeout→空轨迹，旧逻辑误判"train 全过"停在 gen0；
  现 evaluator 给无轨迹失败合成信号（"timed out"），forge 区分"全过"与"有失败但无信号"。
- **codex(gpt-5.4) 教师重跑已做**：codex 恢复可用，`forge --train --teacher codex`
  (train=stable_topo+csv) 全闭环跑通：gen0 1/2 → codex **两轮都提出实质合法变异**
  (R1 system_prompt 192→663 + read_file/shell/update_plan 扩写；R2 192→866 不同改法) →
  两轮评测都 **1/2 无提升** → 接受门**均正确拒绝**(+0<margin) → 无回归。耗时 1321s。
  **结论：即便上 gpt-5.4 强教师，也没抬升 deepseek agent 在这些算法任务上的通过率** ——
  因为这些 task 的失败是底层推理/效率所致、非 prompt 可修；强力印证 *model is the lever*。
  框架本身完全正确：强教师真engaged、提出高质量候选、噪声门顶住不伪造提升。
- **骨架敏感任务 + 逼出 lift（已做，capstone）**：workflow 造 8 个"prompt-可修习惯"任务
  （exact ValueError 契约/无 stdout/输入不可变/精确公共 API/精确返回类型/最小编辑…），自校验
  8/8 入库。但 **baseline 全过 16/16** —— 强 agent + nanocodex 默认骨架已经不踩这些坑，
  说明**真实默认骨架的 harness 余量也很薄**（model 与默认 prompt 都已够好）。
  于是做**诚实的优化器能力测试**：新增 `forge --train --from-genome <degraded.toml>` 从
  人为劣化的骨架起训（system_prompt 诱发 print/原地改/加 helper）。结果（codex gpt-5.4 教师）：
  **gen0 train 1/2 → R1 codex 重写 system_prompt(351→1345) → train 2/2 被接受**（margin≥1、
  holdout 1/1 不退、test 无回归）。**结论：headroom 存在时，优化器能真产出经噪声门+holdout
  验证的 lift**（`889078f`）；但默认骨架上余量薄 → 真实增益靠更强 model / prompt-可修的失败。
- **M2 ✅（搜索增强，`a6a47d2`）**：`pareto.py`（多目标 pass↑/cost↓ dominance+front+NSGA-II
  crowding，6 单测）+ `forge.py --population/--pop-cap`（`evolve()` 小种群，保 trade-off，空 eval
  →cost=inf 防误配夺冠）+ `viz.py`（lineage→自包含 HTML：Pareto 散点+血缘表）。3 population 单测；
  对抗复审判 pareto CORRECT(2万随机 0 违例)、evolve substantially correct（其 1 medium 已修）。
- **M2+ 收尾 ✅（`b88e023`+`8786cbb`）**：① promote 5 难任务进 committed bench（t14_overlap/
  t15_base_n/t16_csv/t17_running_stats/t18_rank_purity，均验 seed 失败+无泄漏解+baseline 可解）；
  ② `evolve` 加 `reeval_parents`（默认开，每代重评存活成员，防 lucky 早抽钉死 front）；
  ③ **ncx 一次性模式 stderr 吐 `[ncx-usage] total_tokens=N`**（唯一新增 Rust 改动，`main.rs`
  emit_usage_line）→ evaluator 解析进 `mean_tokens` → **Pareto cost 优先用真 token、无则回退 mean_s**
  （live：cost=33515 tokens）。26 Python 单测 + ncx-cli 全绿。
- **M3 + 弱base + 大种群 ✅（`3056b29`）**：① `train/export.py`——跑 genome×任务抓**完整轨迹**+
  reward+tokens 写 SFT/RL JSONL（`--reward-pass-only`=SFT 集；schema ncx-forge-trajectory/v1），
  live 验(reward=1/14 轮轨迹/真 token)；② `--base-model`（evaluator/forge 透传 `-m`）训**更弱 base**
  （deepseek-chat 余量更大）；③ `forge --population --base-model deepseek-chat --pop-cap 4` 大种群跑
  （结果见 train/runs/lineage_*.{json,html}）。28 Python 单测全绿。
- **🎯 弱 base 真 lift（默认骨架，已复现）**：`forge --population --base-model deepseek-chat`
  （codex gpt-5.4 当教师，train=t14/t16/t18）：**gen0 默认骨架 0.67 → gen1 codex 重写
  system_prompt(192→852)+read_file/shell/update_plan 描述 → 1.00**，Pareto cost 用真 token，
  lineage+viz HTML 已出。证明：**base 够弱（默认骨架有真 headroom）+ 教师够强时，框架能在
  默认骨架上真抬升**（不再需要人为劣化）。修了 cp1252 `→` 崩溃（UTF-8 reconfigure）。
- **gate 已加重试 ✅**：sentinel 自检对 with-genome 探测重试 ≤3 次（模型偶尔不回显码字是噪声、非
  注入失败），单次 miss 不再 block 训练；2 个新单测。
- **export system_prompt = genome base（有意，非缺陷）**：完整拼接 prompt 含 workspace 专属的
  项目指令/memory/skills，会污染可移植 SFT 数据；且把 system 写进 session.jsonl 会让 resume 重复。
  故 export 取**进化的 genome base**（更干净的训练信号）。
- **权重训练脚手架 ✅（`train/finetune.py`）**：`--mode sft`（export reward=1 → chat → trl
  SFTTrainer，trl/torch 懒加载）+ `bench_reward()` RL 奖励 + `rl_design()`（诚实：agentic RL 需
  GPU 侧 rollout collector，非 vanilla GRPO）。数据转换在本机可跑+5 单测；`--mode prep` 预览+打印
  GPU 运行命令。**真正训练只差一台 GPU**：`pip install trl transformers torch peft datasets` →
  `python train/finetune.py --mode sft --data <export.jsonl> --model <hf-model>`。
- **agentic-RL rollout collector ✅（`train/rollout.py`，分支 `feat/train-rl`）**：`collect_rollout`
  (注入 policy chat_fn + tool_exec 的 model↔tools episode，回合末 `bench_reward` 0/1) +
  `ncx_episode`(复用 ncx 真 loop，指向 vLLM-served policy，读 session.jsonl，**推荐生产路径**) +
  `grpo_advantages`(组内归一) + `collect_group`(N episode→优势)。纯逻辑本机可跑+5 单测；
  `run_grpo` 的 token 级 `policy_update` 是 GPU/torch 部分(懒加载+契约)。`finetune.py --mode grpo` 指到它。
- **下一步（仅剩需 GPU / 大算力）**：① 在 GPU 上把 `rollout.run_grpo` 的 `policy_update` 接上
  (vLLM 服 policy + ncx_episode 收 rollout + trl/自写 PG step)；② 跑 finetune.py SFT；③ 扩 corpus。
  **本机功能面 100% 闭环**（含 SFT 数据/脚手架 + RL rollout 收集器 + 验证奖励，只差 GPU 跑权重更新）。
- **diff() 小瑕疵**：champion 的 tool_desc 显示 "→0 chars" 是因 genome 未指定该键（=用默认），
  非真清空；注入对缺失键正确回落默认。diff 显示未区分"缺失"与"清空"，纯展示问题。
- **已知限制**：强基线 + 算法任务 = harness 余量薄；harness 优化对"模型能力门"无效，只对
  "工程习惯门"有效。教师必须比 agent 强，且任务失败须 prompt-可修，才可能抬升。
- **forge Do-Not**：① 别硬编码 codex 模型名（本机经 CLIProxyAPI 代理=gpt-5.4，`-m gpt-5`→502）；
  ② claude 401 是 rc=0+`is_error:true`，只能按字段判；③ api 地板优先用 `$DEEPSEEK_API_KEY`
  （config 里是 `ark_api_key`，未必对）；④ 自检别用"refuse genome→通过率降"（模型常无视，不可靠），
  用 sentinel 注入。

## 当前状态（已完成，约 225 测试全绿）
- 6 核心 crate + CLI(`ncx`) + Tauri GUI + **`ncx-mcp`**（MCP stdio 客户端，已接进 agent：McpTool + mcp.toml loader + 启动注册）
- 工具：read_file·apply_patch·shell·update_plan·grep·glob·web_search·web_fetch·tool_search·remember·skill
- **Skills（已并入 rust-capability）**：SKILL.md 发现 + 渐进披露注入 + `skill` 工具 + builtin（`commit-message`，include_str! 编入二进制，FS 同名可覆盖）+ `/skills` 命令。stream C vision 基础（`7de2235`）也随 FF 一起进了 rust-capability。
- 分层 flash/pro 编排器（`-o`，verifier 选 BEST worker + promote）；memory 自进化 + 启发式/LLM consolidate（`--memory-merge`）；keyed 搜索(Tavily/DDG)
- 已并入并行会话 18 commit：session 持久化/resume、checkpoints、hooks、project_instructions、富 slash、compact、token usage、release 脚本

## 并行拆分（多会话同时做）——接手按此认领
**硬约束**：① 每会话**独立 git worktree**（别共用工作目录）：`git worktree add ../ncx-A -b feat/mcp rust-capability`；
② 从已推的 `rust-capability` 分叉；③ push 前 `cargo test` 全绿；④ 频繁 `git pull --rebase`；⑤ 一个会话当 integrator 合并。

| 流 | 任务 | 拥有/新建文件（低冲突） | 依赖 |
|---|---|---|---|
| **A 分支 feat/mcp** ✅完成(`dc56233`，已并入) | ncx-mcp crate(stdio JSON-RPC client) + McpTool(`Rc<Mutex<McpClient>>`，非只读走审批) + `~/.nanocodex/mcp.toml` loader + main.rs 启动注册。mock server live 测过。⚠️ 之前这些文件未入库导致 HEAD 干净 checkout 编不过，已修复 | `ncx-core/src/mcp_tool.rs`、`crates/ncx-mcp/`、`ncx-config` servers 字段 | 无 |
| **B 分支 feat/skills** ✅完成(`b70907b`) | SKILL.md 发现 + 渐进披露注入 + `skill` 工具(已 live 验) | `ncx-core/src/skills.rs`(新)；tools/lib/cli/runner/gui 各加几行 | 无 |
| **C 分支 feat/vision** ✅完成(已并入) | VL 视觉分流：`with_vision_provider` + `has_image_block` 路由；CLI `--image`(可重复)/REPL 内联 `--image`；base64 多模态 content；`vl_base_url/vl_api_key/vl_model` 配置；含测试 | `agent_loop`、`cli/main.rs`、`ncx-config` vl 字段 | 无 |
| **D 分支 feat/orch** ✅完成(`3207b43`+`3090436`+`23c993a`) | high 任务递归分解：plan→decompose→每子任务 recurse(顺序、各自 promote)→main verify；atomic/depth 耗尽回退 best-of-N(`high_workers`=3)。旋钮 `high_workers`/`max_depth`(0=关)/`max_subtasks`(默认6，防过度拆分)。reasoning 节点(classify/plan/decompose/verify)**无工具**(`reason()`，否则强模型边分类边执行)。`parse_subtasks` 容错(SUBTASK:→编号/项目符号回退，live 模型常不守格式)。`LocalBoxFuture` 保 ?Send。13 测试。`NCX_TRACE` 有 `[orch]` 行。**live 验证**：classify High→decompose→recurse 已触发；但分类器保守(小任务判 Medium)+全 pro 慢，整条 High 递归未跑到完成 | **独占 `ncx-core/src/orchestrator.rs`** | 无 |
| **E 分支 feat/bench** ✅完成(`b175a74`+`96730f0`) | bench：`--repeats`(默认3)通过率 + md/json 报告 + `--tasks` 过滤 + Claude 臂。任务 t1–t13：**新增 5 个难任务** t9_expr_eval(递归下降+优先级)/t10_intervals/t11_wildcard(DP)/t12_toposort(环检测)/t13_jsonpath(嵌套+falsy 边界)，grader 均经参考解验证 well-formed + live 5/5 | **整个 `bench/`（纯 Python，零 Rust 冲突）** | 无 |

**冲突热点（只有这几处，纪律）**：`tools.rs`(register 行)、`lib.rs`(mod/export)、`Cargo.toml`(deps)、`cli/main.rs`(接线)。
**约定**：每条流对这些共享文件只加 **1–2 行**、加在末尾/固定锚点 → 合并是 trivial。
**建议并行度**：A/B/E 最独立（新文件为主），先开这三条；C/D 第二批。
之后 ROI 顺序若还要扩：③ skill(=B) → ④ image(=C) → ⑤ orch(=D)。鲁棒性不单独做，靠以上 + 真实使用磨。

## 基准（bench/，自动评分）
`python bench/run.py --agent <nanocodex|nanocodex-orch|opencode|claude|all>`。同模型 deepseek-chat：nanocodex 4/4、opencode 3/4
（**N=4 单跑、在噪声内，不能断言优势**）。Claude 臂 `claude -p` 报 401，需 `ANTHROPIC_API_KEY`。

## 流 A 完成情况（feat/mcp）
- `ncx-config`：`McpServerConfig` 结构体 + `load_mcp_servers()`/`load_mcp_servers_at()` 解析 `~/.nanocodex/mcp.toml`
- `ncx-core/src/mcp_tool.rs`：`McpTool`（`Rc<tokio::sync::Mutex<McpClient>>` + 审批）+ `register_mcp_server()` 启动帮助函数
- `ncx-cli/src/main.rs`：`ToolRegistry::new` 后自动加载并注册所有 MCP server 工具
- Live 验证：`everything` server 注册 13 个工具，模型成功 `tool_search` + `echo` 调用

## Do-Not（踩过的坑）
- tauri lib 用 `crate-type=["lib"]`（cdylib → gnu ld `export ordinal too large`）；GUI crate 须自列 `async-trait`。
- svelte-plugin `^5` 配 vite `^6`。工具描述**逐字照搬**（含示例），否则模型发 git-diff 死循环；调试 `NCX_TRACE=1`，别用 `| head`（SIGPIPE 打断进程，重定向到文件）。
- opencode：`npm i -g opencode-ai` 后若 "postinstall not run"，手动 `cd node_modules/opencode-ai && node postinstall.mjs`；bin 在 `~/AppData/Roaming/npm/node_modules/opencode-ai/bin/opencode.exe`；DeepSeek 配 `~/.config/opencode/opencode.json`。
- 预期校准：这些抬完成率/触达面，**不抬硬推理天花板**（封顶在 deepseek-v4-pro < Fable）。真正上限杠杆=main 换强模型（`DeepSeekProvider` 已 OpenAI 兼容，改 base_url/key/model 零代码）。
- 残留：`git stash list` 的 `stash@{0}`=会话前 Python 时代 README/config.example 旧改动（已被远程取代，可丢）。
- MCP on Windows：`Command::new("npx")` 找不到 `.cmd` 脚本；`mcp.toml` 里用 `command="cmd"` + `args=["/c","npx",...]` 才能启动。
- 编排器 live 坑：`run_in` 给**所有**节点挂全部工具时，强模型在 classify 回合就 apply_patch 把活干了（classify 永不快速返回）→ 已用 `reason()` 无工具修。子任务隐患：分类器保守 + 无 fast_model 时全 pro，high 递归子任务多→跑不完；用 `max_subtasks` 限。要确定性验 high 递归，需 fast_model 或一个 `-o` 强制 complexity 的开关（尚无）。

## 记忆指针（auto-memory）
rust-rewrite-setup · rust-rewrite-rationale · rust-apply-patch-tool-desc · rust-tauri-gui-gotchas · rust-orchestrator-capability

## 2026-08-18 会话切换与历史轻量化
- 后端恢复历史时只向 GUI 投影每轮用户消息与最后一条非空助手回答；工具名、参数、结果和中间播报不再跨后端/UI 边界。
- Resume/Fork 快照从原先同一路径读取两次改为一次读取，长会话切换减少重复 JSON 解析。
- 流式文本、工具、审批、提问、完成、恢复和错误事件全部携带 `session_id`；前端只接收当前会话事件，切换后旧任务不能污染新界面与累计用量。
- 新会话在进入命令队列前分配 ID，并以空消息种子创建；继续共享当前项目目录、规则、skills 和文件，但不会继承旧聊天与未完成计划。
- 保存设置或应用模型预设改为保留当前会话 ID 重建，不再暗中创建一个前端不知道的新会话；只有切换项目/显式新建才创建空会话。
- 首轮标题生成改为独立 `ncx-title` 线程，不再阻塞串行 agent 命令队列。
- 验证：`ncx-core` 197 项、GUI Rust 32 项测试通过，Vite 正式前端构建通过；正式 Tauri 构建在本轮交付前继续执行。

## 2026-08-18 多会话并发执行
- 修正上一版“切换时停止旧任务”的错误语义：导航/配置协调器不再直接等待 `run_turn`，每个 Prompt 按 `session_id` 分派到独立 `ncx-turn-<session>` OS 线程；切走后原会话继续执行。
- Prompt 前后端契约增加 `session_id`；不同会话可以并行，同一会话仍禁止重入，后续消息保持该会话内串行。
- 运行态、取消标记、审批/提问归属、always-allow grants 全部按 session 隔离；停止 A 不会取消、拒绝或清空 B。
- 每个 session 改写入 `.nanocodex/sessions/<session_id>.jsonl`，避免并发追加同一个审计日志；快照仍由 SessionIndex 分会话持久化。
- 前端为每个会话缓存正在执行时的可见消息、待发送队列、审批和问题；切回运行中的会话不会丢用户消息，后台完成的 token/费用会记到对应会话。
- 最近会话侧栏增加“执行中”状态；新建、继续、分叉或切换会话不再调用 `stop_generation`。
- 验证：GUI Rust 37 项测试与 Vite 生产构建通过；包含并发占用、同会话防重入、目标取消隔离、独立日志和前后端路由契约。
- 真实 WebView/CDP 集成：A、B 两个会话分别执行 12 秒 shell 任务，侧栏同时观测 `RUNNING_COUNT=2`；两者运行中成功切回 A（仍显示停止按钮），最终 `BOTH_FINISHED`。两个结果分别落到 `18cccdb67fce9ddcb3a01`、`18ccf0037389edf069980` 的独立 snapshot 和 JSONL，均含各自 `SESSION_A_DONE` / `SESSION_B_DONE`。
## 2026-08-19 长会话上下文承接修复
- 真实快照核验：会话 `18cccdb67fce9ddcb3a01` 的 22 条用户消息、329 条助手消息均正常持久化；问题不是切换时丢快照。
- 根因位于 `Session::for_model_edited`：超过上下文预算后只合并旧用户请求，丢弃了旧轮次的最终回答、交付物路径和已选方案，导致“继续处理”没有可承接的完成状态。
- 现改为保留最多 12 条紧邻的“用户要求 + 助手完成结果”会话里程碑；工具调用、参数、结果和中间工具噪声仍不进入摘要。
- 回归测试覆盖 PDF 生成结果、PPT 后续决策点和大量工具噪声；`cargo test -p ncx-core` 197 项全部通过。
## 2026-08-19 DeepSeek 思考过程恢复显示
- 根因：`ncx-provider` 已解析并持有 `reasoning_content`，但 `ncx-core::Provider::chat_streaming` 只向上层暴露正文回调，DeepSeek 适配器把 reasoning 回调写成 `|_| {}`，GUI 因而永远收不到思考内容。
- 新增独立 `StreamDelta::Reasoning` → `LoopEvent::ReasoningDelta` → `UiEvent::ReasoningDelta` 事件链，且所有事件继续绑定 `session_id`。
- 前端“思考过程”始终默认折叠、可手动查看；仅保留最近 4000 字并使用纯文本渲染，避免长推理逐 token Markdown 重排拖卡界面。它与工具日志分开，工具输入/结果和历史恢复过滤规则不变。
- 验证：`ncx-core` 197 项、`ncx-gui` 38 项、Vite 生产构建全部通过。
## 2026-08-19 自动上下文压缩持久化
- 之前 `for_model_edited` 只构造临时发送视图，完整工具噪声仍写入日志/快照；长会话每轮都会重复裁剪，没有真正的自动压缩触发。
- 现在每次模型调用前检查 `context_edit_max_chars`：未超限不动；超限后调用 `Session::compact_if_needed`，把会话里程碑物化进 `Session.messages` 并立即重写 JSONL，任务结束后保存到全局快照。
- 压缩保留用户要求、助手完成结果、文件路径、当前计划和最近消息；清理旧工具调用噪声。内部压缩摘要不计作用户问数，历史恢复只显示淡色压缩标记。
- 新增 session-scoped `ContextCompacted` UI 事件，显示压缩前后字符数、清理消息数和工具结果数；后台并发会话不会串事件。
- 验证：`ncx-core` 198 项、`ncx-gui` 40 项、Vite 生产构建全部通过。

## 2026-08-19 长思考卡顿与停止按钮
- 根因一：思考流运行时强制展开，每个增量都重新做 Markdown 渲染；长链路会持续放大 DOM 和主线程开销。
- 根因二：点击停止后按钮被 `stopping` 状态禁用；即使后端取消已按会话直接触发，界面仍让用户感觉无法再次停止。
- 现在思考默认折叠、纯文本、最多保留最近 4000 字并限制展开高度；停止按钮在任务真正结束前保持可点击，可重复发送当前会话的直接取消请求。
- 完成态进一步按整个会话逐轮收口：执行中可临时显示思考和中间播报；收到 `done` 后，每一轮都保留用户消息和该轮最后的正式回答，清掉各轮中间播报，而不是只整理或只保留最后一轮。

## 2026-08-23 DeepSeek Harness 组件化 M2-M4 进展
- 在独立工作树 `D:\github_dgy\nanocodex\.worktrees\deepseek-harness-components` 的 `feat/deepseek-harness-components` 分支继续实施“一切皆插件”的架构。
- M2-M4 已从仅注册插件升级为类型化能力服务：`ncx.llm` 发布模型/推理/视觉能力描述，`ncx.interaction` 发布审批入口，`ncx.policy` 发布沙箱、审批、计划和网络策略，`ncx.context` 发布工作区/记忆/技能上下文描述；`memory`、`compaction` 已保留为独立能力插件入口。
- `CoreToolsPlugin` 通过运行时服务读取策略和上下文，兼容旧的直接工具注册测试；基础 Bundle 已包含上述能力插件，可由 Profile/Bundle/Overlay 选择启用。
- 该条早期记录描述的是当时的未完成状态；后续提交已将 AgentLoop、Provider、Memory、Compaction、Policy、Interaction、Context 接入服务消费者，当前以本节后续增量和最新测试结果为准。
- 本轮验证：`cargo test -p ncx-core --lib plugins::` 13 项通过；`cargo check -p ncx-cli` 通过；GUI Tauri `cargo check` 通过。未生成或提交 Cargo.lock 等无关改动。
- 后续增量 `90a16cb` 已补齐 `MemoryServiceDescriptor` 与 `CompactionServiceDescriptor`；`AgentLoop` 通过 `ToolRegistry::service` 消费 compaction 服务，没有安装该插件时不会自动压缩。全量 `cargo test -p ncx-core --lib` 211 项通过，提交已推送到 GitHub。
- 增量 `93b4a50` 让 `AgentLoop` 消费 `ncx.llm` 发布的能力描述：无推理能力时不再发送 reasoning effort，无视觉能力时回退主 Provider；同时公开 `llm_capabilities()` 供前端/运行时诊断。全量 `cargo test -p ncx-core --lib` 211 项通过，已推送。
- 本轮将 Provider 实例装配闭环：新增 `LlmProviderFactory`/`LlmProviderFactoryHandle` 服务，CLI、GUI 和编排 runner 均通过 `install_llm_provider_factory` 注入配置，再由 `AgentLoop::from_runtime_services` 创建主/视觉 Provider；标题生成等独立短调用仍保留显式 Provider，避免引入无意义的工具运行时。验证：`cargo test -p ncx-core --lib` 211 项、`cargo check -p ncx-cli`、GUI Tauri `cargo check` 均通过。
- M4 增量：`MemoryPlugin` 现在发布带实际 `MemoryStore` 的服务描述，AgentLoop 的提示前记忆召回改为从 `ToolRegistry` 的 memory 服务读取，不再直接耦合 `ToolContext.memory`；记忆回归测试通过。
- M4 增量：`AgentLoop` 现在同时消费 `policy` 与 `interaction` 服务；`runtime_profile()` 优先使用 Harness 策略服务生成有效权限快照，并公开交互服务是否存在，旧的无插件单元测试仍回退到 `ToolContext`。全量 `ncx-core` 211 项测试通过。
- `dc7a915` 补充 AgentLoop 对 `context` 服务的持有与诊断接口；Context/Memory/Policy/Interaction/LLM/Compaction 服务均已有运行时消费者或能力查询入口，分支已推送。
- 最终阶段审计：`cargo test --workspace --quiet` 全部通过（包含 ncx-core 211 项及其他 workspace crate）；`cargo check -p ncx-cli` 与 GUI Tauri `cargo check` 通过。CLI 测试所需的视觉 Provider 兼容导入已恢复，未改变运行时装配路径。

## 2026-08-24 DeepSeek Harness M5-M6
- M5 新增 `ncx.mcp`、`ncx.attachment`、`ncx.media`、`ncx.cost-telemetry` 四个内置插件和类型化服务。MCP 的连接/重载会同步服务器与工具数；CLI/GUI 附件读取前受插件格式/大小策略约束；AgentLoop 通过 media 服务控制视觉路由，并可通过 cost 服务估算本轮费用。
- `full` Profile 新增 media Bundle；`minimal`、`headless` 不加载媒体/MCP/费用能力。三种 Profile 已有真实装配隔离测试。
- GUI 设置页新增 Harness 服务诊断、工作区外部插件列表、安装、升级、启用和停用入口。
- M6 新增工作区外部插件 Catalog：`plugin.toml` 发现、目录复制安装、版本递增升级、启停标记和进程启动。仅允许协议 v1、目录内相对命令；拒绝路径穿越、符号链接和 DLL/SO/DYLIB，外部实现使用清空环境的独立子进程和管道通信。
- 最终验证：`cargo test --workspace --quiet` 全部通过（`ncx-core` 215 项）；CLI 34 项、插件/外部目录/组合测试均通过；CLI/GUI Tauri check 与 Vite production build 通过。MSVC 桌面发布构建产出 `ncx-gui.exe` 和 `bundle/nsis/nanocodex_0.1.0_x64-setup.exe`。仓库默认 `tauri:build` 指向未安装的 GNU target，本轮改用已安装的 `x86_64-pc-windows-msvc` 成功验证；严格 Clippy 仅被未触及的 `ncx-tools/src/text_encoding.rs` 旧 lint 阻挡。

## 2026-08-24 OpenAI Codex Harness M0-M6 差异审查
- 对照源码：OpenAI `openai/codex` main 提交 `068c49f075cf287a1fe7d1ee36cf005efac922e7`；nanocodex 组件分支提交 `97c62d35dc2a699d5f82f504919d2b35c4fbc03b`。
- 核心判断：现有 M0-M6 已形成可组合能力原型，应保留；但 OpenAI 架构的关键不是把全部核心做成插件，而是稳定 Agent 内核、版本化 app-server 协议、存储中立 Thread/Turn、资源型插件和 Hooks 分层。
- 最高优先级缺口是 M3：nanocodex 的会话所有权仍分散在 GUI bridge、SessionIndex、快照和前端缓存，缺少统一的 ThreadId/TurnId/Item/Event 契约与存储接口。
- M4 缺少 OpenAI 式类型化 ContextFragment、片段硬上限、StoredModelContext 以及 Pre/PostCompact Hook；当前 Compaction 服务主要是启用描述符。
- M6 的 `plugin.toml` 子进程当前只有发现、安装、升级、启停和启动隔离，尚无可注册真实能力的正式协议。后续不要先扩展任意可执行插件，应优先兼容 `.codex-plugin/plugin.json` 的 Skills/MCP/Apps/Hooks 资源模型。
- 推荐迁移顺序：P0 新增 `ncx-protocol`、`ncx-thread-store`、`ncx-app-server`；P1 拆 Provider/Policy/ToolExecutor/ContextFragment 接口；P2 兼容 OpenAI 资源插件格式并保留 Profile 作为产品预设；P3 再补 Marketplace、来源策略、健康检查、卸载、签名与资源限制。
- 迁移不变量：多会话继续并发；切换不取消旧任务；事件绑定 thread/session 与 turn；新会话不继承旧聊天和计划；费用累计可恢复；附件、视觉、生图和视频复用现有 Provider/Skill，不建立第二套状态机。

### P0 实施进度
- 新增 `ncx-protocol`：协议 v2、强类型 `ThreadId`/`TurnId`/`ItemId`、Thread/Turn/Item、客户端请求、服务响应及所有事件统一 envelope；事件强制携带 threadId、可选 turnId、协议版本与单调序号。
- 新增 `ncx-thread-store`：存储中立 trait 与 JSON 实现；单线程只允许一个活动 Turn，不同 Thread 可并发；支持创建、读取、列表、元数据更新、分叉、追加 Item、完成/取消 Turn；写入使用临时文件和备份回滚，完成状态可跨重启恢复。
- 新增 `ncx-app-server`：统一处理 thread create/list/read/archive/fork、turn start/interrupt/complete 和 item append，并输出版本化响应及事件。
- GUI Tauri 后端已持有 app-server，开放 `app_server_request` 入口；新建会话使用同一 session/thread ID 双写 v2 store，归档对已迁移线程同步写入。现有 SessionIndex 和 bridge 暂时保留，避免一次性破坏旧历史。
- 验证：三个新 crate 共 8 项测试通过；GUI 后端 44 项测试通过；GUI `cargo check` 通过。下一步把 prompt、流式 Item、完成/取消和历史读取切到 app-server，完成 GUI 协议客户端迁移后再进入 Provider/Policy/ContextFragment 拆分。

### 2026-08-24 P0-P2 增量
- GUI Prompt 已接入版本化 Turn 协议：执行前按 session/thread 创建 `TurnStart`，用户消息、助手最终回答、工具调用/结果及上下文压缩写入同一 `thread_id + turn_id`；正常结束写 Completed/Cancelled/Failed，工作线程异常退出由 Guard 写 Failed 并释放同会话所有权。
- 新增协议回归，证明完成和异常退出都不会遗留永久占用；不同 Thread 并发、同 Thread 禁止重入的不变量继续保留。旧 `SessionIndex` 历史读取仍作为兼容层存在，下一阶段再把历史列表/恢复切成 app-server 投影后删除双写。
- Provider 契约迁至真实所有者 `ncx-provider`，`ncx-core::model_provider` 仅保留兼容 re-export；Policy 快照归 `ncx-sandbox`；新增 `ncx-context` 持有 `ContextFragment`、编辑策略和统计，并强制片段硬字符上限。没有建立第二套 Provider 或压缩状态机。
- 新增 OpenAI Codex 资源插件兼容：解析/校验 `.codex-plugin/plugin.json` 的 Skills、MCP、Apps、Hooks、Interface；支持本地安装、原子式升级、启停、卸载，拒绝路径穿越和符号链接。GUI 设置页可管理资源插件并查看 Marketplace。
- Marketplace 对齐发现 `.agents/plugins/marketplace.json`、`api_marketplace.json`、`.claude-plugin/marketplace.json`、`.cursor-plugin/marketplace.json`；本地 source 从仓库根安全解析并可安装，Git/NPM source 明确返回“需先物化到本地缓存”，本阶段不擅自执行网络下载。
- 验证：Rust workspace 全量通过（`ncx-core` 219 项及所有其他 crate）；GUI Rust 46 项全通过；Vite production build 通过；`git diff --check` 通过。

### 2026-08-24 OpenAI Codex Harness 迁移收口
- `ncx-protocol` 已补齐 GUI 所需的 Thread 重命名/导入/创建激活/恢复激活/分叉激活与 Turn 提交/停止请求；所有协议事件继续携带 v2、单调 sequence、threadId 和可选 turnId。
- `ncx-thread-store` 成为会话持久化和活动 Turn 所有权的事实源：同 Thread 禁止重入、不同 Thread 并发；异常退出释放所有权；重启时残留 Running/Queued Turn 转 Failed；主文件损坏时可从 `.bak`/`.tmp` 恢复；分叉不会复制活动所有权。
- GUI 历史、新建、恢复、分叉、归档、Prompt 和 Stop 主路径均通过 `app_server_request`；前端按 Thread 拒绝旧序号或错误协议版本事件。旧 `SessionIndex` 仅保留启动迁移、日志打开和兼容快照，不删除历史数据。
- 旧 SessionIndex 启动时批量导入 v2 Store，只投影每轮用户要求和最终结论；工具输入、工具输出与中间日志不进入恢复历史。费用与最终回答仍随 Thread 恢复。
- Provider 契约归 `ncx-provider`，Policy 归 `ncx-sandbox`；CLI/GUI 的项目说明、Skills 和 Plan 提示已改为类型化 `ContextFragment`，并在组装时强制字符硬上限。
- OpenAI 资源插件已实际接入现有运行时：已启用插件的 Skills 可发现/加载，Hooks 映射到既有生命周期，MCP 会连接并注册工具且更新 GUI 诊断；禁用插件不会加载 Skills/Hooks/MCP。Apps 当前完成目录解析与诊断展示，因项目没有独立 Apps 执行子系统，不另建第二套状态机。
- `.codex-plugin/plugin.json`、本地 Marketplace、Git source 和 NPM source 已支持发现、校验、安装与升级；Git/NPM 会先物化到工作区 `.ncx/codex-plugin-stage`，包含子路径越界、清理边界和 NPM 包名校验。当前未使用外部真实 Marketplace 做网络 live 安装，相关安全与解析路径由单测覆盖。
- 组合验证继续覆盖 `full`、`minimal`、`headless`：Skills/Context 属基础能力；media/MCP/attachment 等仅在相应 Profile 与已启用资源存在时装配。
- 本轮验证：`cargo check --workspace`、GUI Tauri check、`cargo test --workspace --quiet` 全通过（`ncx-core` 222 项）；GUI Rust 52 项全通过；Vite production build 与 `git diff --check` 通过。

### 2026-08-24 协议主路径与持久化上下文最终收口
- GUI 的会话主路径已完全改为 `ncx-app-server` 协议客户端：新建、激活、分叉、归档、重命名、提交、停止和历史读取均不再回退旧 Tauri 会话命令；协议事件继续绑定 `thread_id + turn_id + sequence`，切换会话不会取消其他会话的后台任务。
- `ncx-thread-store` 现在同时持久化用户可见 Thread/Turn/Item、Provider 使用的 `StoredModelContext` 以及每轮 `TurnUsage`。模型上下文与可见历史分离，自动压缩结果、累计 token 和估算费用都可跨重启恢复；Fork 复制最后稳定上下文但不复制运行所有权。
- 新增后端 `ThreadReadVisible` 投影：历史 UI 只收到每轮用户消息和最后一条正式回答，工具调用、工具结果、推理和中间播报不再从后端传给前端。旧 `SessionIndex` 仅用于启动迁移和旧日志/快照兼容，不再参与 GUI 运行时读写。
- OpenAI Apps 资源兼容补齐：支持 manifest 内联 Apps 和 `.app.json` 的 `id`/`connector_id`，禁用插件不加载，GUI 诊断显示 Apps 数量；Apps 按 Hosted Connector 资源处理，不另建第二套执行状态机。
- 真实桌面端验证：`npm run tauri -- dev --target x86_64-pc-windows-msvc` 成功启动当前工作树的 `rust/gui/src-tauri/target/x86_64-pc-windows-msvc/debug/ncx-gui.exe`，进程响应正常。
- 最终回归：`cargo test --workspace --quiet` 全部通过（`ncx-core` 223 项）；GUI Rust 55 项通过；Vite production build 通过；生成的独立测试 target 目录不纳入版本控制。

### 2026-08-24 CLI 与会话查询统一到 Thread Store
- 完成收口审计后确认 CLI 与 Agent 的 `session_*` 查询工具仍直接依赖旧 `SessionIndex`，会造成 GUI、CLI 和模型查询看到不同历史；现已移除这两条运行时直连。
- CLI 的新建、`--resume`、`--history`、普通 Turn 和 orchestrator Turn 统一写入 `ncx-app-server` + `ncx-thread-store`。每个 CLI Thread 使用独立 JSONL，恢复优先读取 `StoredModelContext`，每轮持久化用户消息、最终回答、状态、Token、估算费用和模型上下文。
- `session_search`、`session_trace`、`session_event_read/search/trace` 改读 v2 Thread Store，并统一使用协议层的可见投影；工具调用、工具结果、推理和中间回答不会重新泄漏到模型的历史查询结果。
- 可见历史投影已下沉到 `ncx-protocol::Thread::into_visible`，app-server 和内部查询共用同一规则，避免两套过滤口径。
- 验证：CLI 35 项通过；新增 CLI 创建/占用/恢复测试和会话查询过滤测试；Rust workspace 全量回归在 CLI 首轮迁移后全部通过，协议投影下沉后需在最终交付门禁再次执行全量回归。

### 2026-08-24 Provider / Policy / ContextFragment 真实消费者收口
- Provider 契约继续由 `ncx-provider` 持有，CLI/GUI 通过 Harness 的 `LlmProviderFactory` 创建实例；核心只保留兼容 re-export。
- Context 的确定性组装器已从 `ncx-core` 迁移到 `ncx-context::ContextAssembler`；新增可执行 `ContextService` + `ContextEntry`。CLI/GUI 不再自行拼接系统提示，而是把项目说明、Skills、Plan 作为有序硬上限片段交给 Context 插件，再从运行时服务生成 Provider 输入。
- Policy 服务从只读诊断快照升级为携带真实 `SandboxPolicy`、审批策略和 Plan 模式的可替换运行时服务。ToolRegistry 在每次工具执行、Middleware 和 Hooks 前读取有效 Policy/Interaction 服务，因此 Overlay/Provider 替换会真正改变执行边界，而不只是改变诊断页面。
- 新增回归证明替换 Policy 服务后工具实际看到只读策略；Context 服务回归证明排序、替换和字符硬上限由独立 crate 统一执行。
- 验证：`ncx-context` 3 项、`ncx-sandbox` 15 项、`ncx-core` 221 项、CLI 35 项、GUI Rust 55 项通过。

### 2026-08-24 Codex 压缩 Hooks 闭环
- OpenAI 资源插件 Hooks 新增 `PreCompact` → `pre_compact`、`PostCompact` → `post_compact` 映射，不再静默忽略压缩生命周期。
- AgentLoop 在真实自动压缩前执行 PreCompact；失败可阻止本次压缩并把诊断作为运行时说明交给模型。压缩写回 Session/JSONL 后执行 PostCompact，并把压缩统计作为 Hook 结果输入。
- `Session::needs_compaction` 公开纯判断，避免为了触发 Hook 先修改会话；实际压缩仍由原有唯一状态机完成，没有建立第二套 compaction 实现。
- 回归覆盖插件资源解析和真实前/后 Hook 执行，`ncx-core` 222 项通过。

### 2026-08-24 Marketplace 升级恢复
- `CodexPluginCatalog` 在发现和安装/升级前自动恢复中断的原子升级：目标目录缺失时把 `.backup` 恢复为正式插件；目标已存在时清理旧 backup；同名 `.staging` 只在 Catalog 根内清理。
- 隐藏的 staging/backup 目录不再作为普通插件进入发现清单，避免升级中断后出现重复插件或插件消失。
- 新增崩溃点模拟回归：手工构造“正式目录已移到 backup、staging 已生成”的状态，下一次 discover 恢复唯一正式插件并清理临时目录；OpenAI 兼容插件相关 7 项测试通过。

### 2026-08-24 真实 WebView 协议修复与 E2E
- 真实 Tauri/WebView E2E 首次调用即发现协议字段命名错误：`ClientRequest`/`ThreadItem` 的枚举 variant 已是 camelCase，但内部字段仍要求 `thread_id/turn_id/call_id`，前端实际发送 `threadId/turnId/callId` 会报 `missing field thread_id`。
- `ncx-protocol` 现对所有枚举 variant 字段统一启用 camelCase，并新增请求/Item 序列化往返测试，确保协议 JSON 与 GUI TypeScript 契约一致。
- 新增 `npm run test:e2e:protocol`：真实启动 MSVC Tauri 桌面端，经 WebView2 CDP 和 Tauri IPC 创建两个 Thread、同时启动两个 Turn、验证同 Thread 重入失败、写入用户/工具/中间回答/最终回答、完成并读取可见历史；结果证明跨 Thread 并发成功且工具输出/中间播报未泄漏。
- Windows 的既有 question E2E 也固定使用 MSVC target，真实 choice/free-text/cancel 三条交互全部通过。

### 2026-08-24 Thread Store 跨进程一致性
- 完成审计发现原 Store 只用进程内 Mutex：GUI 与 CLI 同时打开会缓存不同副本，后写者可能覆盖另一进程；任一新进程打开还会把其他进程的 Running Turn 错误恢复成 Failed。
- `JsonThreadStore` 现用全局文件锁串行化读取/写入，每次操作在锁内从磁盘重载后再原子保存，消除多进程陈旧快照覆盖。
- 每个 Thread 使用独立 OS 文件租约：活跃进程持有租约期间，其他进程保留 Running 状态且拒绝同 Thread 重入；进程退出释放租约后，下一次操作才把孤儿 Turn 恢复为 Failed。不同 Thread 仍可并发。
- 锁文件名同时哈希 Store 路径和 ThreadId，多个测试 Store/用户 Store 互不干扰；Windows 锁竞争错误码 32/33 与 WouldBlock 统一识别。
- 新增真实子进程回归：子进程占用 Turn，父进程验证不误恢复、不重入、可写另一 Thread；子进程退出后父进程恢复孤儿 Turn且保留双方写入。`ncx-thread-store` 12 项、Rust workspace 全量、GUI Rust 55 项通过。

### 2026-08-24 历史会话恢复 E2E 收口
- `test:e2e:protocol` 在真实 Tauri/WebView 中新增页面刷新、展开“最近会话”、点击协议创建的历史 Thread 并恢复最终结论的验证；证明 GUI 历史入口已使用 `ncx-app-server`/`ncx-thread-store` 的可见投影，而不是旧 SessionIndex 或前端缓存。
- E2E 启动时会归档先前失败遗留的 `workspace == "e2e"` 测试 Thread，结束时归档本轮两个 Thread，避免测试数据污染真实历史列表。
- 已验证结果：跨 Thread 并发、同 Thread 所有权拒绝、工具/中间输出过滤、刷新后历史加载和会话激活全部通过，输出为 `protocol e2e: ok (concurrency, ownership, visible projection, history reload/open)`。

### 2026-08-24 App Server 与 OpenAI 插件协议收口
- `ncx-app-server` 新增宿主适配边界并拥有完整协议路由；Tauri 的 `app_server_request` 不再匹配 Thread/Turn 方法，只实现 Agent 队列、取消和桌面资源 I/O。Thread 创建/激活/分叉、Turn 提交/停止因此不再由 Tauri bridge 决定路由。
- Codex 插件列表、安装/升级、启停、卸载、Marketplace 列表及 Marketplace 插件安装均新增 `ncx-protocol` 请求，并由 GUI 统一通过 `app_server_request` 调用；对应旧 Tauri 命令已从 handler 移除。
- 对照 OpenAI Codex `068c49f` 源码补齐真实清单兼容：`mcpServers` 支持文件路径，Hooks 支持单路径/多路径/内联文档，Interface 图标和截图受插件根目录边界校验；Marketplace 支持字符串本地路径、`local`、官方 `url`、`git-subdir`、`npm` 以及旧 nanocodex `git` source，保留 ref/SHA/NPM registry。
- 结构拆分：app-server 单测移到 `src/tests.rs`，OpenAI Marketplace 解析移到 `openai_compat/marketplace.rs`，兼容测试移到 `openai_compat/tests.rs`；这些后端文件已通过代码结构门禁。工具分发参数收敛为 `DispatchOutput`，严格 Clippy 阻挡项同步修复。
- 验证：`cargo test --workspace --quiet` 全绿（`ncx-core` 225 项）；受影响模块 `cargo clippy ... -D warnings` 通过；GUI Rust 56 项、Vite production build、question E2E、protocol E2E 全部通过。协议 E2E 现额外验证插件/Marketplace 经真实 WebView/Tauri IPC 返回。
- GUI 结构拆分已启动：设置弹窗、模型目录、插件管理分别提取为 `SettingsModal.svelte`、`ModelCatalogSettings.svelte`、`PluginSettings.svelte`，三个新组件均低于 300 行并通过生产构建；协议 E2E 会实际打开设置页并检查 Codex 插件与 Marketplace 区域。首次重跑遇到一次 WebView2 CDP 页面发现超时，确认无残留进程后重跑通过，属于测试启动竞争而非页面断言失败。
- 尚未完成的结构债务：`rust/gui/src/App.svelte` 已从 2674 行降至 2466 行，但仍是历史单体，结构门禁继续报超 300 行。下一拆分批次继续拆会话侧栏、消息流、输入区和各工作区面板；在完成前不能宣告整套架构改造全部结束。

### 2026-08-24 GUI 业务组件拆分增量
- 会话侧栏、顶部栏、消息流、输入 Composer、审批/用户问题弹窗、右侧工作区面板已从 `App.svelte` 按业务边界提取；设置、模型目录和插件管理沿用前一批独立组件。
- 输入组件保留模型、DeepSeek 思考等级、权限模式、每会话累计 Token/费用、附件、Slash 命令、排队和可重复停止行为；消息组件保留执行中工具明细，完成后默认折叠工具、思考并保留每轮最终结论。
- `App.svelte` 从本批开始时的 2402 行降至 1943 行；纯 Markdown、工具结果状态和 Diff 行格式化移到 `src/lib/ui-format.ts`。新增生产组件均低于 300 行并通过结构门禁。
- 验证：Vite production build 通过；真实 question E2E 的 choice/free-text/cancel 通过；真实 protocol E2E 的跨 Thread 并发、同 Thread 所有权、可见历史投影、刷新恢复、插件 Marketplace 全部通过。
- 尚未收口：`App.svelte` 的协议事件、会话状态、工作区状态和设置编排仍集中在同一脚本中，后续必须拆成 `.svelte.ts` 控制器/store；在结构门禁通过前，不宣告 GUI 单体拆分完成。
- 扩大回归：`cargo test --workspace --quiet` 全绿（`ncx-core` 225 项及其余 workspace crate）；GUI Rust 56 项全绿。原 GUI 模板测试只读取 `App.svelte`，组件化后已改为读取对应组件事实源。
- 本地 `main` 已快进到组件分支 `986fd31`；HTTPS 推送功能分支和 `main` 仍均被 `Recv failure: Connection was reset` 阻断。当前桌面端已从组件工作树 MSVC debug 路径启动。

### 2026-08-24 GUI 状态控制器拆分增量
- 新增 `app-server-client.ts`：统一协议 v2 请求、Thread/Item 类型、历史行投影，以及按 Thread 的单调事件序号门禁；App 不再自行持有协议序号 Map。
- 新增 `conversation-model.ts`：统一工具组完成、思考长度上限、完成后隐藏工具活动和“每轮用户请求 + 最终结论”投影规则。
- 新增 `.svelte.ts` 控制器：侧栏尺寸/拖动、累计 Token/费用恢复、文件浏览、Git 分支/Diff、检查点、项目记忆。费用和币种不再散落在 App 状态中。
- `App.svelte` 从 1943 行降至 1474 行；新增控制器均通过结构门禁。Vite production build、GUI Rust 56 项和真实 protocol E2E 全绿。
- 尚未收口：设置/模型/插件编排、Thread/Turn UI 事件状态机、Composer/Slash 状态仍在 App；下一批继续拆，结构门禁仍未通过，不能宣告 GUI 单体改造完成。

### 2026-08-24 设置与插件控制器增量
- `PluginController` 统一 Harness 诊断、外部插件和 Codex 资源插件的发现、安装、升级、启停、卸载与 Marketplace 安装；所有 Codex 生命周期继续走 app-server 协议。
- `SettingsController` 统一设置加载/保存、配置文件入口、厂商模型目录、OpenRouter 刷新、模型预设及官方价格来源；模型预设继续同步快捷模型列表、Token 单价、币种和累计费用显示。
- `App.svelte` 从 1474 行降至 1210 行；两个新控制器均通过结构门禁。Vite build、GUI Rust 56 项、question E2E 和 protocol/Marketplace E2E 全绿。
- 下一批只剩核心状态拆分：Thread/Turn UI 事件状态机与 Composer/Slash；在 App 结构门禁通过前保持未完成状态。

### 2026-08-24 GUI 根组件与 Harness 控制器收口
- `App.svelte` 的 Composer、附件/排队/停止、右侧面板和应用启动/协议监听职责分别下沉到 `ComposerController`、`PanelController`、`AppRuntimeController`；根组件只保留控制器装配和视图绑定。
- `App.svelte` 已从改造前的 2674 行，逐批降到 2402、1943、1474、1210、762、591，最终为 297 行；所有本批生产文件通过结构门禁，没有修改门禁阈值或用生成标记规避检查。
- GUI 模板回归不再错误地只读取 `App.svelte`，而是按真实所有者读取 Composer、Runtime 和 Lifecycle 控制器；继续覆盖同 Thread 防重入、跨 Thread 并发、切换不停止旧任务、停止可重试和协议序号门禁。
- 拆分保持附件文件名展示、每会话累计 Token/费用恢复、设置/插件操作、工具活动完成态收口、历史最终结论投影和工作区面板行为不变。
- 最终验证：Vite production build、结构门禁、GUI Rust 56 项、question E2E、protocol E2E 和 Rust workspace 全量均通过；协议 E2E 覆盖并发、所有权、可见历史刷新/恢复以及插件 Marketplace。

### 2026-08-24 GUI 运行交互协议化增量
- 完成度审计发现 GUI 的 Thread/Turn 与插件虽已使用 app-server，但运行状态、Ready 重发、工作区切换、审批和用户问答仍直接调用 Tauri 命令；这与“GUI 是协议客户端、Tauri 只做宿主适配”的目标不一致。
- `ncx-protocol` 新增 `runtimeStatusRead`、`runtimeReadyRefresh`、`workspaceSet`、`interactionApprove`、`interactionAnswer`；`ncx-app-server` 统一拥有路由，GUI 只通过 `appServerRequest` 调用，Tauri 对应旧命令已从 invoke handler 删除。
- 审批和问答请求携带可选 ThreadId：真实会话必须与后端 pending owner 一致，避免旧会话 ID 误操作其他 Thread；仅无会话归属的调试问答使用 `null`，且只能匹配后端同为空归属的 pending 项。
- 验证：协议/App Server 定向测试、Rust workspace 全量、GUI Rust 57 项、Vite production build、真实 question E2E（选择、自由文本、取消）与原 protocol E2E 均通过；结构门禁和 `git diff --check` 通过。

### 2026-08-24 GUI 运行配置协议化增量
- 模型切换、权限模式、设置读取/保存、模型目录读取和模型预设应用改走 `ncx-protocol` → `ncx-app-server`；GUI 不再直连对应 Tauri 命令，旧 handler 已删除。OpenRouter 实时刷新保留为异步宿主网络 I/O，打开配置文件/价格来源仍是桌面资源 I/O。
- 设置页保存 payload 补齐此前遗漏的 `sandbox_mode` 与 `approval_policy`，与模型、思考程度、上下文限制和费用配置一并通过 `settingsUpdate` 持久化，再用既有 Agent 重建路径应用，不创建第二套配置状态源。
- 删除已经没有客户端的 `SetApproval`/`SetSandbox` bridge 命令及处理分支，避免运行配置继续存在协议外旁路。
- 验证：协议/App Server 路由单测、GUI Rust 58 项、Vite production build与真实 protocol E2E 均通过；E2E 实际打开设置页并继续验证 Thread 并发、所有权、历史恢复和插件 Marketplace。

### 2026-08-24 M6 插件控制面协议统一
- Harness 运行诊断和进程隔离外部插件的发现、安装/升级、启停已加入 `ncx-protocol` 与 `ncx-app-server`；GUI 插件页不再通过 Tauri 直连这组命令。
- 外部 `plugin.toml` 插件与 OpenAI `.codex-plugin/plugin.json`、Marketplace 现在共用一个 app-server 客户端边界；Tauri 只负责工作区目录、进程和网络等宿主实现，不再拥有 GUI 路由选择。
- 对应旧 Tauri handler 已删除，新增 App Server 路由单测与前端事实源回归，防止后续重新形成两套插件控制面。
- 验证：协议/App Server 10 项、GUI Rust 59 项、Vite production build、结构门禁和 `git diff --check` 通过；最终交付前仍需重跑真实 protocol E2E 与 workspace 全量。

### 2026-08-24 M4 Memory 控制面与组合测试加固
- GUI 的项目记忆列表、写入和合并改走 `ncx-protocol` → `ncx-app-server`，与 AgentLoop 消费的 Memory 服务共享控制边界；只有“用系统程序打开记忆文件”保留为桌面资源命令。
- `full`/`minimal`/`headless` 组合测试不再只检查诊断布尔值和 schema 数量：开始直接读取真实 Attachment、Media、MCP、CostTelemetry 服务，执行费用遥测记录，并断言 minimal/headless 未挂载这些服务。
- 对应旧 Memory Tauri handler 已删除；新增 App Server 路由单测和前端事实源回归。`AppServerAdapter` 已从超限的 `lib.rs` 提取为独立模块，没有放宽结构门禁。
- 验证：结构门禁、`git diff --check`、Vite production build 和真实 protocol E2E 通过；Protocol 4 项、App Server 11 项、Harness `full/minimal/headless` 真实组合测试、GUI Rust 60 项通过。

### 2026-08-25 M5 媒体 Skill 能力门禁
- `Skill` frontmatter 新增类型化 `capability`：`general`、`vision`、`image-generation`、`video-generation`；缺省保持 `general` 兼容旧 Skill，未知值按不支持处理，禁止因拼写错误误放行。
- `HarnessRuntimeBuilder` 成为 CLI/GUI 共用的能力装配入口：根据最终 Profile/Overlay 的 `ncx.media` 配置，在工具安装前同步过滤 `ToolContext.skills` 和 `skills` ContextFragment。`full` 可见媒体 Skill，`minimal`/`headless` 不再暴露媒体 Skill 或提示词目录。
- 这一步建立了生图/视频真实消费者门禁，但仓库仍没有已确认可执行的 Wan/生图 Skill 实现；不能据此宣告完整媒体执行链已完成。
- 验证：Rust workspace 全量通过（含 `ncx-core` 228 项、跨进程 Thread 租约）、GUI Rust 60 项、Vite production build、结构门禁和 `git diff --check` 通过。

### 2026-08-25 M5/M6 外部插件协议与阿里媒体执行链

- 外部 `plugin.toml` 插件不再只是启动子进程：新增 line-delimited JSON 协议 v1，包含 `handshake`、`toolCall`、`toolResult`，校验协议版本、插件 ID、能力、工具 schema 和参数对象；外部工具强制使用插件命名空间，不能覆盖 `shell` 等内置工具。
- 外部工具每次调用使用独立子进程、最小环境、超时终止；`HarnessRuntimeBuilder::configured` 会发现工作区 `.ncx/plugins`、完成握手并注册真实工具。`full` Profile 新增 `external` Bundle，`minimal` 与 `headless` 不加载外部插件。
- 新增阿里百炼 `DashScopeMediaProvider`、`generate_image`、`generate_video` 与两个内置 Skill；默认模型为 `wan2.2-t2i-flash` 和 `wan2.1-t2v-turbo`。只有 `full` 且存在第二套 `vl_api_key`/`DASHSCOPE_API_KEY` 时才装配，绝不回退使用 DeepSeek 主 Key。
- 媒体结果返回模型、任务 ID、真实 URL、币种、计价单位、价格来源、核对日期和本次预估费用，并写入 `CostTelemetryService`。当前内置估算为图片 `0.14 CNY/张`、视频 `0.24 CNY/秒`，分别可由 `NANOCODEX_IMAGE_PRICE_CNY`、`NANOCODEX_VIDEO_PRICE_CNY_PER_SECOND` 覆盖；由于当前网络未能重新打开官方价格页，不能宣称该价格已实时官方核验。
- GUI Harness 诊断新增图片、视频、外部工具就绪状态；设置页明确第二套密钥用于视觉与阿里媒体，媒体不复用主模型 Key；外部插件显示“协议 v1 · 正式能力握手”。
- 真实 Key 验收：从用户提供的密钥文件中脱敏读取两把 Key，分别请求阿里百炼原生媒体接口，均返回 HTTP 401 `InvalidApiKey`。因此代码执行链已完成，但付费 live 生图/视频成功验收仍缺少标准 DashScope Key；没有创建成功任务，也不应产生费用。
- Windows protocol E2E 首次因 MSVC 增量链接缓存损坏出现 LNK2019/LNK1120；使用 `cargo clean --manifest-path src-tauri/Cargo.toml -p ncx-core --target x86_64-pc-windows-msvc` 做包级清理后恢复，真实 E2E 通过：跨 Thread 并发、同 Thread 所有权、可见历史投影、刷新恢复、运行设置、Memory 和两类插件控制面全部正常。
- 最终验证：`cargo test --workspace --quiet` 全绿（`ncx-core` 234 项）；GUI Rust 60 项；`npm run build` 成功（141 模块）；真实 `npm run test:e2e:protocol` 通过；15 个目标生产文件结构门禁与 `git diff --check` 通过。

### 2026-08-25 Provider / Policy / ContextFragment 统一运行时装配

- 完成度审计确认：`ncx-protocol`、`ncx-thread-store`、`ncx-app-server` 和 OpenAI `.codex-plugin/plugin.json`/Marketplace 已有源码、单测与真实 WebView E2E 证据；剩余架构旁路是 CLI 主流程、GUI Agent 和并行 Runner 各自创建 Sandbox、Provider 与三段 ContextFragment。
- 新增 `ncx-core::ConfiguredHarnessRuntime`，成为 Provider、Policy、ContextFragment 的统一配置所有者；`RuntimeContextSources` 只接收项目说明、Skills、Plan 注入、Memory、Hooks、Genome，`RuntimeHostBindings` 只接收审批、用户问答和会话授权等宿主端口。
- CLI 主流程、GUI Agent、并行/无工具 Runner、后台会话标题和 Memory 摘要全部改走统一运行时；生产入口不再直接调用 `ToolContext::new`、`SandboxPolicy::new`、`DeepSeekProvider`、`install_llm_provider_factory` 或自行创建 `project_instructions`/`skills`/`plan_mode` Fragment。
- 工具完整与 reasoning-only 两条路径共享同一 Provider、Policy、Interaction、Context 服务契约；新增测试证明 plan 模式生成只读 Policy、上下文按稳定顺序装配、tool-less worker 仍拥有相同服务边界和正确模型路由。
- 验证：Rust workspace 全量通过（`ncx-core` 236 项）；`cargo clippy -p ncx-core -p ncx-cli --all-targets -- -D warnings` 通过；GUI Rust 60 项、Vite production build、真实 protocol E2E 和 question E2E 全部通过；生产入口搜索仅剩 GUI 测试夹具中的一次 `ToolContext::new`。
- 未完成的结构门禁债务：本批触及的 `rust/crates/ncx-cli/src/main.rs` 是既有 2191 行单体，结构检查仍报文件超过 700 行且 `run` 超过 80 行；没有调低门禁。后续应按参数/启动、Runtime 装配、Thread Recorder、REPL 命令和 MCP 生命周期继续拆分，不能据此宣告所有结构改造收口。
- GUI 严格 Clippy 仍被三个既有问题阻挡：`spawn_worker` 参数过多、历史投影可折叠 match、模型目录构造函数参数过多；本批 `cargo check` 无新增 warning，不能把 GUI `-D warnings` 宣称为通过。

### 2026-08-25 CLI 单体拆分收口

- `rust/crates/ncx-cli/src/main.rs` 从 2191 行降到 436 行，不修改结构门禁阈值；主文件只保留进程入口、REPL/Slash 调度、MCP reload 和 orchestrator 门面。
- 新增 `cli_app.rs`：把原 227 行 `run` 拆成配置加载、早退出/Memory 合并、Harness Runtime 构建、MCP 接入、Thread/Agent 恢复和单轮执行，所有函数低于 80 个逻辑行；Codex MCP 资源发现失败仍保持原行为并以退出码 1 终止，普通 MCP 加载/注册失败保持降级继续。
- 新增 `session_recorder.rs`、`usage.rs`、`command_support.rs`、`runtime_support.rs`，分别拥有 Thread Store 持久化与使用量、Slash/导出/配置渲染、Checkpoint/附件/Runtime 辅助逻辑；原内联测试迁入 `main_tests.rs`，测试内容和私有边界保持不变。
- 8 个受影响 CLI 生产文件全部通过结构门禁；`cargo clippy -p ncx-cli --all-targets -- -D warnings` 通过；CLI 35 项和 Rust workspace 全量通过（`ncx-core` 236 项）；`ncx --version` 与 `ncx --help` 真实命令输出正常。

### 2026-08-25 五层架构完成度审计

1. `ncx-protocol`：统一 Thread、Turn、Item、Event 与 GUI 控制请求；JSON 字段、版本、Thread/Turn 归属和单调事件序号均有往返测试。
2. `ncx-thread-store`：会话持久化和活动 Turn 所有权的唯一事实源；跨进程文件锁与 Thread 租约证明同 Thread 禁止重入、不同 Thread 并发、孤儿 Turn 可恢复。
3. `ncx-app-server`：拥有协议路由和可见历史投影；GUI 只通过 `app_server_request` 使用 Thread、Turn、运行设置、Memory 和插件控制面，剩余 Tauri 命令仅处理打开文件/URL、Git、目录读取、临时图片等桌面资源 I/O。
4. Provider / Policy / ContextFragment：`ConfiguredHarnessRuntime` 成为 CLI、GUI、Runner、标题和摘要的统一装配所有者；生产入口搜索不存在直接 Sandbox/DeepSeek Provider/固定 Fragment 拼装旁路。
5. OpenAI 插件与 Marketplace：兼容 `.codex-plugin/plugin.json` 的 Skills、MCP、Apps、Hooks、Interface 和官方/本地/Git/NPM Marketplace source；安装、升级、启停、卸载、路径边界和真实 WebView 协议调用均有测试。

- M5/M6 补充链路：附件、视觉、图片、视频、Cost/Telemetry、外部进程插件协议 v1 和 `full/minimal/headless` 组合均已进入同一 Profile/Bundle 与 app-server 诊断边界。
- 总体验证证据：`cargo test --workspace --quiet` 全绿，`ncx-core` 236 项；CLI 严格 Clippy 通过；GUI Rust 60 项；Vite build；protocol/question E2E；相对 `origin/main` 的 25 个生产文件结构门禁和 `git diff --check` 均通过。
- 外部验收限制不改变架构完成度：现有两把阿里 Key 均返回 401，尚不能证明付费 live 生图/视频成功；GitHub HTTPS 当前不可达且本机 SSH Key 无仓库权限，三个本地提交尚未同步远端。

### 2026-08-25 DSH Desktop 对话界面第一批对齐

- 参考仓库已独立克隆到 `D:\github_dgy\dsh-desktop-reference`，只用于视觉与交互对照，未把参考源码复制进 nanocodex，也未改变 Thread/App Server 状态所有权。
- GUI 已对齐 DSH Desktop 的桌面层级：低对比侧栏、工作区/会话分组、对话/轨迹页签、右对齐用户气泡、无卡片化助手正文、弱化的思考与工具轨迹、底部悬浮 Composer。
- 模型、DeepSeek 思考程度、权限模式、工作区、附件、停止和发送仍在 Composer 内；每会话累计 Token 与费用移到 Composer 下方独立状态行，功能没有删除。
- 会话切换不停止旧任务、不同 Thread 并发、历史只恢复用户请求与最终结论、已完成工具明细默认收起等现有协议行为保持不变。
- 验证：Vite production build 通过；GUI Rust 60 项通过；真实 protocol E2E 通过（并发、所有权、可见历史、刷新恢复、运行设置、Memory、插件）；question E2E 首次遇到已知 WebView2 页面发现竞争，清理残留进程后重跑 choice/free-text/cancel 全通过；浏览器真实页面完成 1280×720 深色视口检查；`git diff --check` 通过。

### 2026-08-25 DSH 单轮对话细节对齐

- 对照本机 `D:\deepseek-harness-master\deepseek-harness-master` 的官方 `ui-conversation`、`ui-tool` 和 Web E2E 快照，补齐消息级交互，不再只有外观参考。
- 用户消息与助手回答支持复制并短暂显示成功态；完成回答支持本地赞/踩状态；当前末条完成回答支持从该 Thread 分叉，运行中明确禁用，继续复用现有 app-server Fork 协议。
- 顶部“对话/轨迹”改为真实可切换视图。轨迹按当前前端实际拥有的事件顺序显示用户、回答、思考、工具、压缩与状态摘要；不伪造已被后端可见投影过滤的历史工具日志。
- Composer 下方统计对齐 DSH，显示本会话轮次、当前可见工具步骤、累计输入/输出 Token 和累计费用；原 UsageController 仍是 Token/Cost 唯一数据源。
- 未伪造当前协议没有提供的逐消息时间、TTFT、tok/s、缓存命中率和真实反馈上报。后续若需要这些字段，应先扩展 TurnUsage/Event 协议与持久化，再接 UI。
- 验证：Vite production build、GUI Rust 61 项、真实 protocol E2E、`git diff --check` 全通过；浏览器实际点击“轨迹”后确认切换到独立事件视图。桌面端从组件工作树重新启动。

### 2026-08-25 DSH 会话命名与侧栏操作对齐

- 对照 DeepSeek Harness `ui-workspace` 的 Session row、rename assembly 和 Web E2E：会话行改为整行打开、尾部单一省略号菜单；菜单提供重命名、分叉、打开日志和归档/取消归档，菜单操作不会误打开会话。
- 重命名使用正式模态框，预填现有名称、自动选中、限制 36 字符、折叠多余空白并在保存失败时保持弹窗和错误提示；写入继续走 `threadRename` 协议，当前顶栏和侧栏同步刷新。
- 内部占位标题 `(no prompt yet)` 只投影为“新会话”；空白 Thread 不显示时间与操作菜单。自动标题仍由首轮成功完成后的独立 Provider 任务生成，不进入串行会话命令队列。
- 分叉标题按 DSH 规则生成 `原名称 (1)`、`(2)`，并通过 `threadRename` 持久化；不会复制活动 Turn 所有权。
- 修复重命名暴露的历史恢复竞态：会话点击先读取 `threadReadVisible` 立即展示用户请求与最终结论，再激活后台 Agent；运行中 Thread 若已有内存消息则不被历史投影覆盖。
- 验证：Vite build、GUI Rust 62 项、真实 protocol E2E 全通过；E2E 现覆盖历史恢复、侧栏菜单重命名、名称持久化以及重命名后最终结论仍保留。

### 2026-08-25 新会话自动命名兜底

- 修复首轮完成后会话名称完全依赖模型生成的问题：现在先从首条用户请求生成本地短标题，经既有 `threadRename` 协议立即持久化并刷新顶栏/侧栏，再由独立后台 Provider 生成更自然的标题并覆盖。
- Provider 满载、断网、配置读取失败或返回无效标题时，本地标题继续保留，不再永久显示“新会话”；寒暄会映射为“日常问候”，普通请求会清理常见请求前缀、合并空白并限制长度。
- 对修复前已经落盘的占位标题增加历史投影兼容：侧栏从首条用户消息生成同规则短标题，因此旧会话刷新后也不再成片显示“新会话”；空白 Thread 仍保留“新会话”。
- 保持原有边界：仅首轮成功完成后命名，取消和失败轮次不触发；标题任务不进入会话命令队列，不阻塞其他 Thread 并发执行。
- 验证：新增 3 项本地标题单测；Vite production build、GUI Rust 65 项和真实 protocol E2E 均通过。protocol E2E 首跑出现一次重命名侧栏定位超时，独立复跑通过，未复现协议或持久化错误。

### 2026-08-25 空会话侧栏交互修复

- 修复已持久化但尚无首轮消息的空会话无法点击：侧栏打开操作不再受 `has_snapshot` 限制，空会话也可正常激活、重命名和归档。
- 分叉仍要求会话已有快照，避免创建没有上下文意义的空分叉；会话切换期间继续统一禁用打开操作。

### 2026-08-25 项目与会话目录层级

- 侧栏由全局“最近会话”平铺改为 `项目目录 → 会话` 两级结构；分组事实源使用 Thread metadata 的 `workspace`，不按标题或前端临时状态猜测项目归属。
- 每个项目目录可独立折叠，显示目录名和会话数量；当前工作区项目加深显示，会话缩进到对应项目下并保留打开、运行态、重命名、分叉、日志和归档能力。
- 已归档区域继续位于所有项目会话下方，符合此前确认的侧栏位置要求。
- Windows 工作区路径在投影时统一去除 `\\?\`/`\\?\UNC\` 扩展路径前缀、统一分隔符和盘符大小写；项目分组键忽略大小写，避免 `D:\github_dgy` 与 `\\?\D:\github_dgy` 被错误显示为两个项目。

### 2026-08-25 Composer 遮挡消息修复

- Composer 从覆盖滚动区的绝对定位改为主栏底部的独立 Flex 区域，保留圆角、阴影和半透明悬浮卡片外观，但其真实高度会参与布局；思考、工具和最终回答不会再落到输入框后面。
- 删除滚动区写死的 `9.5rem` 补偿，恢复正常底部间距，并关闭连续事件期间会产生追赶延迟的平滑自动滚动。

### 2026-08-25 设置中心整体重做

- 设置页从单列超长弹窗重构为桌面设置中心：固定标题栏、左侧分类导航、独立滚动内容区和固定保存区；配置项按“通用、模型与费用、连接与媒体、上下文、插件”五类呈现。
- 原有模型选择、官方/聚合目录、单价币种、主接口与密钥、视觉/阿里媒体连接、上下文裁剪、Harness 诊断、外部插件、Codex 插件和 Marketplace 均保留，没有另建配置状态源。
- 配置文件入口移到左侧底部；权限模式继续只由 Composer 控制。设置表单改为说明/控件双栏卡片，小窗口自动收窄为图标导航和单列表单。

### M7：DSH Community Marketplace 兼容层（已确认范围）

- 市场源：兼容 `dshfind`、DeepSeek 1024 Store 和 DSH standard HTTP v1；网络层必须限制 HTTPS、固定/清单声明 Origin、重定向终点、响应体大小、分页数量和缓存有效期，不能把任意 Provider URL 当可信安装源。
- 控制协议：新增目录搜索、插件详情、兼容性判定、Host 风险预览、安装/升级/启停/卸载、安装回执、失败回滚和需重启状态；统一进入 `ncx-protocol` → `ncx-app-server`，GUI 不新增 Tauri 市场旁路。
- 安装安全：Catalog 只提供候选身份；Host 必须再向 NPM Registry 核验精确包名/版本、仓库归属、完整性、Node engine、弃用状态、依赖和 lifecycle scripts。安装目标进入隔离 Profile，不执行市场返回的命令字符串。
- 兼容性分级：`native`（已有 `.codex-plugin/plugin.json` 或 `plugin.toml`）、`convertible`（可静态转换的 Skill/MCP/命令资源）、`ui-adapter`（依赖 DSH UI Slots，必须映射到 nanocodex 声明式槽位）、`incompatible`（依赖 Cordis/DSH 私有运行时、动态代码或未支持 Slot）。只有前三类可进入预览，`incompatible` 只能查看原因，禁止强装。
- UI Slots 首批映射范围限定为设置页插件标签、侧栏底部动作和 Shell Overlay；任何 React 组件不能直接注入 Svelte DOM，需通过类型化声明/资源协议投影。未映射 Slot 明确显示不兼容。
- GUI：设置中心“插件”分类增加 DSH 市场子页，提供源切换、搜索/筛选、详情、风险预览、安装状态、升级/停用/卸载和错误恢复；当前 Codex Marketplace 与外部进程插件继续保留。
- 审计事实：本机参考实现位于 `D:\github_dgy\dsh-desktop-reference\dsh-community-market`；其市场和安装流程不能直接复用现有 OpenAI Marketplace 安装器，后者缺少 DSH Catalog 快照、NPM 二次核验、安装回执和 UI Slot 运行时。

### 2026-08-25 M7 第一条可用链路

- `ncx-protocol`/`ncx-app-server` 新增 DSH 市场搜索、Host 风险预览和安装请求；GUI 设置中心插件页可切换 dshfind、DeepSeek 1024 Store、standard HTTP v1，输入搜索词并查看兼容性原因。
- 宿主网络限制为 HTTPS、固定或清单同源 Origin、禁止重定向、15 秒超时和 5 MiB 响应上限；真实访问 dshfind（约 110 KiB）和 1024 Store（约 2.5 MiB）均返回 HTTP 200。
- dshfind 明确标记 `is_risky` 的条目不会进入 GUI；只有 Provider 提供 verified、精确版本且不要求构建放行的 NPM 目标才进入“待核验”，其余直接标记不兼容。
- 风险预览会向官方 NPM Registry 二次核验包名/版本，检查 lifecycle scripts 与 `@deepseek-ai/dsh-*` 私有运行时依赖，并给出 `convertible`、`ui-adapter` 或 `incompatible`。
- `convertible` 安装会使用精确版本 `npm pack` 隔离暂存、再次核验解包身份；原生 Codex 清单直接安装，Skill/MCP/Markdown 命令资源生成受控 `.codex-plugin/plugin.json` 后进入现有 Codex Plugin Catalog，因此复用已有升级、启停和卸载能力。
- DSH React UI 包不会被执行或注入 Svelte DOM；当前依赖私有 DSH Runtime 的包明确不兼容。`ui-adapter` 仍需完成声明式 Slots 资源格式和三个 GUI 消费槽位，不能宣告 M7 全部完成。
- 2026-08-25 已完成 DSH React UI Slots 安全适配：隔离解包后只静态扫描 `settings.plugins.tab`、`sidebar.footer.action`、`shell.overlay`，总扫描量限制 4 MiB，生成 `.ncx/ui-slots.json`；GUI 启动时读取启用插件的受限声明，在插件设置页、侧栏底部和类型化 Overlay 中投影，第三方 React/JS 不会执行。未知 Slot、非 HTTPS 外链、超量声明、lifecycle script 和 DSH 私有运行时继续拒绝。
- UI Slot 插件现在可通过 `ui-adapter` 预览并安装；启停会同步控制声明式贡献。Rust 定向测试覆盖三槽转换和未知槽拒绝，protocol E2E 覆盖真实安装、重载后侧栏入口、打开/关闭 Overlay 及卸载清理。
- UI Slots 收尾补齐插件归属与稳定排序，设置页 Slot 以独立入口投影；重复 slot/id、HTTP 外链和无效声明会在插件设置中显示诊断，不再静默隐藏。真实 E2E 还覆盖停用后入口立即消失、重新启用后恢复。
- E2E 同时发现并修复协议 Artifact 被后到达的旧 `loaded` 快照覆盖的竞态：同一会话的协议产物现在会合并进旧桥接快照，不能被删除。
- 真实 protocol E2E 已通过 dshfind 搜索、选择带精确 NPM 目标的条目并完成 NPM 风险预览，同时继续覆盖 Thread 并发、历史投影、设置、Memory 和现有两类插件控制面。

### 2026-08-25 会话媒体产物链接

- `generate_image`/`generate_video` 成功结果中的 HTTP(S) URL 不再随工具日志一起隐藏：Bridge 将其提升为类型化 `ThreadItem::Artifact`，包含图片/视频类型、名称和 URL。
- 可见历史投影保留 Artifact，同时继续过滤普通工具调用、工具输出和思考；模型快照恢复路径也会通过 tool call ID 识别媒体工具并恢复产物，刷新和切换会话后链接不会消失。
- GUI 实时解析同一工具结果并显示独立产物卡片；图片带缩略图，视频显示播放入口，点击统一调用受限的 `open_url` 交给系统浏览器。助手 Markdown 中的 HTTP(S) 链接也拦截后走同一安全打开命令。
- 最终回答中以 Windows 本地绝对路径返回的图片、视频和 PDF（例如 `C:\Users\...\result.png`）也会被识别为本地产物卡片，点击后调用系统默认查看器；Host 会先确认文件存在并限制扩展名，禁止借回答路径打开 EXE、脚本或其他可执行内容。protocol E2E 覆盖刷新历史后本地产物卡片仍存在。
- 本地 PNG/JPEG/WebP/GIF/BMP 会由 Host 在 12 MiB 上限内编码为受限 `data:image/*`，直接在会话卡片内展示大图；SVG、视频、PDF 和超大图片仍只显示安全点击入口。Windows 点击使用原生文件关联（本机验证为“照片”应用），不再复用固定打开记事本的配置文件函数。E2E 已断言历史恢复后的 `<img>` 使用 `data:image/png;base64`。

### 2026-08-25 DSH Marketplace 分类目录

- 以本机 DSH Desktop 和 DeepSeek 1024 Store 实际接口为准，市场共 12 类：工具与能力、UI 增强、开发与运行时、会话与消息、模型与账号接入、工作流与自动化、技能包、通知与集成、娱乐、记忆、主题与外观、待分类。
- Host 现在保留 1024 Store 返回的 `categories`、`meta` 和每个插件的 `category`；dshfind/标准 HTTP 未提供目录时按结果动态聚合，非法或未知分类归入 `unclassified`。分类 ID、名称、数量均经过边界校验，市场任意文本不会直接成为界面标识。
- GUI 在市场来源和搜索框下增加横向可滚动分类导航，显示分类数量；分类与搜索结果同时生效，切换来源会回到“全部”，小窗口不会撑坏设置页。
- 真实接口证据：12 个分类、当前返回 500 个插件、`meta.catalogTotal=10619`；前 100 条中有 10 个 `memory` 插件。
- 验证：分类归一化 Rust 定向测试 4 项通过；Vite 构建通过（141 模块）；protocol E2E 通过，并真实断言 1024 Store 返回 12 类、点击“记忆”后只显示 `memory`、切回“全部”恢复其他插件；`cargo test --workspace --quiet` 全部通过。`cargo fmt --check` 仍会报告本轮之前累计的未格式化 Rust 改动，本次未批量格式化，避免扩大用户未提交差异。

### 2026-08-25 厂商实时模型目录与应用外观

- 修复选择厂商预设后模型下拉框只剩静态精选项的问题：设置读取会请求当前 OpenAI 兼容厂商的 `/models`，把安全、去重后的实时模型与当前模型、静态回退目录合并；5 秒超时、禁止重定向、1 MiB 响应上限，失败时不影响设置页使用。
- 使用当前 DeepSeek 官方接口和本机已配置密钥实测返回 `deepseek-v4-flash`、`deepseek-v4-flash-vision-exp`、`deepseek-v4-pro`，视觉实验模型现在会进入下拉框；API Key 不进入日志或前端。
- 用户提供的 1024×1024 猫图已保存为 `rust/gui/src/assets/nanocodex-cat-icon-source.png`，并通过 Tauri 官方图标生成器生成 Windows ICO、桌面 PNG、macOS ICNS、Appx、Android 和 iOS 尺寸。Windows 安装器和应用窗口继续引用 `src-tauri/icons/icon.ico`。
- Tauri 2.8 的窗口配置不支持 `app.windows[].icon`；任务栏图标改为在 `setup` 阶段读取编译进应用的 `default_window_icon` 并调用 `window.set_icon`，开发版窗口也显示猫图，不再沿用终端默认图标。
- 新增 `跟随系统 / 浅色 / 深色` 三种外观：设置页位置为“通用 → 外观主题”，顶部工具栏另有 `◐/☀/☾` 快捷切换；选择写入本地 `nanocodex.theme`，重启保留，不污染项目或会话配置。
- 验证：GUI Rust 80 项通过，Vite 构建通过（141 模块），`git diff --check` 通过；protocol E2E 已增加实时模型与主题应用/持久化断言。
- 品牌名已改为 `BugleCat`，对应“妙脆角猫咪”；窗口、安装包、开始菜单和侧栏展示使用新名称，内部 `nanocodex` 配置目录、协议 ID 和仓库地址保持不变，避免丢失现有设置与会话。
- Agent 系统提示加入 BugleCat 人设：温暖、好奇、可靠，跟随用户语言并先给结果；只在问候或庆祝时允许轻微猫咪风格，错误、风险、代码和技术说明禁止强行卖萌，准确、执行与验证优先于角色扮演。空会话页显示猫咪头像和“妙脆角猫咪准备好了”。
- 侧栏和顶部栏旧字符图标统一替换为 1.7px 线宽圆角 SVG：收起/展开面板、项目展开箭头和文件夹采用同一视觉语言；项目标题右侧原本不可点击的装饰字符改为“切换或添加工作区”和“折叠全部项目”两个真实按钮，并补齐中文 Tooltip 与无障碍名称。

### 2026-08-27 当前会话 Provider/模型安全切换

- 核实 GUI 的真实执行模型：每条 Prompt 都由 `spawn_turn_worker` 从已提交配置构建会话级 Agent；模型切换不需要重建整套 Harness、MCP、Skills 或 Thread。
- 自定义模型商激活和同厂商模型切换现在先通过 `ProviderCatalogService::validate_route_model` 校验候选 Route 的 Token、Base URL 及目标模型；失败不会修改 `providers.json` 或 `config.toml`，成功后才由 Provider Directory 一次提交完整 Route。
- `Command::SetModel` 改为提交后的状态刷新通知，不再二次写 `model`，不再调用 `build_agent`，不发 `Loaded`；当前会话 ID、可见消息、模型上下文和会话授权保持不变，下一条消息读取新 Route。
- Legacy 模型下拉若没有命中预设，也由调用方显式提交 `model` 后再通知，避免依赖 Worker 的隐式写配置副作用。
- 验证：Core Provider 定向测试 5 项通过；新增“目录只返回目标模型”测试证明错误模型校验前后 Provider 文件逐字节不变且 config 未生成；GUI 静态调用链测试证明 SetModel 分支不重建、不写配置、不替换会话，均通过。开发版已热重载到 `rust/target-codex-check/gui-catalog/debug/ncx-gui.exe`。
- 增加 `ProviderActivationGate` 代次门：每次选择先登记 generation，网络校验在锁外并行；提交时持短锁核对最新 generation。旧请求晚返回时闭包不会执行，最新请求失败时也不会让旧请求覆盖原活动 Route。定向并发测试通过。
- 新增聚焦 WebView E2E `npm run test:e2e:provider-switch`：真实创建并完成 Thread，经 App Server 调用当前活动模型切换，再逐字段比较切换前后可见投影并读取 Provider Directory 诊断。2026-08-27 实测输出 `providerSwitchE2e=true`、`transcriptPreserved=true`、`modelPreserved=true`；当时活动 Route 为 legacy 云末，因此没有把结果冒充为自定义 Provider 验证。
- 原全量 protocol E2E 已加入同类断言，但本次运行在既有大型 Marketplace 网络调用阶段遇到 `error decoding response body`，未形成全量通过证据；模型切换聚焦 E2E 已与该外部波动隔离。干净 MSVC 目标首次构建超过原脚本 120 秒 CDP 等待窗，缓存完成后聚焦用例通过。
- 回归：`ncx-core` 240/240、GUI Rust 87/87、Vite 143 modules 均通过。正常开发版恢复在 `rust/target-codex-check/gui-catalog/debug/ncx-gui.exe`（当次 PID 43424）。
- Provider 激活诊断现包含 generation、`idle/validating/active/failed`、更新时间和最近错误；过期 generation 不得覆盖最新状态。错误限制 300 字符，并二次脱敏 Bearer、`sk-`、`api_key=`、`apikey=`、`token=` 形态，结构中仍不包含 Token 字段。
- 设置 → 插件 → Harness 运行诊断新增“最近一次模型切换”卡片，失败状态显示脱敏原因。状态机/脱敏测试 2 项、GUI Rust 89/89、Vite 143 modules 通过；聚焦真实 WebView E2E 输出 `providerSwitchE2e=true`、`transcriptPreserved=true`、`modelPreserved=true`、`activationDiagnosticsVisible=true`。正常开发版恢复为 PID 40692。

### 2026-08-27 Provider 对话平面显式探测

- 新增独立 `ProviderChatProbeService` 和 Harness 插件 `ncx.provider-chat-probe`（服务 `provider.chat-probe`），与只读 `/models` Catalog 分离；不会在保存、刷新或切换时自动产生推理费用。
- 用户在自定义模型商卡片点击“测试对话”才发送真实的一 token 请求：OpenAI Compatible 使用 `/chat/completions`，Anthropic 使用 `/messages`；非流式、20 秒超时、禁止重定向、响应上限 256 KiB。
- 探测经 `ncx-protocol` 的 `customProviderChatProbe` 和 App Server Adapter 统一路由；成功返回请求模型、服务端确认模型（若存在）和协议，不返回正文。失败只暴露 HTTP 状态或安全连接错误，不读取/回显第三方错误正文和 Token。
- 设置页明确区分“获取模型（只验证目录）”与“测试对话（真实 1 token，可能有极小费用）”；目录激活成功提示也明确“目录可用不代表对话权限已开通”。
- Mock HTTP 测试覆盖 OpenAI/Anthropic 端点、鉴权头、1-token 请求、确认模型和 403 正文/Token 不泄漏；Provider 44、Core 240、App Server 12、GUI 89 项及 Vite 143 modules 通过。
- 真实 AIGoCode Route（未激活）探测：`/models` 包含 `gpt-5.6-sol`，但 Chat Completions 与 Responses 均 HTTP 403，服务端原因为账号仅允许 Codex 官方客户端；添加常见 Codex 请求形态/标识仍为 403。因此未切换当前 legacy 云末 Route，也没有把目录成功误报为聊天可用。聚焦 E2E 输出 `customProviderChatProbe=blocked`、`customProviderConfirmedModel=null`，会话/当前模型保持不变。

### 2026-08-27 Windows NSIS 全量安装包与插件冲突门禁

- 审计确认原 `tauri:build`/`tauri:installer` 固定使用 `x86_64-pc-windows-gnu`，但当前 Windows 工具链只安装 MSVC target；脚本已改为 `x86_64-pc-windows-msvc`，并通过修正后的 `npm run tauri:installer` 真实完成全量构建。
- 最终产物：`rust/target-codex-check/installer-audit/x86_64-pc-windows-msvc/release/bundle/nsis/BugleCat_0.1.0_x64-setup.exe`，大小 3,747,725 字节，SHA-256 `49F85FE63F3597F4EE2C1BA44CD0664BF6CE20C4DC87B1C80BED79C1123CB161`。
- 生产前端构建包含 143 个模块；`dist/assets/buglecat` 中 16 状态 × 4 尺寸的 64 张 PNG 全部存在。安装器版本/产品名为 `BugleCat 0.1.0`。
- 既有“重复能力阻断”是应用内 Codex 插件检查，不是 NSIS 检查，且此前只覆盖 DSH Marketplace。现在本地 Codex 插件、普通 Marketplace、DSH Marketplace和禁用插件重新启用均统一检查已启用插件的插件名、Skill ID、MCP server ID；升级仅跳过自身，仍会阻断与其他插件重复的能力。
- 新增功能测试证明不同插件包含同名 Skill 时安装被阻断，停用冲突插件后才放行；GUI Rust 90/90 通过，Vite 143 modules 通过，`git diff --check` 通过。
- 发布剩余风险：安装器尚未数字签名，Windows 会显示未知发布者；正式公开分发需提供代码签名证书并接入签名流程。开发版热重载后运行 PID 为 11928，路径仍是独立 `gui-catalog/debug/ncx-gui.exe`。

### 2026-08-27 GUI 壳层结构门禁

- 运行仓库真实 `scripts/check_code_structure.py` 复现 `App.svelte` 368 行超过 Svelte 300 行硬上限；CLI `ncx-cli/src/main.rs` 当前仅 436 行，低于 Rust 700 行上限，未做无依据拆分。
- 新增 `AppUtilityPanels.svelte` 作为工作区抽屉与设置中心的组合层，直接消费现有 Controller；状态所有权、App Server 边界和所有 `bind:` 语义保持不变。`App.svelte` 降至 265 行，组合层 94 行，两者均通过真实结构脚本。
- 验证：Vite 144 modules、GUI Rust 90/90、聚焦真实 WebView E2E 通过；输出仍为 `providerSwitchE2e=true`、会话/模型保留、激活诊断可见、AIGoCode 对话探测 blocked。正常开发版恢复 PID 8172。
- E2E 启动脚本此前强制 `--target x86_64-pc-windows-msvc`，在自定义 `CARGO_TARGET_DIR` 下另建 target 子目录并造成三轮冷编译；已改为项目正确启动命令 `npm run tauri -- dev`，复用正常开发缓存。
- 对全部当前改动运行结构门禁仍发现后续存量：`ncx-app-server/src/lib.rs` 714、其 tests 978（另有 103 逻辑行测试）、`ncx-config/src/loader.rs` 1477、`ncx-core/src/agent_loop/tests.rs` 1335、`ncx-core/src/tools.rs` 756、`ncx-protocol/src/lib.rs` 704。下一轮应按职责逐项拆分，不能把本次 App 通过冒充全仓通过。

### 2026-08-27 Rust 协议与工具结构收口（进行中）

- `ncx-protocol/src/lib.rs` 的协议契约与内联测试分离，公共文件 704 → 501 行；测试进入 `src/tests.rs`，序列化字段和公开类型不变。协议 5/5 通过。
- `ncx-app-server` 的 `DispatchOutcome`/`AppServerError` 进入独立 `outcome.rs` 并从 crate 根原样重导出，`lib.rs` 714 → 673 行；App Server 12/12 通过。
- `ncx-core/tools.rs` 的执行、保守恢复、Policy/Interaction 服务投影、Middleware 与 Hook 链进入已有 `tools/` 职责目录中的 `execution.rs`；公共 ToolRegistry API 不变，主文件 756 → 591，执行模块 160 行。Core 240/240 通过。
- 上述 6 个相关文件均通过真实结构脚本和 `git diff --check`。全改动结构门禁剩余：App Server tests 978（其中插件/市场测试 103 逻辑行）、config loader 1477、agent loop tests 1335；下一步按业务域拆测试，并按配置来源拆 loader。
- 当前开发版仍为 PID 8172；Tauri watcher 只监视 `src-tauri`，不会因依赖 crate 内部等价重构自动重启，完成下一批拆分后需主动重启以确保运行二进制来自最新全树源码。

### 2026-08-27 全改动结构门禁收口

- App Server tests 从单文件 978 行拆为共享 Runtime Fixture 336 行、Thread/Turn 182 行、Runtime Adapter 约 470 行；103 逻辑行的插件/市场用例拆为 Codex 插件与 Marketplace 两条独立测试。App Server 用例由 12 增至 13，13/13 通过。
- Config loader 从 1477 行拆为来源解析 `loader/sources.rs` 233 行、合并构建 `loader.rs` 585 行、测试 `loader/tests.rs` 659 行；DeepSeek/Codex/nanocodex 层级、环境变量、Provider Route、Hook 和阿里附件配置测试 36/36 通过。
- Agent loop tests 从 1335 行拆为共享 Fixture 271 行、基础回合行为 536 行、运行时/取消/调度行为 534 行；不扩大 AgentLoop 生产 API，Core 240/240 通过。
- `python scripts/check_code_structure.py --git-diff HEAD` 现对全部 41 个当前修改生产文件通过，`git diff --check` 通过；不再有此前记录的 7 个超限文件或超长函数。
- Rust 全 workspace 回归首次发现 CLI 旧测试仍假设只配置 `vl_model` 即启用视觉 Provider；生产实现和 Core 契约均已要求 `alibaba_attachment_parser_enabled=true`。测试已更新为同时断言“仅配置模型仍关闭”和“显式开启后可用”，保持附件解析插件默认关闭。修正后全 workspace 所有 crate 通过。
- Vite 生产构建 144 modules 通过。已主动重启 Tauri 以纳入依赖 crate 重构，最新开发版 PID 40788，路径 `rust/target-codex-check/gui-catalog/debug/ncx-gui.exe`。

### 2026-08-27 GUI Slash 第一批真实能力对齐

- GUI 命令面板新增 `/history`、`/review`、`/security-review`、`/verify`、`/docx`、`/pdf`、`/pptx`、`/xlsx`。历史命令直接打开现有 App Server 会话面板；审查、验证和文档命令展开为可编辑任务并保留用户在命令后的范围或文件参数，不冒充已经执行。
- 移除仅提示“规划中”的 `/schedule` 与 `/workflows` 可执行入口；真正的定时任务和多 Agent Orchestrator 仍需后续协议及运行态实现，不能用提示框或 Prompt 模板冒充。
- 修复带参数的内置命令会从菜单消失的问题：命令筛选现在只匹配 `/命令头`，例如 `/verify provider switch` 可被选择且参数完整进入任务。
- 新增聚焦 WebView E2E `npm run test:e2e:slash`，真实启动 Tauri 后断言 8 个可用命令存在、两个占位命令不存在、参数保留、PDF 模板展开和历史面板打开。实测输出 `slashParity=true`、`placeholdersHidden=true`、`argumentPreserved=true`、`historyPanelOpened=true`。
- 验证：Vite 生产构建 144 modules 通过；全量当前改动结构门禁对 42 个生产文件通过；`git diff --check` 通过。

### 2026-08-27 GUI Orchestrator 接入前置审计

- 当前不能只加一个“多 Agent”开关：Core Orchestrator 尚无结构化进度、合作取消或聚合用量，真实 Runner 仍由 CLI 私有；直接在 GUI 复制实现会形成第二套运行状态机，并破坏 App Server 的 Thread/Turn 所有权。
- `rust/gui/GUI_FEATURES_PLAN.md` 的旧 P4“toggle + shared snapshot”已替换为五道协议门禁：共享 Runner、Core 进度/取消/用量契约、App Server 执行模式、复用 GUI trajectory、Core/App Server/WebView 分层验证。
- 审计发现 CLI `LiveRunner` 在隔离目录复制失败时退回真实工作区；并行 Worker 因而可能同时修改用户目录，与其隔离承诺相反。现在改为 fail-closed：失败 Worker 返回明确 setup failure，不执行 Agent，也不登记残缺 scratch 目录。
- 新增定向测试证明源工作区缺失时不会创建或写入真实工作区，且 scratch registry 保持为空。CLI 定向测试 1/1、Core Orchestrator 13/13、`cargo fmt --check -p ncx-cli`、43 文件结构门禁及 `git diff --check` 通过。
- Core `AgentRunner` 新增向后兼容的结构化结果入口 `AgentCallResult`；旧 Mock 仍只需实现字符串 `run`，生产 Runner 可额外返回各节点 Token、请求模型、服务端确认模型和取消标记。`OrchestratorOutcome.telemetry` 对 classify/plan/worker/verify 全图聚合这些证据并对模型列表去重。
- CLI `LiveRunner` 已接入结构化入口，普通执行、无工具 reasoning 和隔离 Worker 都返回真实 `TurnResult.usage`、请求模型和 stop reason；未来 GUI 不需要再从最终文本猜测用量或型号。
- Orchestrator 的公开类型与纯解析/任务拼装分别拆入 `orchestrator/types.rs`、`orchestrator/support.rs`，主文件保持 689 行，新增文件 110/127 行。Core Orchestrator 14/14、CLI check、46 文件结构门禁和 `git diff --check` 通过。
- 新增 `OrchestratorControl`、`OrchestratorEvent` 与六种强类型阶段：Classify、Plan、Decompose、Workers、Verify、Promote。Host 可复用普通 Turn 的原子取消标记；Core 在每个节点边界和模型调用后检查，取消后不再调度后续 Worker、重试、Verify 或 Promote。
- `OrchestratorOutcome.cancelled` 与 telemetry 取消证据分开保留；CLI recorder 现在把取消编排写成 `TurnStatus::Cancelled`，不会误记为验证失败。定向测试在 Workers 事件触发取消，证明只发生 classify/plan 两次调用且无 Verify/Promote；正常事件顺序也有显式断言。
- 原 Orchestrator 内联测试机械迁移到 `orchestrator/tests.rs`，控制契约测试进入 `orchestrator/control_tests.rs`；高风险分解函数提取 `plan_and_decompose`。生产主文件 425 行，Core Orchestrator 16/16、CLI 36/36、GUI Rust check、47 文件结构门禁及 `git diff --check` 通过。

### 2026-08-27 App Server 执行模式协议

- `ncx-protocol` 升至 v3，新增 `ExecutionMode::{Agent, Orchestrator}`；`Turn.execution_mode`、`TurnStart.execution_mode`、`TurnSubmit.execution_mode` 使用 camelCase 且均以 `Agent` 为 serde 默认，旧 v2 JSONL/请求缺字段时可直接读取，不要求破坏性迁移。
- 模式从 GUI `TurnSubmit` 经 App Server Adapter、Bridge `Command::Prompt` 进入 `ProtocolTurnGuard::start` 并持久化在真实 Turn；不是只存于前端 localStorage。CLI 普通 Turn 写 Agent，`--orchestrate` 写 Orchestrator。
- TypeScript App Server client 与事件 gate 同步要求协议 v3；历史 `ProtocolThread` 暴露可选 `executionMode`。App Server 测试证明 Orchestrator 模式被路由到 Host 且存储后仍为 Orchestrator，Protocol 测试证明旧 Turn/Submit 自动回退 Agent。
- 回归：Protocol 6/6、Thread Store 12/12、App Server 13/13、CLI 36/36、GUI Rust check、Vite 144 modules 与 48 文件结构门禁通过。实际 Orchestrator Runner 分支尚未接入 Bridge，当前即使提交 Orchestrator 模式仍只持久化模式；下一步必须实现真实执行链后再开放 GUI 开关。

### 2026-08-27 GUI 真实 Orchestrator 执行链

- Core 新增共享 `HarnessAgentRunner`，CLI 与 GUI 统一复用 Configured Harness、Host bindings、取消检查、用量/请求模型/服务端确认模型聚合、Worker scratch 隔离与 fail-closed 清理；CLI 不再维护第二套私有 LiveRunner。
- Bridge 在 `ExecutionMode::Orchestrator` 下运行真实 `Orchestrator::handle`，沿用普通 Turn 的 Provider/Token/Base URL、审批/提问/授权、原子取消标志、Protocol Turn、会话上下文和标题生成；最终回答、模型证据、Token/费用与状态写回同一个 Thread/Turn。
- GUI Composer 增加 `Agent / 多 Agent` 执行方式。当前会话内切换不清空 Transcript；运行中切换仅影响下一轮；每条排队任务固定提交时的模式，避免出队时被新选择悄悄改写。
- 多 Agent 阶段 `classify/plan/decompose/workers/verify/promote` 投影到现有对话/轨迹状态，不另建平行运行状态机。界面明确提示多 Agent 更慢且调用更多；当前不支持图片附件，发送前和 Host 两层均 fail-closed，普通 Agent 继续承载原生多模态。
- 验证：Core Orchestrator 17/17、共享 Runner 1/1、App Server 13/13、CLI 35/35、Protocol 6/6、Thread Store 12/12、GUI 91/91、Vite 144 modules、48 文件结构门禁与 `git diff --check` 全部通过。真实 WebView 检查确认菜单文案、Agent→多 Agent 切换及 Transcript 保留；未触发付费模型调用。
- Windows 默认 Tauri 增量目录出现历史 LLVM 匿名符号链接缓存损坏；源码在 `CARGO_INCREMENTAL=0` 的独立目标目录完整链接并通过 91 项 GUI 测试。本地新版已从 `rust/target-codex-check/gui-orchestrator-test/debug/ncx-gui.exe` 启动；默认坏缓存未删除，避免破坏用户产物。

### 2026-08-27 Worker 三方差异提升

- 修复共享 Runner 的获胜结果提升只做 `copy_tree`、无法同步删除文件的问题。现在每个 Worker 创建后记录 SHA-256 基线，提升时比较“启动基线 / Worker 结果 / 当前真实工作区”，支持新增、修改、删除和空目录清理。
- 提升先检查全部变更路径；若真实工作区在 Worker 运行期间修改了同一路径，整批 fail-closed，不写入任何新增文件、不覆盖用户内容。失败原因进入 `OrchestratorOutcome.promotion_error`，并强制 `verify_passed=false`，GUI/CLI 不再把“模型验证通过但文件没提升”报告为完成。
- 全 Core 并行回归发现不同 `HarnessAgentRunner` 实例的 scratch 计数器各自从 1 开始，可能在同进程并行时使用同一路径并互删。现改为进程级原子唯一编号，所有 Runner 共用，不依赖随机碰撞概率。
- 测试覆盖新增/修改/删除、空目录清理、并发冲突零部分写入、提升失败状态、Runner 删除集成和 scratch 清理。Core 248/248、Orchestrator 18/18、Runner 2/2、CLI 35/35、GUI Rust check、Vite 144 modules、49 文件结构门禁及 `git diff --check` 通过。

### 2026-08-27 Worker 输出路径规范化

- Worker 在隔离目录运行时，模型最终文本可能引用 `%TEMP%\\ncx_worker_*`；scratch 清理后这些文件链接必然失效。共享 Runner 现在在结果进入 verifier 和最终回答前，把完整隔离根前缀映射为真实 workspace 根。
- 同时覆盖 Windows 反斜杠、模型常输出的正斜杠与 Windows 大小写不一致；只替换完整 scratch 根，不替换普通 `ncx_worker_*` 文本。获胜 Worker 提升后，GUI 的本地文件卡片和回答路径可继续打开真实文件。
- Runner 聚焦测试增至 4/4；全 Core 250/250、CLI 35/35、GUI Rust check、Vite 144 modules、49 文件结构门禁与 `git diff --check` 通过。

### 2026-08-27 多 Agent Worker 安全活动轨迹

- 共享 Runner 新增结构化 `HarnessRunnerEvent`，只在隔离 Worker 执行工具时报告 Worker 编号、工具名、开始/结束和规范化失败类别；分类、计划、验证节点不伪装成 Worker 工具活动。
- 原始工具参数、Shell 命令、文件内容、工具结果和第三方错误正文均不进入该事件，从事件契约层避免 Token/敏感信息泄漏。Core 测试用含 `api_key`/`token` 的输入证明观察者事件不包含秘密值。
- Bridge 将活动映射为 `orchestrator_activity`；GUI 只在“轨迹”视图显示 `W1/W2 + 工具 + 执行中/完成/失败类别`，并把同一工具的开始项原位更新为完成项。探索 Worker 活动不写 Protocol Thread、不进入聊天正文，Turn 完成后由现有结论收口逻辑移除。
- 回归：Runner 5/5、Core 251/251、CLI 35/35、GUI 91/91、Vite 144 modules、49 文件结构门禁与 `git diff --check` 通过。未用真实 Token 触发付费多 Agent 调用。

### 2026-08-27 Orchestrator WebView 协议恢复证据

- 新增聚焦 E2E `npm run test:e2e:orchestrator`，使用真实 Tauri WebView 和 App Server 创建 `executionMode=orchestrator` 的 Turn，写入用户要求与取消前结论，再持久化 `cancelled` 状态。
- 页面真实验证已有 Transcript 下 Agent→多 Agent 切换不改变任何会话文本；整页 reload 后从历史重新打开同一会话，回答仍可见；底层 Thread 读取确认 `executionMode=orchestrator` 与 `status=cancelled` 均保留。
- 实测输出：`orchestratorModeSwitch=true`、`transcriptPreserved=true`、`executionModePersisted=orchestrator`、`cancelledTurnRestored=cancelled`、`historyReloaded=true`。Core 的取消测试另行证明取消后不再调度 Verify/Promote；E2E 不触发真实付费模型。
- `GUI_FEATURES_PLAN.md` 的 P4 已从过期“current/尚未完成”修正为 DONE，并明确多 Agent 图片仍不支持，避免文档与运行代码冲突。

### 2026-08-27 完成后运行轨迹保留

- 修复“对话”和“轨迹”共用同一个消息数组，Turn 完成收口聊天时同时删除工具/Worker 证据的问题。`ThreadController` 现在为每个会话维护独立的最近一轮 trajectory；聊天继续只保留用户要求、结论和产物。
- 发送时记录本轮轨迹起点；运行中轨迹实时读取本轮切片；完成或失败时在移除 reasoning/tool activity 前复制轨迹。切换会话会 stash/restore 各自轨迹，不把 A 会话证据显示到 B 会话。
- 普通工具轨迹现在显示实际工具名（如 `shell · 已完成`），多 Agent 继续显示 `W2 / read_file · 完成`。原始参数与结果只在运行中的工具详情出现，完成后的聊天不泄漏。
- 聚焦 WebView E2E 通过 Tauri 真实事件总线注入与 Bridge 同形的阶段、Worker、工具和完成事件，实测 `completedTrajectoryRetained=true`、`chatKeptClean=true`；同时保留原有模式切换、协议持久化、取消恢复与历史 reload 断言。GUI 91/91、Vite 144 modules、49 文件结构门禁及 `git diff --check` 通过。

### 2026-08-27 多 Agent 资源预算

- Config 新增普通 Worker（1–4）、高风险 Worker（1–6）、验证重试（0–3）、递归深度（0–2）和子任务上限（1–12）；旧配置缺字段保持原默认 `2/3/1/1/6`，非法文件值在运行前 fail-closed。
- `OrchestratorConfig::from_runtime_config` 是 CLI 与 GUI Bridge 的唯一预算映射，替换两处固定 `Default`；设置保存后当前会话不重建，下一轮编排读取新预算。环境变量、profile、nanocodex TOML、writer 白名单和安全 redacted 投影均已贯通。
- 设置 → 通用新增“多 Agent 资源预算”卡片，明确每项范围与费用含义。聚焦 WebView E2E 验证五个控件真实可见且 HTML min/max 与后端校验一致，输出 `resourceBudgetControlsVisible=true`，未修改用户配置或触发模型调用。
- 验证：Config 38/38、Core Orchestrator 23/23、CLI 35/35、GUI 91/91、Vite 144 modules、49 文件结构门禁与 `git diff --check` 通过。

### 2026-08-27 多 Agent 预算保存前原子校验

- GUI 设置保存现在先在内存中校验五项预算，再调用配置 Writer；普通 Worker 1–4、高风险 Worker 1–6、验证重试 0–3、递归深度 0–2、子任务上限 1–12，非整数与越界值均返回明确中文错误。
- 新增真实临时文件回归：`orchestrator_workers=9` 和 `NaN` 都使包含其他合法字段的整批更新失败，原配置文件字节逐字节不变，证明不会部分写入或污染现有 Route/模型配置。
- 合法边界 `4/1/0/2/12` 一次性持久化；Writer 继续遵循现有“统一字符串落盘、Loader 归一为 bool/int”的配置契约，没有引入第二套序列化规则。
- 验证：Config 38/38、Core Orchestrator 23/23、CLI 35/35、GUI 93/93、Vite 144 modules、真实 WebView Orchestrator E2E 全部通过；E2E 仍确认模式切换、Transcript 保留、执行模式持久化、取消恢复、历史 reload、完成后轨迹保留、聊天干净及预算控件可见。49 个生产文件结构门禁与 `git diff --check` 通过。
- 验证后已用 `CARGO_INCREMENTAL=0` 和独立 `gui-orchestrator-test` 目标目录重新启动开发版；交接时 GUI PID `40716`，可执行文件为 `rust/target-codex-check/gui-orchestrator-test/debug/ncx-gui.exe`（PID 仅为快照）。

### 2026-08-27 Hermes LLM 记忆合并前置收口

- 扫描确认 GUI 当前 `memoryConsolidate` 只做确定性近重复删除；真实 LLM 合并此前由 CLI 私有 `LiveSummarizer` 实现。训练框架 `train/forge.py` 等已经存在于当前工作树，`GUI_FEATURES_PLAN.md` 中“需先合并 feat/train”的前置条件已过期。
- 新增 Core 共享 `ProviderMemorySummarizer`，由宿主注入当前 Harness Route 解析出的 `Provider`；提示词、结果清理和错误拒绝只有一套实现。CLI 仅负责选择 fast model 并装配 Provider，不再拥有第二套 LLM 合并逻辑。
- 注入测试验证两条模型消息、事实编号、输出去空白，以及 Provider 错误时返回 `None`，第三方错误正文不会写进记忆。模型合并器拆入独立 `memory_summarizer.rs`，避免 `memory.rs` 超过 700 行结构门禁。
- GUI LLM 合并仍未开放：同步 App Server dispatch 内直接等待模型会卡 UI，并且取消时可能留下部分写入。下一步应实现后台操作协议，在临时记忆副本上合并，取消或真实文件并发变化时不提升，成功后再原子替换；训练触发复用同一后台操作生命周期。
- 验证：Core 253/253、Core memory 聚焦 14/14、CLI 35/35、GUI 93/93、Vite 144 modules、50 个生产文件结构门禁与 `git diff --check` 通过；未调用真实模型或输出任何 Token。验证后已重启开发版，交接时 GUI PID `40556`，路径仍为独立 `gui-orchestrator-test/debug/ncx-gui.exe`。

### 2026-08-27 LLM 记忆合并草稿与冲突安全提升

- Core 将 LLM 合并拆为 `prepare_summarize_consolidate` 与 `commit_summarize_consolidate`：准备阶段从真实文件捕获字节基线并只在内存生成 `MemoryMergeDraft`，不写项目；提交阶段重新读取真实文件，仅在基线逐字节一致时写入结果。
- 用户或另一进程在模型运行期间新增/修改记忆时，提交返回 `WouldBlock`，整份草稿拒绝且并发内容逐字节不变。取消检查进入每个模型合并边界，返回 `Interrupted`，草稿不会提升。
- 原 CLI 同步入口复用 prepare/commit 兼容路径，因此也获得并发冲突保护。聚焦测试覆盖“准备零写入、成功提交、并发更改整批拒绝、取消零写入”。
- 合并算法和草稿生命周期从 `memory.rs` 拆入 `memory_merge.rs`，保持存储、模型 Consumer 和后台操作职责分离。验证：Core memory 17/17、Core 256/256、CLI 35/35、51 个生产文件结构门禁与 `git diff --check` 通过。

### 2026-08-27 GUI 后台模型记忆整理

- Protocol/App Server 新增 `memoryMergeStart`、`memoryMergeStatusRead`、`memoryMergeCancel`；启动立即返回 generation/status，模型调用在独立命名线程和 current-thread Tokio runtime 中执行，不阻塞同步 App Server dispatch 或 WebView。
- GUI 项目记忆明确拆为“快速去重”和“模型整理”。模型整理显示请求模型与运行状态，运行中提供取消；Controller 每 400ms 读取版本化状态，完成后刷新记忆，取消/冲突/失败均明确提示未写入。
- Coordinator 禁止并发启动第二个整理任务；取消通过 `tokio::select!` 丢弃正在等待的 Provider future，并由 Core cancellable prepare 返回 `Interrupted`。Provider 返回错误或空结果时 `failure_count>0`，GUI 严格失败，不再把启发式 fallback 误报为模型合并成功。
- 后台结果只提交 Core `MemoryMergeDraft`；真实文件基线变化进入 `conflict`，第三方错误正文不进入状态或 UI。Coordinator 测试覆盖 running→cancelling 与安全冲突错误映射。
- 验证：Config 38/38、Protocol 6/6、App Server 13/13、Core 256/256、CLI 35/35、GUI 95/95、Vite 144 modules、53 个生产文件结构门禁与 `git diff --check` 通过。真实 WebView E2E 输出新增 `memoryMergeControlsVisible=true`、`memoryMergeIdleStatus=idle`；未触发付费模型调用或修改用户记忆。
- 验证后已恢复本地开发版，交接时 GUI PID `2868`，可执行文件为独立 `rust/target-codex-check/gui-orchestrator-test/debug/ncx-gui.exe`。

### 2026-08-27 Forge GUI 接入安全审计与进程所有权

- 扫描真实 `forge.py --help` 和训练链确认可公开的安全参数仅为有界 `rounds/repeats/timeout/budget-s/teacher/accept-margin`；GUI 不应暴露 `--no-gate`、`--no-reeval`、`--from-genome`、任意 task/path 或原始命令行。原始 stdout/stderr 可能包含失败轨迹和第三方内容，也不能进入 UI。
- 新增 `train/process_control.py`，所有 Forge/Teacher/Evaluator/Genome/Export/Rollout/TaskGen 外部进程统一由 `run_owned` 创建独立进程组。超时时 Windows 使用精确 PID 的 `taskkill /T /F`，POSIX 使用进程组 kill；不再只终止直接子进程而遗留 ncx/教师/grader 孙进程。
- Forge 支持宿主通过 `NCX_FORGE_GENOMES_DIR` 与 `NCX_FORGE_RUNS_DIR` 指定隔离产物目录；未来 GUI 必须指向当前工作区 `.ncx/forge/`，不能把 lineage/genome 写回安装资源或源码 `train/`。
- 回归测试真实启动“父进程再生成延迟写文件孙进程”，触发 0.2 秒超时后等待，确认孙进程未存活且没有写 marker；另验证 stdout 捕获和宿主输出目录解析。全 `python -m pytest train -q` 43/43、Python compile、53 个生产文件结构门禁与 `git diff --check` 通过。
- 尚未开放 GUI 训练按钮：安装包尚未携带 Forge Python 资源，且 App Server 外层任务还需完整树取消、typed 参数校验、安全 lineage 摘要与成本确认。当前继续保持 pending，避免开发机可用但安装版失效的假入口。

### 2026-08-27 Forge 可重复安装资源

- `bench/run.py` 不再只认源码树 `rust/target/release/ncx.exe`；宿主必须通过 `NCX_FORGE_NCX_BIN` 注入 sidecar。二进制缺失时 `forge.py` 以退出码 2 fail-closed，不进入自检、评测或付费调用。
- 新增 Windows staging 脚本 `rust/gui/scripts/stage-forge-runtime.ps1`：固定 Python 3.13.7 官方 embeddable ZIP 和 SHA-256 `F6CCA216...D86B65`，独立 release 目标构建 `ncx.exe`，复制最小 Forge/Bench 资源并生成 `buglecat-forge-runtime/v1` 哈希清单。staging 目标严格限制在 `src-tauri/forge-runtime`，该生成目录已忽略。
- 新增 Tauri overlay `tauri.forge.conf.json` 与 `npm run tauri:installer:forge`；普通开发/测试配置不依赖生成资源，完整安装构建才携带约 27.5 MB 的 `python/bin/train/bench` 运行时。
- 真实 staging 通过：嵌入式 Python 能加载 `forge.py --help`，sidecar `--dump-genome` 返回非空 system prompt 与 37 个工具描述；二次 staging 复用缓存仅约 5 秒，且不产生 `__pycache__`。
- Tauri overlay 使用独立 `forge-bundle-smoke` 目标执行 `tauri build --debug --no-bundle`，5m39s 冷编译后成功；产物旁真实存在 `forge-runtime`，嵌入式 Python 可从复制后的资源运行。
- GUI 新增 `ForgeRuntimeStatusRead` App Server 请求。运行时优先读取安装资源目录，debug 才允许源码 staging fallback；逐项校验 Python、Forge 脚本和 ncx sidecar SHA-256，缺失、版本不兼容或篡改时只返回安全的 unavailable 原因。篡改 sidecar 测试确认 fail-closed。
- 验证：Python 44/44、Protocol 6/6、App Server 13/13、GUI 97/97；尚未开放训练启动，下一步实现有界 typed 参数、外层进程树 Coordinator 和安全 lineage 摘要后再生成完整 NSIS。

### 2026-08-27 Forge GUI 后台训练入口

- Protocol/App Server 新增 `forgeJobStart`、`forgeJobStatusRead`、`forgeJobCancel`，GUI Adapter 已接通安装资源发现、当前工作区与独立 `ForgeJobCoordinator`。后端再次校验 rounds 1–5、repeats 1–3、timeout 30–300 秒、budget 60–3600 秒、teacher 白名单和 accept-margin 1–3；输出固定到当前工作区 `.ncx/forge`。
- 项目记忆面板新增独立 Harness Forge 卡片：显示运行时状态、有界参数、运行状态、取消和安全结果摘要。开始前调用显式费用确认；按钮不会暴露 `--no-gate`、任意路径或任务参数，stdout/stderr 始终丢弃。
- lineage 只读取任务启动后生成、低于 2 MiB 的最新 JSON，并仅返回轮次、接受数、冠军/测试分数和报告文件名；trajectory、diff、Token、第三方错误正文不会进入 UI。
- Windows 真实逃逸测试证明单用 `taskkill /T` 无法约束主动脱离的孙进程，因此 Coordinator 改为每次任务创建 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` Job Object；无法取得进程所有权时 fail-closed。测试现在证明取消后延迟写 marker 的孙进程不能存活。
- 修复 PowerShell 5 staging 生成 UTF-8 BOM 清单导致完整运行时误报“清单无效”：读取端兼容既有 BOM，staging 端以后固定无 BOM；BOM、有效清单和篡改 sidecar 均有测试。
- 验证：Protocol 6/6、App Server 13/13、GUI 100/100（BOM 测试加入后聚焦模块累计 101 项）、Vite 146 modules。真实 Tauri WebView E2E 输出 `forgeRuntimeAvailable=true`、`forgeJobIdleStatus=idle`、`forgeBoundedControlsVisible=true`、`forgeCostConfirmationVisible=true`，且没有发送 `forgeJobStart` 或产生付费调用。

### 2026-08-27 Forge 全量 Windows NSIS

- `npm run tauri:installer:forge` 已在独立 `forge-installer-release` 目标完成真实 x64 release + NSIS 构建。首次通过 npm/cmd 调用时 Windows PowerShell 偶发且可复现地无法解析 `Get-FileHash`；staging 现改用内置 .NET `SHA256` API，不依赖 PowerShell 模块自动加载，一键命令随后完整热重跑成功。
- 最终安装包：`rust/target-codex-check/forge-installer-release/x86_64-pc-windows-msvc/release/bundle/nsis/BugleCat_0.1.0_x64-setup.exe`，14,369,781 bytes（13.70 MiB），SHA-256 `A5A97B0A24E386F9B88D0BC43684FB91B34D5A9B2304D9A946579A2A1F64EFC6`。
- NSIS 生成脚本包含 85/85 个 `forge-runtime` 文件项；release 资源目录同为 85 个文件。Python、ncx sidecar、Forge 脚本三项 SHA-256 与 manifest 全部匹配，manifest 无 BOM；嵌入式 Python smoke 返回 0，sidecar genome 含非空 system prompt 和 37 个工具描述，资源树无 `__pycache__`。
- 安装包当前 `AuthenticodeStatus=NotSigned`。这是可安装的完整产物，但 Windows SmartScreen 仍可能提示未知发布者；正式分发仍需代码签名证书，不应把未签名状态描述成已签名发布版。

### 2026-08-27 会话级 Harness Profile

- 对齐 DeepSeek Harness 的会话级 Preset：Protocol `ThreadMetadata` 持久化 `harnessProfile`，旧会话缺字段时兼容为 `full`。GUI 提供全功能、编程、只读、轻量、自动化五种组合。
- Profile 只属于 Thread，不通过进程环境变量热切换。新空会话可修改并立即重建当前 Agent；首个 Turn 创建后，App Server 强制拒绝再次修改。Resume 读取持久化值，Fork 继承源 Thread 的值，普通 Agent 与 Orchestrator Worker 使用同一 Profile。
- `threadCreateActivate` 在落库前调用宿主验证，未知或无法装配的 Profile fail-closed，不留下半创建 Thread。`threadHarnessProfileSet` 同样先验证，并且只允许无 Turn 的 Thread。
- 验证：Protocol 7/7、App Server 15/15、Core 256/256、Thread Store 12/12、GUI 101/101、Vite 146 modules。真实 Tauri/WebView E2E 证明 GUI 选择持久化、空 Thread 可切、首轮后锁定、Fork 继承；未触发模型或付费请求。

### 2026-08-27 持久 Goal 真实桌面续轮 Worker

- `goalResume` 不再只显示 armed：App Server 通过新增的 `continue_goal` 宿主边界，把精确 Thread 交给桌面调度器；队列拒绝时立即撤销进程内 activation，durable Goal 保持 active/disarmed，可由用户安全重试。
- 新增 `bridge/goal_turn.rs`。每轮先执行 `GoalRoundDriver::reserve_next` 的完整 checkpoint/admission，再按当前 Provider Route、Harness Profile 和权限装配 Agent，通过 exact Goal ID/revision/round 调用 `run_goal_round`。模型上下文先持久化，Turn 后完成；Goal 仍 armed 才继续下一轮。
- Goal 隐藏提示继续只进入 ModelContext，不进入可见 Transcript；助手消息、工具活动、usage 和费用继续写入既有 Protocol Turn，不增加平行会话状态。模型在普通人工 Turn 中显式 resume 后，人工 Turn 提交完成会检测 activation 并启动同一 worker。
- 用户输入现在优先于自动轮次：Backend 在持有 run-state 锁时原子放入一条 deferred human prompt，取消并暂停已 admitted 的 Goal round；释放会话 lease 后立即在同一会话执行用户输入。GUI 为 Goal run 和接续的人类 Turn 分别维护 running 事件，发送不再只报“会话仍在执行中”。
- 安全失败路径不把内部错误或 Provider 正文放进 UI；checkpoint、Agent 构建、上下文保存、Turn 完成和线程创建失败均 disarm/block，并使用固定中文错误。轮数上限由 Driver 在模型调用前 block。
- Worker 生命周期已归宿主管理：协调线程、普通 Turn 和 Goal Worker 的 `JoinHandle` 全部登记；退出时拒绝新任务、广播取消、发送 Shutdown 并 join。`shutdown_cancels_and_joins_every_owned_worker` 已覆盖，不再遗留 detached Goal 线程。
- 新增真实 `npm run test:e2e:goal-worker`：临时隔离 HOME、localhost OpenAI SSE fixture、placeholder key，全程无外网和真实 Provider。实测 2 个 admitted rounds、2 个 durable completed Turns、4 次固定模型请求、exact `get_goal`/`update_goal complete`，GoalMessage 未进入 visible history，两条助手结论均在侧栏恢复后的 WebView 页面可见。
- E2E 同时发现并修复真实恢复缺陷：侧栏 Resume 原先会先装载 Protocol Thread，随后被异步 legacy `loaded` snapshot 覆盖，导致较新的持久 Goal 回复从 UI 消失。现在激活时只抑制这一个兼容事件，并以 `threadReadVisible` 作为最终权威；旧 snapshot 不再吞掉新 Turn。
- 最新验证：App Server 28/28、Config 39/39、GUI Rust 107/107、Core 跨 Profile Goal 工具定向测试、Vite 147 modules、Goal Worker WebView E2E 全部通过；未调用真实 Token/Provider。持久 Goal 桌面 Worker、用户输入抢占、退出 join 和无付费真实 UI 续轮这一批已收口；整体 Harness 对齐 Goal 继续保持 active。

### 2026-08-27 Provider Route 与模型来源真实性

- 审计确认实际请求链每个 Turn 都从当前完整 Provider Route 重建 Agent；运行态切 Provider/Token/Base URL/模型不替换 Transcript，当前运行中的 Turn 保持原 Route，下一轮读取新 Route。
- `ThreadItem::AssistantMessage` 已持久化请求模型和 API 响应 `model` 字段，实时 UI 也能显示；但侧栏恢复映射此前只恢复正文，丢失 `model/confirmedModel`。现已补齐，因此重启或切回历史会话后仍能看到请求值与响应值。
- 文案由“响应确认”改为“响应字段”，tooltip 明确：中转站返回或回显的 `model` 字段不等于证明上游内部型号。模型自己的“我是 GPT-5/5.6”回答不作为路由证据。
- localhost WebView fixture 故意请求 `mock-goal-model`、响应 `mock-confirmed-model`；实测 4 个 HTTP 请求体始终使用请求型号，Protocol durable/visible Turn 保存两值，侧栏 Resume 后页面显示“请求 … → 响应字段 …”。无外网、无真实 Token。
- 验证：GUI Rust 108/108、Vite 147 modules、扩展后的 Goal Worker WebView E2E 全部通过。

### 2026-08-27 Composer 当前 Provider Route 可见

- `UiEvent::Ready` 现在把 `provider_id/provider_protocol/model` 作为同一运行态快照发送；前端不再另查一套可能滞后的设置状态。Provider Route 原子提交后的 `SetModel` 继续只刷新快照，不重建 Harness、不替换当前 Transcript。
- Composer 模型按钮直接显示“Provider + 模型”，菜单头显示 Provider ID 和协议；运行中仍允许切换，tooltip 明确当前 Turn 使用旧 Route、下一轮使用新 Route。这样同名 `gpt-5.6-sol` 来自云末、OpenAI 直连或自定义中转时不会再混淆。
- 无网络 WebView E2E 在隔离 HOME 中真实创建并激活 `goal-e2e-relay` OpenAI Route，`/models` 和 4 次对话只访问 localhost；页面确认 Provider、协议、请求模型三项可见，随后两轮 Goal 继续使用该 Route。
- E2E 轮询同时收紧为 Goal complete 且两个 Goal Turn 均 durable completed，消除 `update_goal complete` 先于最终 Turn 提交造成的偶发测试竞争。
- 验证：GUI Rust 109/109、Vite 147 modules、Goal Worker WebView E2E 输出 `composerProviderProtocolModelVisible=true`；无真实 Token/外部 Provider。

### 2026-08-27 Composer 跨 Provider Route 快捷切换

- 模型菜单现在从 App Server `customProviderList` 读取所有已配置 Token 且有模型的 Route，按 Provider 分组，当前 Provider 置顶；按钮优先显示用户配置的友好名称，稳定 ID 仍保留在诊断中。点击其他分组调用既有 `customProviderActivate`，因此切换的是 Provider/Token/Base URL/协议/模型整套 Route，不是只替换同名模型字符串。
- 切换成功立即更新 Composer，并由后端 `ready` 快照最终校准；当前会话 Transcript 不变，运行中的 Turn 保持旧 Route、下一轮使用新 Route。失败时前端恢复完整旧状态，后端 activation gate 保持旧 Route，并显示“当前 Route 未改变”。
- 内部兼容字段 `active_provider_id=legacy` 不再泄漏到 UI：预设根据维护目录的 Base URL 显示 `yunmo/openai/openrouter/deepseek/...`，未知手动地址显示 `manual`，自定义 Route 显示稳定 ID。
- 无网络 E2E 创建 primary/backup/invalid 三个 localhost Provider，primary 与 backup 使用相同 `mock-goal-model`，从 Composer 切换后诊断明确变为 backup，证明不是靠模型 ID 猜 Provider；两条历史回复保持可见。invalid 的模型不在 `/models`，切换被拒绝且 backup Route 保持不变。
- 验证：GUI Rust 110/110、Vite 147 modules、Goal Worker WebView E2E 输出 `composerCrossProviderSwitchPreservedTranscript=true` 与 `failedProviderSwitchKeptRoute=true`；无外网和真实 Token。

### 2026-08-27 已配置预设 Provider 与安全计价

- Composer Route 候选现在合并三类来源：Provider Directory 中有 Token/模型的自定义 Route、当前已激活 Route，以及拥有独立凭据的预设 Route。DeepSeek 仅在保存 DeepSeek Key 时出现，云末仅在保存云末 Token 时出现；没有可用独立凭据的目录卡不会伪装成可切换项。
- 预设切换复用 `modelPresetApply`，一次提交 endpoint、model、快捷模型、官方/聚合价格和币种；自定义 Route 继续复用 `customProviderActivate`。两条路径都经过原 activation generation/CAS，失败不改变 Route。
- 修复费用真实性：自定义中转站没有可信目录单价，`ProviderDirectory::activate` 现把 `price_in/price_out` 与完整 Route 一起原子写为 0，避免继承上一家 Provider 的费用估算；前端切换后立即读取提交值刷新估算器。
- 无网络 E2E 通过占位 `NANOCODEX_DEEPSEEK_API_KEY` 证明 DeepSeek 分组出现，但没有点击或访问官方端点；实际成功/失败切换仍只访问 localhost，并断言自定义 Route 价格归零。
- 验证：Config 39/39、GUI Rust 110/110、Vite 147 modules、WebView E2E 输出 `configuredPresetProviderVisible=true` 与 `customProviderPricingResetToUnknown=true`。

### 2026-08-27 预设 Provider 独立凭据收敛

- 预设切换不再回退到 `active_provider_id=legacy`，统一保存为保留命名空间 `preset:<provider-id>`；Route 独立拥有 Token、协议、Base URL、模型目录和选择，避免继承上一家模型商的通用 Key。
- `ProviderDirectory::upsert_and_activate` 同时提交 Route 与兼容配置快照；配置写入失败会恢复旧 Provider Directory。预设保留目录计价，自定义中转继续归零为未知价格。
- DeepSeek/云末旧独立 Key 在首次选择相应预设时自动迁移进 Route；已有 Route Token 优先。云末动态 `/models` 同时接受迁移后的 `preset:yunmo` Token。
- 设置页官方厂商卡新增独立 Token 输入与脱敏状态；内部 `preset:*` Route 不在“拓展模型商”重复展示。Composer 将其识别为预设 Route，显示友好厂商名并用真实 catalog ID 切换。
- 新增迁移测试覆盖 DeepSeek 与云末旧 Key、脱敏和最终激活 Route；Config Provider Directory 5/5。扩展 Goal Worker WebView E2E 实际填写 Token、检查 `****-key`、提交 DeepSeek、故意拒绝无 Token 云末并确认 Route/Transcript 回滚。
- 最新验证：`cargo test --workspace` 全量通过，GUI Rust 110/110，Vite 147 modules，Goal Worker E2E 输出 `presetTokenEntryInteractive=true`、`legacyPresetCredentialMigratedToRoute=true`、`presetSwitchFailureRolledBack=true`、`presetSwitchPreservedCurrentTranscript=true`；`git diff --check` 通过，仅有工作树既有 LF/CRLF 提示。

### 2026-08-27 Composer 只显示当前供应商

- 按用户界面反馈，Composer 模型菜单暂时只展示当前已选供应商及其模型；其他已配置但未激活的供应商不再整组占满下拉列表。
- Provider 的发现、Token 配置和激活仍由设置页负责；过滤只影响紧凑模型菜单，不删除 Route、模型目录或凭据。
- 同时处理 `provider-id` 与 `preset:provider-id` 的兼容身份，并优先选择精确当前 Route，避免同一供应商重复显示。

### 2026-08-27 归档会话按工作区分组

- 侧栏“已归档”展开后不再平铺全部记录，而是复用普通会话的工作区分组：文件夹名称、会话计数、独立折叠状态及缩进会话列表保持一致。
- 分组仅改变展示结构；归档、取消归档、恢复、重命名、分叉和日志入口继续复用原会话项，不改变持久数据。
- 验证：Vite 147 modules，GUI Rust 110/110，`git diff --check` 通过（仅既有 LF/CRLF 提示）。

### 2026-08-27 Windows NSIS 安装包

- 使用 `npm run tauri:installer` 完成 x86_64 Windows release 与 NSIS 打包；正式程序嵌入 `dist`，不依赖 Vite localhost，且 release 构建使用 Windows GUI subsystem、不显示调试控制台。
- 产物：`rust/gui/src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/BugleCat_0.1.0_x64-setup.exe`，3.73 MiB，SHA-256 `2C62D6433F12C4B472D783913A2402BE3AB3C63D5F04592547BE4F20FE43DD4C`。

### 2026-08-27 Composer 控件收敛与会话恢复不卡队列

- `Agent / 多 Agent 编排` 与当前会话 `Harness Profile` 已从 Composer 移到设置 → 通用；思考程度、权限模式继续留在输入区。执行模式仍由下一轮 Turn 读取，Harness 仍按 Thread 持有并在首轮后锁定。
- 修复恢复历史会话时同步等待 `build_agent` 会阻塞主命令队列的问题。Resume 现在只恢复 Thread、工作区、运行态快照和可见历史；真正的工具、插件与 Provider Route 仍由每轮 Worker 按当前配置构建。
- 验证：Vite 147 modules、GUI Rust 110/110、`git diff --check` 通过。重新构建并启动无控制台 release EXE 后，实际 WebView 确认原卡住会话恢复为空闲状态，侧栏不再显示“执行中”，Composer 不再出现 Agent/Harness 两个低频控件。
- 后续按最终界面标注调整 Composer 分组：主工具栏左侧为附件与权限，长期目标（存在时）同属左侧；模型、思考程度和发送/停止固定在右。工作区单独放到 `＋` 下方的辅助栏，不参与主工具栏换行，因此不撑高输入框。发布版实际 WebView 已确认 Agent/Harness 不再出现且分组生效。
- 最新布局与会话恢复修复已重新打入 NSIS：`BugleCat_0.1.0_x64-setup.exe`，3,915,694 bytes，SHA-256 `C5406375D5F9DE9D90525D8E31EEE835A49186550E19747AB106EE9B860B69DE`。

### 2026-08-27 预设 Route 模型目录自动同步

- Composer 不再直接沿用旧 `preset:*` Route 的模型快照；读取 Route 候选时会用当前维护目录同步模型列表。DeepSeek 从旧两款自动补齐为三款，并保持 Token、当前选择、协议、端点和 Route ID 不变。
- 仅同步已有预设 Route，不触碰自定义模型商；当前 Route 同步时同时更新兼容配置的 `available_models`。若当前选择已被新目录移除则安全拒绝，不静默换模型。
- 验证：Config Provider Directory 6/6、GUI Rust 110/110、Vite 147 modules、`git diff --check` 通过（仅既有 LF/CRLF 提示）。
- 新 release 已实际启动并把本机 `preset:deepseek` 从旧两款同步为三款；NSIS 安装包 3,916,242 bytes，SHA-256 `49E6416BD717081CEC6FB965AB110709E1BE4D870E1B34CF417BF360858E3133`。

### 2026-08-28 工具后模型流停滞保护

- OpenAI Compatible SSE 在工具返回后若已输出部分内容、随后连续 30 秒没有实际模型进展，现在会结束当前 Turn、保留会话并给出可重试提示，不再长期占用会话运行态。中转站的空白、SSE comment 和 JSON ping 心跳不再重置期限；完全未输出时仍保留既有安全重试。
- 可通过 `NANOCODEX_STREAM_IDLE_TIMEOUT_S` 在 5–120 秒内覆盖，默认 30 秒；响应头等待和整次请求超时保持原有独立边界。
- 验证：Provider 46/46（新增心跳不计进展、工具参数计进展）；Agent Loop 挂起模型与挂起工具停止路径通过。两项并行 Hook 测试受 Windows 命令调度超时影响，串行复跑均通过。

### 2026-08-28 原生多模态与附件插件即时生效

- 修复图片发送层错误依赖 Harness Profile 的附件服务：`gpt-5.6-sol` 等原生多模态模型现在可直接接收 data URL 图片块，不再报“未启用附件插件”。Harness 附件服务存在时仍可收紧扩展名和大小策略，缺席时使用 20 MiB 图片安全默认值。
- 阿里附件解析仍只是非原生多模态模型的可选 fallback。GUI 每一轮都重新加载已保存配置并组装 Agent，因此开启并保存后在当前对话的下一轮生效，无需新建或切换对话；设置页文案已明确该行为。
- 验证：GUI 回归 `native_image_transport_does_not_require_harness_attachment_service` 与 `image_attachment_requires_an_explicit_parser_model` 通过；Vite 147 modules 构建通过；本次文件 `git diff --check` 通过。全仓 `cargo fmt --check` 仍被既有 `ncx-dreamina-gateway` 格式差异拦截。

### 2026-08-28 LLM Wiki 本地 MCP + Skill

- 新增轻量本地插件源 `local-plugins/llmwiki-memory`：MCP 直接运行 `D:\LLMWIKI\wiki_mcp.py`，只暴露单一 `llmwiki(action=...)` 工具；Skill 规定 `D:\LLMWIKI` 是实际记忆库，`D:\llm-wiki-template` 只是协议/初始化模板，并保留用户批准、敏感信息和项目事实源边界。
- Codex 兼容插件现在统一发现用户全局 `~/.ncx/codex-plugins` 与当前工作区 `.ncx/codex-plugins`，工作区同名插件覆盖全局。MCP、Skills、Hooks 和 Apps 共用这一规则，因此切换 BugleCat 工作区后 LLM Wiki 仍可用。
- 插件已安装到 `C:\Users\25376\.ncx\codex-plugins\llmwiki-memory`。`wiki_mcp.py --selftest` 的 recall_user/recall_project/project_status/corpus/status 全部通过；Skill 与插件结构校验通过；新增全局发现与工作区覆盖回归通过。

### 2026-08-28 运行态活动收紧与 MCP 隐藏窗口

- 聊天页运行中只显示当前思考或正在执行的工具；本轮已完成的 Skill/工具行不再继续堆在输入框上方。完整思考与工具历史仍保留在“运行轨迹”页；当前活动区限高并内部滚动，不再持续顶走对话。
- `ncx-mcp` 在 Windows 启动所有 stdio MCP sidecar 时统一使用 `CREATE_NO_WINDOW`，Python/Node MCP 不再弹出黑色控制台窗口。
- 验证：MCP 4/4（包含真实 mock server 握手、列表与调用）、GUI Rust check、Vite 147 modules 和本次 `git diff --check` 通过。

### 2026-08-28 LLM Wiki 混合工具权限分类

- 修复单一 `llmwiki(action=...)` MCP 入口被工具名启发式整体判为写操作的问题。`recall_user`/`recall_project`/`project_status`/`status`/`corpus` 现在按只读调用处理，在 `approval_policy=never` 下可直接执行。
- `initialize_project`/`record_project`/`propose`/`approve` 和未知 action 仍按有副作用调用处理，`never` 下继续拒绝，不放宽长期记忆写入边界。
- 真实 Python mock MCP 回归已覆盖 `never + recall_user` 通过与 `never + record_project` 拒绝；MCP Tool 3/3 通过。


### 2026-08-30 全仓测试专项扫描：全绿之下的测试缺陷清单

- 扫描基线：`cargo test --workspace` 全绿（ncx-core 270、ncx-provider 46、ncx-video-agent 57、
  thread-store 17 等），GUI Tauri 后端 114/114，Python `pytest tests/` 601/601 无 skip。
  当前工作区（feat/deepseek-harness-components 未提交改动）没有现成的失败用例；以下问题
  都是"能过但测不对 / 会间歇性误报 / 覆盖被悄悄拿走"的类型，重点是前两条。
- **[P1] MCP 审批拒绝路径的测试覆盖被本次改动拿走**：
  `ncx-core/src/mcp_tool.rs` 的 `register_and_execute_echo` 把 mutation 断言从
  `denied by approval policy 'never'` 换成了 compaction 守卫消息
  （`context compaction consistency check`）。compaction 守卫位于 registry 层
  `execute_attempt`，先于工具执行返回，因此 `McpTool::execute` 里 `Approver` 的
  `AutoDeny` 分支（`approval_policy=never` 拒绝有副作用 MCP 调用）现在删掉也会全绿。
  全仓对该分支唯一剩下的引用只是对只读 `recall_user` 的否定断言，不构成覆盖。
  建议拆成两个测试：一个只设 `approval_policy=never` 断言拒绝文案，一个只设
  compaction 守卫断言拦截文案，互不遮蔽。
- **[P1] 脆弱的 MCP 发现错误传播被测试锁死**：
  新增的 `resolve_mcp_process_value` 对 command/args 逐个调用 `validate_resource(..)?`，
  任何一个插件 `mcpServers` 配置里出现一个坏相对参数（如 `../x.py`）都会让
  `discover_codex_mcp_servers` 整体返回 Err；`gui/src-tauri/src/bridge.rs:1124` 再用 `?`
  上抛，结果单个插件配置错误会导致整个会话组装失败、所有 MCP server 一起消失。
  同时 `lib.rs:5005` 的测试 `assert!(bridge.contains("discover_codex_mcp_servers(&cfg.workspace)?"))`
  把这条脆弱路径当成契约锁死，后续做 per-plugin 容错（跳过坏 server 并记 warning）
  时必须同步改这条断言。另注意 bundled-resource 启发式会把恰好与插件根下同名文件的
  裸相对参数（如 `settings.toml`）改写成绝对路径，改变 server 自己的 CWD 解析语义。
- **[P2] 两个 hook 测试在 Windows 并行跑会间歇性超时误报（HANDOFF 2026-08-28 已记录，未修）**：
  `agent_loop/tests/runtime_tests.rs` 的 `user_prompt_hook_can_block_model_call` 与
  `user_prompt_hook_output_is_sent_as_system_note` 起真实 shell 进程且 `timeout_s: 3`，
  测试二进制并行调度下进程启动延迟可超 3s，把"hook 行为正确"误报成失败。建议测试内
  超时放到 15–30s（生产默认 10s 不动），或改为注入 fake executor。
- **[P2] 并行批次回归检测依赖墙钟**：`read_only_calls_run_concurrently` 用
  `elapsed < 800ms`（4×300ms sleep）判断并发，串行退化时约 1200ms 能抓住回归，
  但重负载 CI 上有误报空间；可加一个"in-flight 计数器峰值≥2"的确定性断言替代纯计时。
- **[P2] 固定名临时目录存在跨进程互踩风险**：`agent_loop/tests.rs::tmpdir`、
  `tool_dispatch.rs` 新测试（`ncx_dynamic_read_dispatch`）、`tool_scheduler.rs`
  （`ncx_bounded_read_pool`）、`ncx-mcp/src/lib.rs`（`ncx_mcp_mock`）、
  `ncx-sandbox/src/policy.rs`（`ncx_policy_test`/`ncx_ws`）都是固定路径且开头
  `remove_dir_all`。同一次 `cargo test` 内名字不冲突，但 IDE 与 CLI 同时各跑一份
  测试时会互删对方目录。`plugins/openai_compat/tests.rs::temp()`（名字+纳秒时间戳）
  是现成的正确范式，建议统一。
- **[P3] GUI 的 include_str! 源码字符串断言继续累积脆性**：
  `gui/src-tauri/src/lib.rs` 测试大量 `include_str!` + 整段标记逐字符匹配。本次新增的
  reasoning-run 断言要求 `<details class="reasoning-run" ...>` 整行 Svelte 标记逐字符
  一致，任何无害排版改动都会红；其负向断言（把同一串再拼上 ` open=` 断言不包含）接近
  恒真，防不住真正的行为回退。这类测试建议只锚定行为关键标记（如
  `class:current-run={busy && ...}` 本身），不要锁整段属性串。
- **[P3] 杂项**：`git status` 里 `rust/gui/src-tauri/Cargo.toml` 显示 modified 但内容
  diff 为空（CRLF 行尾噪音），建议 `git checkout --` 还原；全仓 `cargo fmt --check`
  仍被既有 `ncx-dreamina-gateway/src/lib.rs` 格式差异拦截（已知，未修）。
- 本次扫描确认无误的部分：`basic_tests.rs` 两个新 compaction 恢复测试边界正确
  （`run_turn` 清守卫、`run_goal_round` 不清，已对照 `run_turn_with_authority` 实现）；
  `tool_dispatch.rs` 新的单测对 `call_is_read_only` 动态分类的断言方向正确；
  `openai_compat` 新增安装后解析测试通过且临时目录唯一；thread-store 跨进程 lease
  测试（唯一路径+轮询上限+子进程 helper）与 Python 侧 `tmp_path` 用法均规范。
