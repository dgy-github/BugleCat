## 系统画像 · 跨子系统连接图

> 把 nanocodex 的 8 个子系统画成一张图：在线运行时（1–7）由一次 `ncx` 启动装配而成、围绕 `AgentLoop::run_turn` 转动；离线训练（8 ncx-forge）通过 `NCX_GENOME` 这唯一一道接缝把演化出的 prompt + 工具描述注入运行时。所有 trait 都是 `#[async_trait(?Send)]`，整套 agent 跑在一个 Tokio current-thread runtime 上（`main.rs` 显式 `new_current_thread()`），共享状态用 `Rc<RefCell<…>>`。

---

### 一次用户回合的端到端数据/控制流

**阶段 A — 启动装配（`ncx-cli/src/main.rs::run`，每进程一次）**

1. `load_config(overrides)` 解析配置；`--permission-mode` 若给出则经 `permission_mode_to_knobs` 覆盖 `--sandbox`/`--approval`（得到 `sandbox_mode / approval_policy / require_edit / plan_mode`）。
2. **沙箱(4)** 被构造为纯决策对象：`SandboxPolicy::new(sandbox_mode, &cfg.workspace).with_network_access(network)`。它此刻不做任何强制执行，只回答"物理上允许吗"。
3. **项目记忆(6)** `MemoryStore::new(workspace/.ncx/memory)` 建好后立即 `memory.consolidate(0.85)`（启动即跑的、幂等的启发式近重复合并）。
4. **Skills/MCP/视觉(7)** 的 Skills 半边在此装配：`discover_skills(workspace)`（builtin via `include_str!` → `~/.ncx` → `<workspace>/.ncx`，后者遮蔽前者）→ `skills_index_block(&skills)` 产出一级披露索引。
5. **ncx-forge 接缝(8→运行时)**：`Genome::from_env()` 读 `NCX_GENOME`。未设置/空/格式错误 → `Genome::default()`（可证明的 no-op）。`base_prompt = genome.base_system_prompt(SYSTEM_PROMPT)`。
6. `compose_system_prompt(&base_prompt, &[instructions, skills_index, plan_note])` 把基础 prompt、项目说明、技能索引、plan-mode 注记拼接成最终 system prompt。
7. `ToolContext::new(workspace, policy)` 通过一连串 `with_*` builder 装入**全部能力**：approval policy、`require_edit`、`plan_mode`、timeout、search 配置、`memory(6)`、hooks、`skills(7)`、`genome(8)` —— 这就是那个被廉价 `Rc` 克隆的"能力袋"。
8. **工具系统(3)** `ToolRegistry::new(ctx)` 注册核心九件 + 条件注册 `remember`（仅 `ctx.memory.is_some()`）与 `skill`（仅 `!skills.is_empty()`）；`register` 时用 `genome.describe(name, default)` 把 **genome(8)** 描述烘焙进 `tool_catalog`。
9. （可选）`--dump-genome` 在 MCP 注册之前导出默认 genome 给训练器并退出（保证只 dump 可演化的核心面）。
10. （可选）`--mcp`：`register_mcp_server` 经 stdio 连接每台服务器，把唯一的 `McpClient` 包进 `Rc<Mutex<McpClient>>`，为每个 `McpToolDef` 注册一个共享该句柄的 `McpTool`（**7** 的 MCP 半边）。
11. `Session::resume / with_log` 建会话；`AgentLoop::new(provider, tools, session).with_task_budget(...).with_context_edit(...)` (**2**) `.with_vision_provider(build_vision_provider(cfg))` (**7** 的视觉半边，仅当 `vl_model` 非空)。装配完成。

**阶段 B — 一个回合（`AgentLoop::run_turn` → `run_turn_inner`，**Harness(1)** 主导）**

12. **Harness(1)** 把 `event_sink` 从 `&mut self` 取出（规避借用冲突），算 `use_vision_this_turn = vision_provider.is_some() && has_image_block(user_input)` (**7**)，提取 `tool_query`。
13. 触发 `user_prompt` hook（可直接以 stop_reason `blocked` 短路整回合）；追加用户消息。
14. `for iteration in 0..max_model_calls`（`= max_iterations.min(task_budget.max_model_calls.max(1))`）：
    - 顶部查 `cancel()`；调 **工具系统(3)** 的 `schemas_for_query(tool_query)` 重建本回合可见 schema 视图（`tools.len()<=9` 全暴露，否则 `ALWAYS_VISIBLE_TOOLS` + 上回合 `tool_hints` + `catalog_score` 词法打分，上限 9，并重新应用 **genome(8)** 覆盖）。
    - 前置一条合成的 `budget_note` system 消息 + 任何 prompt-hook 注记。
    - `call_model` → **上下文压缩(2)** `Session::for_model_edited(notes, &self.context_edit)` 构建**非破坏性发送时视图**（第 1 趟压缩旧 `tool` 结果到 `max_tool_result_chars`，第 2 趟超 `max_chars` 时在 user 边界丢弃最旧前缀），`self.messages` 不变 → 分派给 `active_provider()`（视觉 vs 主，**7**）→ provider `.chat()`。
15. `finish_reason==error` → 返回 error。无 `tool_calls` → 记录助手文本，返回 `completed`。
16. 否则持久化携带 OpenAI 形态 `tool_calls` 的助手消息，按索引遍历：
    - 每个位置重查 cancel + 剩余工具预算（为 0 → 回填未应答项、返回 `task_budget`）。
    - `parallel_run` 当且仅当当前 + 下一个调用按注册表都 `read_only`：贪心收集连续只读序列，`join_all(execute_cancellable)` 并发跑；否则单个（写/未知）串行跑。
    - 每次分派经 **工具系统(3)** `ToolRegistry::execute(name, args)`：pre-hook（可硬拦截）→ 工具 → post-hook。
        - `apply_patch` / 写 `shell` → 向 **沙箱(4)** 询问：`SandboxPolicy::can_write` 判定逃逸路径；逃逸且有 approver → `ApprovalRequest{escalated:true, details=full diff}`；`ShellTool::needs_escalation` + `Approver::classify(cmd, esc)` 给出 `AutoApprove/Ask/AutoDeny`，`step_decision` 在禁用自动审批时单调升级。
        - `remember` → 写 **记忆(6)**（仅 CONFIRMED，精确去重，200 上限）。
        - `skill` → 返回 **Skills(7)** 二级正文 `load_body()`，让模型可再 `read_file` 随包资源（三级）。
        - `tool_search` → 重写 `ctx.tool_hints`，使命中工具的 schema 在**下一回合**浮现（**3** 的反馈回路）。
        - `McpTool` → `Approver` 门控后 `client.lock().await` 串行化 stdio RPC（**7**）。
    - 结果按原始顺序 `add_tool_result(id, name, result)` 追加 → 回到 14 继续循环。
17. 循环以"模型不带工具作答 / error / cancelled / 触及预算 / `max_model_calls` 耗尽"退出。`recorder.record(&agent.session)` 落 JSONL。

**记忆(6) 何时读**：召回路径在调用方（`runner.rs`：`recall(task, 6, 3000)`；GUI 会话开始 `recall("", 8, 4000)` 纯近期）把命中笔记作为 `RECALL_HEADER`（"线索而非事实"）注入 system prompt —— 即阶段 A 第 6 步的一部分。**写**则发生在回合内 `remember` 工具（第 16 步）。

**编排器(5) 何时包裹**：当 `--orchestrate` 时，`run` 不直接 `agent.run_turn`，而走 `run_orchestrated` → `Orchestrator::handle`。它在外层用 `classify`（Fast，无工具）路由，每个 worker / 子任务节点是**一次全新的 `AgentLoop` run**（`LiveRunner` 每节点新建 `DeepSeekProvider + Session + ToolRegistry`），plan/verify 节点用 `ToolRegistry::empty`（无工具推理）。即：**5 是 1 的外层调度器，1 是 5 的执行原语**。

---

### 流程图

```
                          离线 (OFFLINE LOOP)                            在线运行时 (RUNTIME)
        ┌─────────────────────────────────────────────┐   │
        │  8  ncx-forge  (train/ Python, no GPU)        │   │
        │  ┌───────────────────────────────────────┐   │   │
        │  │ self_check(sentinel) ─ gate            │   │   │
        │  │ extract_current ── ncx --dump-genome ──┼───┼───┼──► baseline genome (system_prompt + tool_desc)
        │  │ teacher panel (codex/claude/api)       │   │   │
        │  │   └ reflective mutation (TOML 覆盖)     │   │   │
        │  │ evaluator: bench + hidden check.py     │   │   │
        │  │   └ harvest FAILURE TRAJECTORY(脱敏)    │   │   │
        │  │ accept: TRAIN margin & VAL no-regress  │   │   │
        │  │ pareto: passrate↑ / cost↓              │   │   │
        │  └───────────────────────────────────────┘   │   │
        │        winner genome.toml ─────────────────────┐  │
        └─────────────────────────────────────────────┘ │  │
                                                         ▼  │
                                        ═══════ NCX_GENOME (唯一接缝) ═══════
                                                            │
   ╔════════════════════════════════════════════════════════════════════════════════════════╗
   ║                          ncx 进程 · current-thread runtime · ?Send / Rc<RefCell>          ║
   ║                                                                                          ║
   ║  阶段A 启动装配 (main.rs::run)                                                            ║
   ║   load_config ─► 4 SandboxPolicy(纯决策)                                                  ║
   ║              ├─► 6 MemoryStore + consolidate(0.85)                                        ║
   ║              ├─► 7 discover_skills → skills_index                                         ║
   ║              ├─► 8 Genome::from_env()  (空=可证明no-op)                                    ║
   ║              ├─► compose_system_prompt(base ⊕ instructions ⊕ skills_index ⊕ recall)       ║
   ║              ├─► ToolContext(.with_memory/.with_skills/.with_genome/policy...)             ║
   ║              ├─► 3 ToolRegistry::new (genome.describe 烘焙进 catalog; +remember/skill)     ║
   ║              ├─► 7 (--mcp) register_mcp_server → Rc<Mutex<McpClient>>                      ║
   ║              └─► AgentLoop::new(.with_context_edit, .with_vision_provider)                 ║
   ║                                                                                          ║
   ║  阶段B 一个回合 ── 1 AgentLoop::run_turn ───────────────────────────────────────────────  ║
   ║                                                                                          ║
   ║   user_input ─► [has_image_block? → 7 vision] ─► user_prompt hook                         ║
   ║        │                                                                                 ║
   ║        ▼   ┌──────────────── for iteration in 0..max_model_calls ────────────────┐       ║
   ║   call_model│  3 schemas_for_query (ALWAYS_VISIBLE ⊕ tool_hints ⊕ catalog_score)  │       ║
   ║        │    │             │ (genome 覆盖再应用 ⟵ 8)                                │       ║
   ║        │    │             ▼                                                       │       ║
   ║        │    │  2 Session::for_model_edited (非破坏性发送视图: 压缩+前缀丢弃)        │       ║
   ║        │    │             │                                                       │       ║
   ║        │    │             ▼                                                       │       ║
   ║        │    │  active_provider().chat()  (主 / 7 视觉)                            │       ║
   ║        │    │             │                                                       │       ║
   ║        │    │   tool_calls?── 无 ─► completed                                     │       ║
   ║        │    │      │ 有                                                           │       ║
   ║        │    │      ▼  连续 read_only → join_all 并发 / 写·未知 → 串行             │       ║
   ║        │    │  3 ToolRegistry::execute (pre-hook ▸ tool ▸ post-hook)              │       ║
   ║        │    │      ├ apply_patch / 写shell ─► 4 SandboxPolicy.can_write           │       ║
   ║        │    │      │                         + Approver.classify(Ask→approver)    │       ║
   ║        │    │      ├ remember ─────────────► 6 MemoryStore (写, CONFIRMED)         │       ║
   ║        │    │      ├ skill ───────────────► 7 load_body (二级披露)                │       ║
   ║        │    │      ├ tool_search ─────────► 3 重写 ctx.tool_hints (↺ 下回合可见)   │       ║
   ║        │    │      └ McpTool ─────────────► 7 Approver ▸ client.lock().await       │       ║
   ║        │    │             │ add_tool_result(按序回填)                              │       ║
   ║        │    └─────────────┘  budget / cancel 守卫 ─► 回填未应答 tool_calls          │       ║
   ║        ▼                                                                                 ║
   ║   TurnResult ─► recorder.record(JSONL)                                                   ║
   ║                                                                                          ║
   ║  5 Orchestrator (--orchestrate): classify(Fast,无工具) ─► best-of-N / decompose          ║
   ║     每个 worker/子任务节点 = 一次全新的 [阶段B] AgentLoop run; plan/verify 用空工具集       ║
   ╚════════════════════════════════════════════════════════════════════════════════════════╝
```

---

### 谁调用谁 · 依赖表

| 子系统 | 它调用 / 依赖 | 谁调用它 | 跨边界的数据 |
|---|---|---|---|
| **1 Harness** (`AgentLoop`) | **2** `for_model_edited`、**3** `schemas_for_query`/`ToolRegistry::execute`、provider `.chat`、**7** `active_provider`(视觉)、hooks | `main.rs::run`（一次性/REPL）、`run_one_turn`；**5** 每节点新建一个 | `user_input`(可含 `image_url`)、`TurnResult{final_text,iterations,tools_used,usage,stop_reason}` |
| **2 上下文压缩** (`Session`) | 仅纯数据变换（`json_chars`/`compress_tool_result`）；`/compact` 时 `rewrite_log` | **1** `call_model` 每回合（非破坏视图）；CLI `compact_session_text`（破坏性） | 入：`messages`+`system_notes`+`ContextEditPolicy`；出：发送时 `ContextMessages` + `ContextEditStats` |
| **3 工具系统** (`ToolRegistry`/`Tool`) | **4** `policy.can_write`/`Approver`(apply_patch、shell)、**6** `MemoryStore`(remember)、**7** skills/MCP、**8** `genome.describe`/`schema_for` | **1** 每回合 `schemas_for_query`+`execute` | 入：`ToolContext`(能力袋)、`tool_query`、`ToolCall{id,name,arguments}`；出：result `String`、`tool_hints` 副作用 |
| **4 沙箱** (`SandboxPolicy`/`Approver`) | 无（纯决策，零 I/O）；`needs_escalation`/`looks_read_only` 由消费方算 | **3** `ShellTool`/`apply_patch`；`main.rs` 构造 policy | 入：`(mode, workspace, path, command, needs_escalation)`；出：`bool` / `Decision{AutoApprove,Ask,AutoDeny}` / `ApprovalRequest` |
| **5 编排器** (`Orchestrator`) | `AgentRunner`(→`LiveRunner` → 每节点一次全新 **1** + provider + **3**)；`reason()` 用空工具集 | `main.rs::run_orchestrated`(`--orchestrate`) | 入：`task` 字符串；出：`OrchestratorOutcome{complexity,verify_passed,best_worker,final_text}` |
| **6 项目记忆** (`MemoryStore`) | `Summarizer` trait(→CLI `LiveSummarizer` fast 模型)；纯 markdown I/O | `main.rs`(`consolidate` 启动 / `--memory-merge`)、`runner.rs`(`recall` 注入 prompt)、**3** `RememberTool`(写) | 写：`MemoryEntry{ts,tags,text}`；读：`recall` → `RECALL_HEADER` 文本块注入 system prompt |
| **7 Skills/MCP/视觉** | Skills: `include_str!`+FS；MCP: `Rc<Mutex<McpClient>>`+`Approver`(**4**)；视觉: `DeepSeekProvider`(`vl_*`) | `main.rs`(discover/register/build)、**3**(`SkillTool`/`McpTool` 注册)、**1**(`active_provider` 路由) | Skills 索引/正文文本、`McpToolDef`+RPC、按回合 provider 切换(`use_vision_this_turn`) |
| **8 ncx-forge** (离线) | `ncx --dump-genome`(子进程)、teacher panel、隐藏 `check.py`、`session.jsonl` 收割 | 人工 / 训练脚本；产物 = `winner genome.toml` | **唯一出口 = `NCX_GENOME` 文件**：`{system_prompt?, tool_desc:{name→desc}}` → 运行时 **8 接缝** |

---

### 在线运行 vs 离线训练的分界

- **子系统 1–7 = 在线运行时（runtime）**：装在一个 `ncx` 进程里，每个用户回合都活动。它们共享 current-thread runtime、`Rc<RefCell>` 状态、同一个 `ToolContext` 能力袋。
- **子系统 8 ncx-forge = 离线循环（OFFLINE LOOP）**：纯 Python，跑在 `train/`，**绝不导入运行时代码**，只通过 `subprocess` + 环境变量驱动 `ncx.exe`。它的搜索/评估/接受循环（teacher 变异 → bench + 隐藏 `check.py` → train/val/test 接受 → Pareto）全部发生在用户回合之外。
- **`NCX_GENOME` 是训练与运行之间唯一的接缝**：genome = `system_prompt` + 每个工具的 `description`（**纯文本，绝不含工具行为**）。
  - 出口（离线→在线）：ncx-forge 写出 `winner genome.toml`。
  - 入口（在线）：`Genome::from_env()` 在阶段 A 读取它；`base_system_prompt`/`describe` 仅在有覆盖处叠加，其余字节级保持默认。
  - **可证明的 no-op 保证**：未设置/空/格式错误/空白 → `Genome::default()`，运行时与硬编码脚手架逐字节一致。正因为这个 no-op 可证明，离线训练的"全绿"才不会是假象——故有 `self_check` 的 sentinel 暗号词门，在无法证明注入确实生效前拒绝训练。
  - **安全侧的同一道边界**：因为 genome 只能换文字、执行始终由 **沙箱(4)** 把关，一个被不可信失败轨迹喂养的 teacher 无法注入新能力 —— 训练接缝与安全边界是同一条线。

---

### 三条贯穿全局的不变式/主题

1. **`?Send` / current-thread + `Rc<RefCell>` 契约**（贯穿 1 Harness · 3 工具 · 7 MCP · provider）。`Provider`/`Tool`/`ApprovalHandler`/`Summarizer`/`AgentRunner` 全标 `#[async_trait(?Send)]`，`main.rs` 显式 `new_current_thread()`。工作负载 I/O 受限，故用单线程 `join_all`（而非 OS 线程）做只读并发，用 `Rc<RefCell>`/`Rc<Mutex<McpClient>>`（而非 `Arc<Mutex>` + `Send+Sync+'static`）共享 plan / 工具目录 / hints / 事件 sink / mock provider。这是一个一旦选定就无处不在的承重决策。

2. **OpenAI 历史合法性不变式**（贯穿 1 Harness 回填 · 2 上下文压缩前缀丢弃 · resume 净化）。"每个 assistant `tool_call` 必须有匹配的 tool 回复，且历史不得以孤立 tool 消息开头，否则下一次请求 400"——三处协同维护它:Harness 在 cancel/预算耗尽时 `backfill_unanswered_tool_calls` 合成 `[interrupted…]` 回复；**上下文压缩(2)** 的前缀丢弃把切点推进到 user 边界并跳过打头的孤立 tool 消息；`resume` 的 `sanitize_restored_messages` 为悬空 tool_call 回填占位。三个子系统对同一条 API 形态不变式负责。

3. **"模型是第一杠杆，harness 是第二杠杆"哲学**（贯穿 5 编排器 · 8 ncx-forge，并由 4 沙箱托底）。两处都明说"能力瓶颈在模型而非 harness"：**编排器(5)** 不靠更聪明的模型，而靠**结构**（best-of-N、闭环 verify-retry、递归分解）把廉价模型省下的成本换成可靠性，并坦承 plan/verify 跑在主模型上、无法越过其推理上限；**ncx-forge(8)** 离线演化的也只是 harness 的**文本**（prompt + 工具描述），不动行为。同一信念的两个方向；而 **沙箱(4)** 把"执行始终受控"作为托底，使无论哪个杠杆都改不动安全边界。

---

**关键文件**（绝对路径）:`D:/agent_prac/nanocodex/rust/crates/ncx-cli/src/main.rs`（阶段 A 装配 + 阶段 B `run_one_turn` + `run_orchestrated`）、`D:/agent_prac/nanocodex/rust/crates/ncx-core/src/lib.rs`（核心 crate 的 re-export 接线面）、`D:/agent_prac/nanocodex/docs/design_data.zh.json`（8 子系统 flow_steps + key_mechanisms 出处）。
