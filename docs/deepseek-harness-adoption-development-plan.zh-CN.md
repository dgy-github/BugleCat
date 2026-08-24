# DeepSeek Harness 特性吸收开发计划

> 迁移状态（2026-08-24）：本文保留最初 P0-P8 的行为目标和门禁，但 P3-P5 的实现路线已由后续 OpenAI Codex 差异审查修订。当前事实源是 `ncx-protocol`、`ncx-thread-store`、`ncx-app-server` 和 Harness Profile/Bundle；不再新增平行的 SessionEvent/Agent Profile 状态机。旧 SessionIndex 仅用于一次性迁移与旧文件兼容。最新完成度与剩余结构债务见仓库根 `HANDOFF.md`。

## 1. 目标与边界

目标是在不改变 nanocodex 公开 CLI、配置键、会话恢复语义、沙箱和审批执行边界的前提下，吸收 DeepSeek Harness 的生命周期和可组合性设计。

本计划吸收：

- 追加式 SessionEvent 日志和模型/UI projection；
- Agent inbox、取消收敛和 owned handle；
- Prompt section、工具 schema 和运行时上下文的分层组装；
- 可逆的 tool middleware pipeline；
- 按会话 profile 的能力隔离；
- 受权限继承和深度限制约束的 subagent；
- 非破坏性 compaction 和工具结果剪枝。

本计划不吸收：

- Cordis、YAML/JS 动态加载、HMR；
- DeepSeek Harness 的 Node/pnpm/Web 产品运行时；
- 任意 native DLL 的首期动态插件 ABI；
- 第二套沙箱、审批、Provider、计费或视频任务状态机。

## 2. 当前基线

研究基线固定为 `D:\agent_prac\deepseek-harness` 的提交 `47f943859bef60e4160492346772ded9b24f765a`。nanocodex 当前 Rust 入口包括：

- Agent loop：`rust/crates/ncx-core/src/agent_loop.rs`；
- Session：`rust/crates/ncx-core/src/session.rs`；
- 工具注册和执行：`rust/crates/ncx-core/src/tools.rs`；
- 编排：`rust/crates/ncx-core/src/orchestrator.rs`；
- 沙箱和审批：`rust/crates/ncx-sandbox`；
- CLI 组装：`rust/crates/ncx-cli/src/main.rs`、`runner.rs`；
- 评测和训练闭环：`bench/`、`train/`。

当前工作区已经出现 `rust/crates/ncx-core/src/tool_middleware.rs` 原型及 `ToolRegistry` 接线。该文件属于当前未提交工作，不在本计划中覆盖或重写；第一工作包负责验证并补齐它的契约。

## 3. 总体依赖图

```text
P0 基线与隔离实验
       |
       +--> P1 ToolMiddleware ------+
       |                             |
       +--> P2 PromptAssembler ------+--> P3 SessionEvent + Projection
                                             |
                                             +--> P4 AgentRuntime / Inbox
                                                       |
                                  +--------------------+--------------------+
                                  |                                         |
                             P5 Profiles                              P6 Compaction
                                  |                                         |
                                  +--------------------+--------------------+
                                                       |
                                                  P7 Subagents
                                                       |
                                                  P8 Hardening
```

P1 和 P2 可以并行；P3 需要先确定 P2 的模型可见输入；P4 需要 P3；P5、P6 依赖 P4；P7 依赖 P4、P5 和现有 sandbox policy；P8 是发布前合并门。

## 4. 工作包

### P0：基线、隔离和对照实验

**目的**：先量化框架收益，避免把模型能力变化误判成架构收益。

**工作**：

1. 在独立 worktree 建立 `codex/dsh-adoption-p0`，不与当前有未提交修改的工作区混用。
2. 固定一组离线任务，至少包含工具成功、工具失败、取消、超时、resume、fork 和越权拒绝。
3. 通过 `dsh` headless 子进程或薄 ACP client 做外部对照；不要把 ACP 当作现有 MCP 客户端的直接替代。
4. 记录完成率、模型调用数、工具调用数、token、耗时、恢复成功率和审批次数。

**文件范围**：`bench/` 新增 runner/fixture；不改 Rust runtime。

**验收**：相同模型和权限设置下，旧 nanocodex 与 DSH 对照结果可重复；失败轨迹可定位到具体阶段。

**回滚**：删除实验 runner 和报告，不影响产品代码。

### P1：ToolMiddleware pipeline

**目的**：把 DeepSeek 的 pre/around/post tool pipeline 映射到现有工具执行边界。

**工作**：

1. 验证现有 `tool_middleware.rs` 的注册顺序、逆序退出、阻断和注销语义。
2. 将现有 `hooks.rs` 的 pre/post hook 接入 middleware，但保留旧配置格式。
3. 增加 around/next 能力，用于 timeout、retry、metrics 和 result projection；middleware 不得替换工具身份或绕过 sandbox。
4. 为每次执行增加稳定 call identity、取消 signal 和是否实际进入 tool body 的状态。
5. 注册接口返回可调用的 disposer，避免只靠字符串注销。
6. 未知工具、middleware 异常、超时和审批不可用均 fail closed。

**主要文件**：

- `rust/crates/ncx-core/src/tool_middleware.rs`；
- `rust/crates/ncx-core/src/tools.rs`；
- `rust/crates/ncx-core/src/hooks.rs`；
- `rust/crates/ncx-sandbox/src/approval.rs`；
- `rust/crates/ncx-tools/src/executor.rs`。

**测试**：嵌套顺序、阻断、注销、异常、超时、取消、审批缺失、路径越界和 middleware 不能绕过 executor。

**验收**：现有工具输出和审批行为保持兼容；所有工具调用都经过同一条可观测 pipeline；无孤儿进程。

**回滚**：保留 `ToolRegistry::execute` 兼容门面，以 feature flag 或空 middleware 列表退回旧路径。

### P2：PromptAssembler 和 scoped composition 基础

**目的**：借鉴 DeepSeek 的具名 section、稳定排序和 agent 级遮蔽，替代散落字符串拼接。

**工作**：

1. 定义 `PromptSection { name, order, text/provider }` 和 `PromptAssembly`。
2. 把 project instructions、skills、memory recall、mode、genome 和工具 schema 变成独立 provider。
3. 固定工具 schema 排序；同名 agent section 覆盖全局 section；重复注册和未知变量加载时失败。
4. 对每次 request 生成不可变 snapshot，记录 system、runtime notes、tool schemas 和 route。
5. 保留现有 `Session.system` 和 CLI/GUI 构造入口，先由 adapter 组装旧格式。

**主要文件**：

- 新增 `rust/crates/ncx-core/src/prompt.rs`；
- `rust/crates/ncx-core/src/agent_loop.rs`；
- `rust/crates/ncx-core/src/project_instructions.rs`；
- `rust/crates/ncx-core/src/skills.rs`；
- `rust/crates/ncx-core/src/genome.rs`；
- `rust/crates/ncx-cli/src/main.rs`、`runner.rs`。

**测试**：稳定排序、重复名、agent 覆盖、空 section、变量错误、旧 request snapshot 等价性。

**验收**：相同输入产生相同 provider request；prompt 来源可追踪；不同 agent 的 section 不串扰。

**回滚**：保留 `Session::for_model_edited`，由配置开关选择旧 assembler。

### P3：SessionEvent v1、双写和 projection

**目的**：把会话从“OpenAI message JSONL”升级为可审计、可恢复的追加式事件流。

**工作**：

1. 定义版本化 `SessionEvent` enum、envelope、seq、session id 和 payload 校验。
2. 实现 `SessionLog` 追加写入、flush、损坏尾行处理和错误报告。
3. 实现 `ModelProjection`、`UiProjection` 和 legacy message projection。
4. 先双写旧 JSONL 与新事件日志，只比较 projection，不切换读取路径。
5. 为 assistant stream 中断、tool call 未返回、turn 未闭合和进程崩溃增加恢复 fixture。
6. 迁移只读旧会话；禁止原地覆盖旧文件。

**主要文件**：

- 新增 `rust/crates/ncx-core/src/events.rs`、`session_log.rs`、`projections.rs`；
- `rust/crates/ncx-core/src/session.rs`；
- `rust/crates/ncx-core/src/checkpoint.rs`、`session_index.rs`；
- `rust/crates/ncx-cli/src/main.rs`、GUI session reader。

**测试**：事件 round-trip、projection 等价性、崩溃恢复、尾行损坏、旧会话 resume、fork lineage、tool call/result 配对。

**验收**：所有模型可见输入都能由事件重建；旧会话可恢复；新日志不可通过 compaction 或 UI 操作删除历史。

**回滚**：保留旧 reader 和双写开关；切读失败时自动回退旧 projection，不修改源日志。

### P4：AgentRuntime、Inbox 和取消收敛

**目的**：引入 DeepSeek 的 turn/step/inbox/owned handle 模型，替代简单的同步 `run_turn` 生命周期。

**工作**：

1. 将 `agent_loop.rs` 拆成 driver、request builder、step runner、tool scheduler 和 lifecycle。
2. 定义 `AgentHandle::dispose()`、`when_idle()`、`cancel()` 和 inbox 的 `next-turn`/`next-step`/`inject` 语义。
3. 将 GUI Stop、CLI Ctrl+C、hooks 和子任务取消统一为一个 activity signal。
4. 明确 cancellation、maintenance、wakeup 的收敛规则，避免取消后唤醒丢失或重复运行。
5. 保留 `AgentLoop::run_turn` 作为同步兼容门面，内部委托新 runtime。

**主要文件**：

- 新增 `rust/crates/ncx-core/src/agent.rs`、`inbox.rs`、`lifecycle.rs`、`request.rs`；
- `rust/crates/ncx-core/src/agent_loop.rs`；
- `rust/crates/ncx-cli/src/runner.rs`、`main.rs`；
- `rust/gui/src-tauri/src` 中的 Stop/stream bridge。

**测试**：followup/steer/inject、取消前后入队、idle maintenance、重复 dispose、request error retry、wakeup latch、GUI event sink。

**验收**：取消后没有未回收的进程或活动 child；`when_idle()` 只在整个 agent 活动收敛后完成；旧 CLI 行为不变。

**回滚**：runtime facade 继续暴露旧 `run_turn`，旧 loop 可由测试 profile 选择。

### P5：按会话 Agent Profile

**目的**：让每个 session 在创建前选择独立的工具、prompt、模型和策略组合。

**工作**：

1. 定义版本化 `agent.toml`，只引用已注册工具、skills、middleware、model route 和 MCP server id。
2. 在 agent 发布前完成解析、校验和 scoped registration；失败整体回滚。
3. profile 选择追加 `profile/selected` 事件；已有内容的 session 禁止切换工具集合。
4. profile 只负责组合，不复制 sandbox/approval 规则；权限仍由现有模块解析。
5. CLI/GUI 增加 profile 选择和错误展示，校验逻辑只在配置模块拥有。

**主要文件**：

- 新增 `rust/crates/ncx-config/src/profile.rs`；
- 新增 `rust/crates/ncx-core/src/profile.rs`、`scope.rs`；
- `rust/crates/ncx-cli/src/args.rs`、`main.rs`、`runner.rs`；
- `rust/gui/src/App.svelte` 和 Tauri command bridge。

**测试**：profile schema、未知 capability、重复 tool、两个 session 隔离、空 session recompose、已有内容锁定、MCP 启动门。

**验收**：两个并发 session 使用不同 profile 时工具和 prompt 不串扰；无效 profile 在首个模型请求前失败。

**回滚**：未指定 profile 时使用当前默认组装；profile 文件读取失败不改变旧配置路径。

### P6：非破坏性 Compaction

**目的**：把当前 `Session::compact()` 的 destructive rewrite 改为可解释的 replacement projection。

**工作**：

1. 先实现确定性的 tool result head/middle/tail 剪枝。
2. 选取摘要范围时保持 tool call/result 配对。
3. 追加 `compaction/start`、`compaction/replace`、`compaction/end` 事件，不删除旧事件。
4. 摘要失败、取消、持久化失败都记录失败生命周期并保持原 surface。
5. 将 `/compact`、自动压力压缩和 context-overflow recovery 统一到 compaction service。

**主要文件**：

- 新增 `rust/crates/ncx-core/src/compaction.rs`、`tool_result_pruner.rs`；
- `rust/crates/ncx-core/src/session.rs`、`checkpoint.rs`；
- `rust/crates/ncx-cli/src/main.rs` 的 `/compact` 路径；
- `train/evaluator.py`、`train/export.py` 的轨迹读取。

**测试**：工具配对边界、重复压缩、摘要失败、取消、崩溃恢复、原始事件保留、token accounting。

**验收**：压缩前后未遮蔽尾部逐字一致；压缩失败不改变模型 surface；日志可以解释每次 replacement。

**回滚**：保留旧 `ContextEditPolicy` 作为只读 request-time fallback，不再覆盖持久日志。

### P7：统一 SubagentRuntime

**目的**：让 `orchestrator.rs` 消费统一的 child runtime，而不是自有第二套 worker 生命周期。

**工作**：

1. 首先实现 one-shot child：owned handle、深度限制、取消传播、结果和 dispose。
2. child 创建时捕获父级已解析 sandbox policy；审批固定为 `never`；记录 parent/profile/delegation depth。
3. 将现有 classify/plan/decompose/worker/verify 流程适配到 `SubagentRuntime`。
4. 后续再实现 continuable child、followup、interrupt、list 和冷恢复。
5. 父子释放采用子先于父；基础设施失败不能留下半发布 child。

**主要文件**：

- 新增 `rust/crates/ncx-core/src/subagent.rs`、`subagent_policy.rs`；
- `rust/crates/ncx-core/src/orchestrator.rs`；
- `rust/crates/ncx-sandbox/src/policy.rs`、`approval.rs`；
- `rust/crates/ncx-cli/src/main.rs` 的 orchestration 入口。

**测试**：深度限制、权限继承、审批拒绝、父取消、child 失败回滚、dispose 顺序、child session 恢复。

**验收**：child 不能扩大父权限；父取消后无活动 child；现有 orchestrator 的输出和 verifier 行为保持兼容。

### P8：发布硬化和可选外部扩展

**目的**：完成性能、兼容、安全和文档门禁；只有必要时再开放外部插件。

**工作**：

1. 用 `bench/` 重跑旧 baseline、P0 对照和新 runtime，比较 pass/cost/recovery 三个维度。
2. 用 `train/forge.py` 验证 prompt/tool genome 不依赖旧 session message 格式。
3. 运行 Rust fmt、clippy、全 workspace tests、结构检查和 GUI build。
4. 生成并更新 `docs/project-memory` 目录；同步 CLI/config/session 文档。
5. 如果静态 Rust SPI、MCP 和 Skills 仍不足，再设计进程外插件协议；不直接加载不受信任 DLL。

**验收**：性能没有不可接受回归；所有公开配置和旧 session 可用；安全边界测试全绿；跳过的 live 检查有明确记录。

## 5. PR 和分支拆分

每个工作包单独一个分支或 PR，推荐顺序如下：

| PR | 内容 | 允许改变的公共面 |
| --- | --- | --- |
| PR-1 | P0 benchmark harness | 仅新增 bench fixture/报告 |
| PR-2 | P1 ToolMiddleware | 新增 Rust trait；旧 Tool API 不变 |
| PR-3 | P2 PromptAssembler | 新增内部 assembler；旧 Session API 不变 |
| PR-4 | P3 EventLog 双写 | 新增日志文件和 reader，不停旧日志 |
| PR-5 | P4 AgentRuntime | 新增 facade；`run_turn` 保持兼容 |
| PR-6 | P5 Agent Profile | 新增可选 profile 配置 |
| PR-7 | P6 Compaction | 新增 replacement event；旧 compact fallback 保留 |
| PR-8 | P7 SubagentRuntime | orchestrator 改为 consumer，CLI 参数保持 |
| PR-9 | P8 发布硬化 | 文档、测试、性能和安全收尾 |

实现期间必须使用独立 worktree。当前工作区存在大量用户修改，不能用 reset、checkout 或大范围格式化清理。

## 6. 统一完成定义

每个工作包完成前必须满足：

1. 有行为契约和明确的兼容边界。
2. 有一个无新代码时会失败的回归测试。
3. 失败、取消、超时、权限拒绝和恢复路径都有测试。
4. 没有复制 sandbox、approval、provider、session 或预算规则。
5. 运行适用的窄测试，再运行：

```powershell
cd rust
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cd ..
python scripts/check_code_structure.py --git-diff HEAD~1
```

GUI 改动额外运行：

```powershell
cd rust/gui
npm ci
npm run build
```

跳过需要真实 API、GPU、外部服务或特殊 Windows 工具链的检查时，必须在完成报告中写明具体命令和原因。

## 7. 第一阶段实际执行顺序

当前最合理的第一批实现不是 SessionEvent，而是：

1. 把现有 `tool_middleware.rs` 原型补齐取消、异常和 disposer 契约。
2. 增加 `PromptAssembler`，将现有 prompt 来源改为具名贡献。
3. 用 request snapshot 证明旧行为没有变化。
4. 再开始 EventLog 双写。

如果 P1 或 P2 无法保持旧 provider request 等价，应暂停后续阶段，先修复责任边界，不进入数据格式迁移。
