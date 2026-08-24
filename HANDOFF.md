# HANDOFF — nanocodex (Rust 线)

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
