# DeepSeek Harness 与 nanocodex 组合方案

## 1. 结论

不直接把 DeepSeek Harness 作为依赖嵌入 nanocodex，也不整体移植 Cordis。推荐采用两层策略：

1. 短期通过外部进程 runner 或薄 ACP client 把 `dsh` 作为隔离的参考 agent，建立同任务对照基线。
2. 中期在 Rust 发布实现中原生吸收六项机制：可组合能力接口、追加式会话事件、agent inbox、工具中间件流水线、按会话 preset、可恢复 subagent。

这条路线保留 nanocodex 当前的 Rust 单二进制、公开配置键、GUI、沙箱和审批语义，同时把集中在 `agent_loop.rs`、`tools.rs`、`session.rs` 中的生命周期规则拆成可测试的责任模块。

具体的工作包、依赖顺序、PR 拆分和验收门禁见[特性吸收开发计划](deepseek-harness-adoption-development-plan.zh-CN.md)。

## 2. 研究基线

上游仓库：`https://github.com/deepseek-ai/deepseek-harness`

- 本地参考目录：`D:\agent_prac\deepseek-harness`
- 分析提交：`47f943859bef60e4160492346772ded9b24f765a`
- 提交时间：2026-08-13 19:38:46 +0800
- 上游版本：`0.1.0-rc.5`
- 许可证：MIT
- 运行时：Node.js `^22.19.0 || >=24.0.0`、pnpm workspace、TypeScript ESM
- 仓库状态：开发者预览，上游明确不承诺当前内部 API 和磁盘格式兼容
- 规模快照：226 个 package manifest，约 184544 行 `src` TypeScript，约 208617 行 `tests` TypeScript

规模数据用于判断集成成本，不用于评价实现质量。上游是完整产品和插件生态，不是适合直接复制进 Rust CLI 的轻量库。

## 3. DeepSeek Harness 的核心框架

### 3.1 一切皆插件，但核心价值是所有权

DeepSeek Harness 使用 Cordis 组装插件树。模型适配器、agent loop、session、工具注册表、compaction、审批、沙箱和 subagent 都是插件。注册是可撤销 effect，插件卸载时其监听器、服务和注册项一起释放。

值得迁移的不是 YAML 或 Cordis API，而是以下不变量：

- 每项能力有明确的定义、提供方和消费方。
- agent 创建返回带 `dispose()` 的所有权句柄。
- 每个 agent 有自己的作用域，按会话覆盖工具和 prompt，不污染其他会话。
- 新行为优先挂到事件或服务接口，不修改 agent loop 主干。

### 3.2 agent loop 是 turn/step 状态机

上游把一次用户工作划分为 turn 和 step：

```text
inbox -> turn/start -> pre-step -> step/start
      -> request -> stream -> assistant/message
      -> tool/call -> tool pipeline -> tool/result
      -> next step or turn/end
```

`followup`、`steer`、`inject` 共用一个 inbox，但进入不同边界：新轮次、下一 step、仅注入不唤醒。取消通过单个活动信号传播，agent 在完全空闲后才允许 maintenance 操作。

### 3.3 会话日志是唯一真源

上游不把 OpenAI messages 当作权威存储。它只追加 `SessionEvent`，再通过 projection 派生模型历史、GUI、恢复、fork、telemetry 和 transcript。关键规则是：模型能看到的内容必须能从日志重建。

这使下列行为可审计：

- turn/step 的开始和结束；
- 流式 chunk 和最终 assistant message；
- tool call/result 配对；
- 审批请求和结果；
- sandbox、preset、模型路由等会话级变化；
- compaction 替换了哪些历史节点；
- subagent 的父子关系和生命周期。

### 3.4 工具执行是分层流水线

上游的工具执行包含：参数验证、`pre-execute` 策略、`execute` 包装器、工具 body、`post-execute`、最终内容投影和只读结果观察。超时、重试、审批和 telemetry 是包装器或监听器，不写进每个工具。

并发调度也与工具定义分离：明确声明可并行的调用进入有界池，其余调用形成顺序屏障；结果仍按模型调用顺序写回日志。取消后，未开始调用会得到合成的错误结果，保证回放时 call/result 仍配对。

### 3.5 prompt、工具和运行时上下文分开组装

系统 prompt 由具名、有序 section 组成；同名的 agent 级 section 可以遮蔽全局 section。工具 schema 有稳定顺序。sandbox、approval 等变化频繁的事实不改系统 prompt，而以追加的运行时上下文快照进入历史，减少 KV cache 前缀失效。

### 3.6 compaction 是可替换能力

compaction 不在 loop 内硬编码。确定性工具结果剪枝先运行；仍超限时才生成摘要。压缩不删除旧日志，而是追加替换事件，记录被遮蔽节点、摘要、token 估计和失败生命周期。选择范围必须保持 tool call/result 配对。

### 3.7 subagent 有明确的权限和生命周期

subagent 支持一次性和可继续两种运行。父子关系写入持久日志，委派深度单调递增。进程内 child 继承父级已解析的沙箱范围，但审批固定为 `never`，因此无人值守 child 不能自行升级权限。父级拥有 child，释放按子先于父的顺序收敛。

## 4. 与 nanocodex 的映射

| DeepSeek Harness 机制 | nanocodex 现有实现 | 组合判断 |
| --- | --- | --- |
| LLM adapter seam | `ncx-provider`、`agent_loop::Provider` | 保留并扩展路由信息，不引入上游 adapter |
| Agent driver | `ncx-core/src/agent_loop.rs` | 拆分状态机和扩展点，保持 `run_turn` 兼容门面 |
| Session event log | `ncx-core/src/session.rs` 的 OpenAI message JSONL | 新增版本化事件日志和 projection；旧日志只读迁移 |
| Tool registry/pipeline | `ncx-core/src/tools.rs`、hooks、`ncx-sandbox` | 抽出统一中间件；真实授权仍由现有执行边界拥有 |
| Prompt sections | `Session.system`、project instructions、skills、memory notes、genome | 新增稳定有序的 `PromptAssembler`，复用现有内容提供方 |
| Compaction seam | `ContextEditPolicy`、`Session::compact()` | 从破坏性重写升级为追加式 surface replacement |
| Agent preset | CLI/GUI 构造 `ToolContext` 和 registry | 新增可信 `agent.toml` profile，先支持内置能力和 MCP 引用 |
| Subagent runtime | `orchestrator.rs` 的 worker/递归编排 | 让 orchestrator 成为 subagent consumer，不再拥有第二套生命周期 |
| Plugin lifecycle | 静态 Rust 注册、MCP、skills、hooks | 先做 Rust 静态插件 SPI；动态原生插件延后 |
| Test harness | `bench/`、`train/forge.py` | 复用现有离线任务、轨迹和接受门控比较新旧 runtime |

当前 Rust 实现已经具备 provider、工具、hooks、skills、MCP、memory、sandbox、approval、session、fork、orchestrator 和 forge。缺口主要是统一的生命周期协议，而不是缺少更多功能入口。

ACP 和 MCP 是不同协议。DSH 提供 ACP 自动化服务，nanocodex 当前提供 MCP 客户端，二者没有可直接复用的互通路径；实验适配器必须明确选择进程调用或实现 ACP client，不能把现有 MCP 能力当成 ACP transport。

## 5. 目标架构

```text
ncx-cli / Tauri GUI / ACP adapter
                 |
          AgentRuntime facade
                 |
   +-------------+-------------+
   |             |             |
Inbox       PromptAssembler  EventBus
   |             |             |
   +-------- AgentDriver ------+
                 |
        append-only SessionLog
                 |
       SessionProjection(s)
      /        |         \
 model       GUI       transcript

AgentDriver -> ToolScheduler -> ToolPipeline -> Tool
                              -> approval/sandbox enforcement

Orchestrator -> SubagentRuntime -> child AgentRuntime
Compactor    -> SessionProjection replacement events
Profile      -> scoped prompt/tool/middleware registrations
```

### 5.1 Rust 接口建议

接口名称是方案级建议，最终以实现时的源码检索和兼容约束为准。

```rust
trait CapabilityProvider {
    fn register(&self, scope: &mut AgentScope) -> RegistrationSet;
}

trait AgentMiddleware {
    async fn before_step(&self, ctx: &mut StepContext) -> StepDecision;
    async fn before_request(&self, request: &mut ModelRequest) -> Result<()>;
    async fn on_request_error(&self, failure: &ProviderFailure) -> RecoveryAction;
    async fn before_turn_end(&self, ctx: &TurnContext) -> Result<()>;
}

trait ToolMiddleware {
    async fn pre_execute(&self, call: &ToolCallContext) -> ToolDecision;
    async fn execute(&self, call: &ToolCallContext, next: ToolNext<'_>) -> ToolOutcome;
    async fn post_execute(&self, call: &ToolCallContext, result: ToolOutcome) -> ToolOutcome;
}
```

`RegistrationSet` 必须可释放；注册顺序和覆盖规则必须确定。第一版只加载编译进二进制的实现及现有 MCP 工具，不加载任意 native DLL。

### 5.2 SessionEvent v1

建议至少包含：

```text
session/created
turn/start, turn/end
step/start, step/end
user/message
assistant/chunk, assistant/message
tool/call, tool/result
request/header
approval/asked, approval/decided
sandbox/mode
profile/selected
compaction/start, compaction/replace, compaction/end
subagent/created, subagent/settled
```

每条事件包含 `format_version`、`session_id`、单调 `seq`、时间、类型和类型化 payload。事件写入后不可改写。模型消息、GUI 卡片和恢复状态都从 projection 得到。

兼容策略：

- 旧 JSONL 保持可读，不原地改写。
- 第一次 resume 时可在新日志中追加 `legacy/imported` 及投影后的消息事件。
- 迁移前保留原文件或使用新扩展名，失败可回退。
- 公开 CLI 参数和配置键不因内部日志升级而改变。

### 5.3 安全不变量

- `ncx-sandbox` 继续拥有策略分类；`ncx-tools::PolicyExecutor` 继续拥有真实进程执行。
- `apply_patch` 的路径校验继续位于写入边界，不能仅由 middleware 判断。
- middleware 只能收紧权限，不能绕过 executor 或路径校验。
- 缺少审批应答者、middleware 异常、超时和无效返回一律 fail closed。
- subagent 固定继承父级的已解析沙箱范围，审批策略固定为 `never`。
- profile 中的任意外部进程或 MCP 配置继续经过现有 `--mcp` 启动门和审批策略。

## 6. 分阶段实施

### P0：隔离验证，不改生产行为

目标：证明哪些收益来自框架，哪些来自模型。

- 保留上游浅克隆及提交号，不将其 vendoring 到 nanocodex。
- 首选直接运行 `dsh` headless 子进程；若需要流式事件、取消和结构化状态，再实现仅覆盖实验所需方法的 ACP client。
- 用同一模型、同一任务、同一权限范围跑 `bench/` 对照。
- 记录完成率、模型调用数、工具调用数、token、耗时、恢复成功率和人工审批次数。

退出标准：至少覆盖正常完成、工具失败、取消、超时、resume、fork 和越权拒绝；未形成稳定优势时不进入大规模迁移。

### P1：抽出接口，保证字节级行为兼容

- 将 `agent_loop.rs` 拆为 driver、scheduler、request builder、event sink。
- 将 `tools.rs` 拆为 tool trait、registry、pipeline、built-in tools。
- 新增稳定的 prompt section assembler，先由现有 system/project instructions/skills/memory/genome 供给。
- 保留 `AgentLoop::run_turn`、`Tool` 和现有 CLI/GUI 构造入口作为兼容门面。

退出标准：现有 Rust 测试和 benchmark 不退化；相同 fixture 产生相同 provider request 和最终消息。

### P2：追加式事件日志与 projection

- 定义 `SessionEvent`、`SessionLog`、`ModelProjection` 和 `UiProjection`。
- 双写旧消息日志与新事件日志，先比较 projection，不切换读取路径。
- 增加崩溃点测试：assistant stream 中断、tool 已调用未返回、turn 未闭合、compaction 中断。
- 对比一致后切换模型、GUI、resume、fork 到 projection，最后停止旧格式写入。

退出标准：任意注入的崩溃点后均可恢复为合法 tool call/result 序列；旧会话可恢复且原文件未被破坏。

### P3：工具和 agent 中间件

- 把现有 pre/post hook 接到工具 pipeline。
- 把 approval、timeout、telemetry、result pruning 分别做成独立 middleware。
- 引入 `exclusive`/`parallel` 调度分类和有界并发，提交结果保持模型顺序。
- agent 侧加入 pre-step、request、request-error、turn-stopping 扩展点。

退出标准：策略异常 fail closed；取消后无孤儿进程；每个已记录 call 均有结果或明确的合成取消结果。

### P4：按会话 profile

- 定义版本化 `agent.toml`，只引用已注册能力、prompt section、工具集合、模型路由和 MCP server id。
- 在 agent 发布前完成 profile 解析和注册；失败时整体回滚。
- profile 选择写入会话事件。已有内容的会话禁止热切换工具集合。
- GUI 增加 profile 选择，但不在 UI 中复制配置校验逻辑。

退出标准：两个同时运行的 session 使用不同 profile 时工具和 prompt 不串扰；无效 profile 在首次模型请求前失败。

### P5：非破坏性 compaction

- 先做确定性的旧 tool result 中段剪枝。
- 再做摘要 provider；压缩范围保持 tool call/result 边界。
- 用 replacement event 遮蔽旧 surface，不删除原事件。
- `forge` 同时评价质量、token 和恢复正确性。

退出标准：压缩前后未遮蔽尾部逐字一致；摘要失败不改变模型 surface；日志可以解释每次替换。

### P6：统一 subagent runtime

- 先实现一次性 child：owned handle、深度限制、取消传播、子先于父释放。
- 将 `orchestrator.rs` 改为 `SubagentRuntime` 的 consumer，保留原 CLI 行为。
- 再实现 continuable child、followup、interrupt、list 和冷恢复。
- 将父子关系、profile id、权限快照和终止原因写入 session events。

退出标准：父级取消后无活动 child；child 无法请求提权；冷恢复不重置委派深度或 profile。

### P7：外部插件 SDK，按需求决定

只有静态 Rust SPI、MCP 和 skills 无法满足真实扩展需求时再做。优先选择进程外协议和签名/信任策略，不直接加载不受信任的 DLL。Cordis 的热卸载和 HMR 不作为首期目标。

## 7. 需求与测试追踪建议

| ID | 可观察要求 | 主要测试 |
| --- | --- | --- |
| REQ-DSH-001 | 现有 CLI/GUI 和配置键保持兼容 | TEST-DSH-001 request/CLI snapshot |
| REQ-DSH-002 | 模型可见输入全部可从事件日志重建 | TEST-DSH-002 projection round-trip |
| REQ-DSH-003 | 工具策略和审批 fail closed | TEST-DSH-003 middleware fault injection |
| REQ-DSH-004 | 取消后无孤儿工具进程 | TEST-DSH-004 cancellation/process-tree |
| REQ-DSH-005 | 不同 session 的 profile 不串扰 | TEST-DSH-005 scoped composition |
| REQ-DSH-006 | compaction 不破坏原始事件 | TEST-DSH-006 replacement replay |
| REQ-DSH-007 | subagent 不能扩大父级权限 | TEST-DSH-007 delegated policy |
| REQ-DSH-008 | 新 runtime 在固定 benchmark 上不退化 | TEST-DSH-008 repeated offline benchmark |

正式进入 G0 时应改用仓库 `TRACEABILITY.md` 的全局连续编号；本表中的 `REQ-DSH-*` 是研究阶段占位符，避免提前占用未知编号。

## 8. 不采用的方案

### 直接把 DSH Web/CLI 当作 nanocodex 新核心

不采用。它会引入 Node/pnpm 运行时、两套 GUI、两套会话目录、两套审批和沙箱语义，并破坏 Rust 单二进制目标。

### 将 Cordis 全量重写为 Rust

不采用。Cordis 的插件加载、realm、effect、HMR 和配置 patch 是一个独立平台工程。nanocodex 当前需要的是少量明确扩展点和所有权约定。

### 只复制 DeepSeek 的 system prompt

不采用。上游的主要收益来自状态、事件、权限和组合机制；prompt 单独复制无法提供恢复、隔离或审计能力，而且 nanocodex 已有 `ncx-forge` 负责 prompt/tool description 优化。

### 先实现动态 native 插件

不采用。动态库扩大 ABI、供应链和沙箱风险，且当前 MCP、skills、hooks 已覆盖大部分外部扩展需求。

## 9. 主要风险

- 事件日志迁移是最大风险。必须双写、对比 projection，再切读路径。
- 当前工作区已有大量未提交改动；实现阶段应使用独立 worktree 和专用分支。
- 上游处于 RC/开发者预览，后续设计可能变化；本方案固定提交，不追逐内部 API。
- 更细的生命周期会增加类型和测试数量。应以删除 `agent_loop.rs`、`tools.rs` 中的交叉规则为成功标准，而不是仅新增抽象。
- 框架改造不会提高模型的硬推理上限。收益应落在恢复率、权限正确性、可组合性、工具效率和可观测性。

## 10. 建议的第一项实现

第一项生产改动应是 P1 的 `PromptAssembler` 与 request snapshot 测试，而不是 session 格式或 subagent。它跨模块影响较小，能建立 scoped registration、稳定排序和可释放注册的基本模式，也能立即把 project instructions、skills、memory、mode、genome 从字符串拼接变为具名贡献。通过该模式验证 Rust 侧的组合 API 后，再把同一生命周期模型扩展到 tool pipeline 和 session events。
