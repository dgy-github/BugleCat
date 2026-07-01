# nanocodex 系统画像（重建版）

> 用途：系统性重建 nanocodex 全栈细节的面试应答材料。全部内容 **grounded 在真实源码** 并经过"敌意 fact-checker"逐条对抗校验（数字、函数名、控制流、解析回退都核对过源码行号）。
>
> **怎么用**：先读"第 0 章·连接图"建立整体心智 → 逐个子系统过"一句话主线 + 30秒/2分钟/深挖三档" → 用每章末"自测·主动回忆"先盖答案自答 → 考前只过"速记页"(见 nanocodex-portrait-cram.zh.md)。
>
> **每张卡的结构**：一句话主线 · 深度阶梯(30秒/2分钟/深挖) · 核心机制支柱 · 关键数字必背 · 取舍与坑 · 高频追问与应答 · 自测(L1-L4) · 别发散到这。
>
> 代码标识符保留英文，叙述用中文。行号引用对应主 checkout `D:/agent_prac/nanocodex` 在生成时的源码。

## 目录

1. [系统画像 · 跨子系统连接图](#连接图)（先读这章）
2. Harness 工程管理
3. 上下文压缩 · context editing
4. 工具系统 · 动态暴露
5. 沙箱 · 审批状态机
6. 主子 agent 如何通讯
7. 项目记忆 · 自进化
8. Skills · MCP · 视觉
9. ncx-forge · 骨架训练框架

---

<a id="连接图"></a>

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


---

## Harness 工程管理

### 一句话主线
整个 harness 就是一个**有预算上限、可取消、且永远让对话保持 API 合法状态的单一回合循环**——`call_model -> run_tools` 交替推进，直到模型无 tool_call 自然结束、或双层预算触顶强制终止;其余所有工程决策(并发、schema 选择、context 编辑、hooks、错误映射)都是挂在这条循环上的约束,目的只有三个:让它 (a) 一定终止、(b) 可被打断、(c) 每次发出的请求 provider 都接受。这是一条主线,不是功能清单。

### 答题骨架:7 根支柱
按顺序背,每根一句话,讲完就停。

1. **预算有界循环 (TaskBudget)** — `for iteration in 0..max_model_calls` 驱动 `call_model->run_tools`,无 tool_call 即 `completed`,否则跑工具;双上限 + 每轮注入 `budget_note()` 让模型自我节流 — 因为 LLM 循环没有天然停止条件,双上限保证一定终止,把硬切断变成协作式收尾。
2. **两层协作式取消** — 边界 `cancel_check` 轮询(每轮顶部 / 每个工具前后) + `execute_cancellable` 用 biased `select!` 对工具 future 计时 — 因为光靠边界轮询打不断一个永久挂起的工具,计时 + drop future 让它可被放弃。
3. **正确性闸门下的并发 (parallel_run)** — 连续只读工具才 `join_all` 并行,写/未知工具打断成串、单独串行执行 — 因为读之间无依赖可重叠降延迟,而读之间夹一个写若被重排会破坏状态。
4. **每轮动态 tool-schema 选择 (schemas_for_query)** — 每轮按 query 选出固定核心工具 + `tool_search` 请求的 + 词法 top 命中,封顶 9 个 — 因为大注册表撑爆 prompt 又降低选工具准确率,固定核心保证模型永远能读/改/跑/再搜。
5. **非破坏性 context 编辑视图 (for_model_edited)** — 系统提示 + 临时 per-turn notes + 受字符预算压缩的正文,只是发送时的瞬态视图,真实 history 不动 — 因为要既压住载荷又不污染 `--resume`/`/compact`,临时 note 持久化会毒化后续回合。
6. **API 不变量稳健性 (backfill)** — 未知工具返回错误串不 panic、取消或预算耗尽时 `backfill_unanswered_tool_calls` 给每个悬空 tool_call 补占位回复、`event_sink` take/restore 避免借用冲突 — 因为 OpenAI 式 API 一旦有 tool_call 无对应 tool reply 就 400。
7. **Hooks 守回合边界 + 图像路由 vision** — 模型调用前跑 UserPrompt hook(blocked 则零模型调用短路),回合后跑 Stop hook;`use_vision_this_turn` 仅对带图回合切到 vision provider — 因为 hooks 是不碰循环代码的策略扩展缝,仅图像回合路由保持文本回合走主模型。

### 每根支柱的"再深一层"
被追问时才放,放一两句即收。

1. **预算** — `max_model_calls=60` 封外层 for 循环、`max_tool_calls=120` 在内层用 `remaining_tools = max_tool_calls.saturating_sub(tools_used.len())` 检查,==0 即 `budget_result`;两者在不同层,所以当 `max_tool_calls` 小于一次模型回合吐出的 tool_call 数时,这批调用会被**截在列表中间**并 backfill,而不是等下一次模型调用。注意只读批次还**额外**受 `batch.len() < remaining_tools`(line 469)约束截断,不只靠每轮顶部 `remaining_tools == 0`(line 456)的早返回。
2. **取消** — 第二层是 `tokio::select!` 对工具 future 与 100ms sleep tick 二选一;`biased`(line 271)让每次唤醒都**先 poll future 臂**(line 272)、再 poll 计时器臂,cancel 标志只在计时器臂被采样,所以任何在 100ms tick 前 resolve 的 future 都返回真实结果、与 cancel 无关,翻转后才在 tick 上 drop future 返回 `[interrupted: stopped by user mid-command]`。
3. **并发** — `parallel_run` 仅当 `calls[idx]` 与 `calls[idx+1]` **都** `is_read_only` 才为真;并行结果通过 `batch.iter().zip(results)` 按原始调用顺序缝回,所以模型永远按调用序看到 tool reply。
4. **schema** — `DEFAULT_VISIBLE_TOOL_LIMIT=9`;总数 ≤9 时全显,否则 = 固定 `ALWAYS_VISIBLE_TOOLS`(read_file/apply_patch/update_plan/shell/tool_search/skill) + `ctx.tool_hints` + 词法 `catalog_score` top 命中填空位。注意词法回填在非空 query 下遇到 `catalog_score` 不为正即停(`if score > 0 || q.is_empty()`,line 371),所以注册表很大时**视图也可能少于 9 个**,并非总填满到上限。
5. **context 视图** — `budget_note` 和 memory recall note **从不写入** `session.messages`(测试断言其缺席);模型看到的 transcript ≠ 存储的 transcript。
6. **backfill** — 占位回复以 assistant 消息的 `tool_calls[].id` 为键,只修模型真正发出的 call;`event_sink` 在 `run_turn` 入口 take()、唯一返回路径 restore,使 sink 能作局部 `&mut` 穿过,不与 `&mut self`/`&self` provider 借用冲突。
7. **hooks** — UserPrompt `blocked` 时 `iterations:0`、零模型调用,但仍记一条 assistant 消息保持 history 一致。

### 不要发散到这里
点到名字就走,不要展开。

- **Provider/传输内部**:DeepSeek HTTP、SSE 流式、重试 — 循环只看 `finish_reason=='error'`,HTTP 错被映射成 `ModelResponse{finish_reason:'error'}` 而非抛异常,到此为止。
- **单个工具实现**:apply_patch 的 diff 解析、shell 沙箱、web_search — 循环把每个工具当作不透明的 `async fn(args)->String`。
- **context 压缩/编辑算法内部**:那是 `session.rs` / `for_model_edited` 自己的子系统,循环只负责"调用它",压缩策略细节是另一题。
- **memory store 的 recall 排序、`catalog_score` 的精确权重(100/50/20)** — 知道有词法打分即可,不背权重。
- **MCP、GUI/Tauri bridge plumbing、`LoopEvent` 在前端的渲染** — 名字带过。

只守四个轴:**循环控制(终止)、安全(取消 + 写串行)、预算(双上限 + 自节流 note)、历史合法性(每个 tool_call 有回复、不 panic、note 临时)**。

### 高频追问与应答

- **怎么保证一定终止?** — 双独立上限:`max_model_calls=60` 封外层迭代、`max_tool_calls=120` 在内层逐工具检查;两个维度(推理调用 + 副作用调用)分别封顶,任一触顶即返回 `task_budget`。有 `stops_at_max_iterations` 测试兜底。注意 `with_max_iterations` 与 `with_task_budget` 互相覆盖模型调用上限,最后一个 builder 调用生效,但二者对下限处理**不同**:只有 `with_task_budget` 把 `max_model_calls` floor 到 1(`.max(1)`,line 200),所以预算传 0 仍跑一轮;而 `with_max_iterations` **不做任何 floor**,且运行期上限是 `max_iterations.min(task_budget.max_model_calls.max(1))`(lines 351-353),`.max(1)` 只兜 task_budget 一侧——因此 `with_max_iterations(0)` 得到有效上限 `0.min(0.max(1)) = 0`,`for 0..0` 一轮都不跑,直接落到 task_budget 返回。所以"预算 0 仍跑一轮"只在 `with_task_budget` 路径成立,不普遍成立。
- **取消的粒度和盲区?** — 粒度 100ms(`select!` 的 sleep tick),stop 最多滞后约 100ms;**快于 100ms 的工具一定跑完**——因为 `biased` 让 future 臂每次唤醒都先被 poll,cancel 只在计时器臂采样,任何在 tick 前 resolve 的 future 都返回真实结果,即按设计无法打断亚 100ms 工具。边界轮询负责工具之间,`execute_cancellable` 负责工具运行中。
- **为什么只并行只读?** — 读之间无相互依赖,`join_all` 重叠只为降延迟(`read_only_calls_run_concurrently`: 4×300ms < 800ms);但两读之间夹一个写,并行会把写相对读重排而破坏状态,所以写一律单独串行、保序(`write_between_reads_stays_serial_and_ordered` 锁死 `[r1,w1,r2]`)。`read_only` 是个**信任边界**,不是验证——工具谎报只读且写入就会被并发跑而 race。
- **历史怎么始终对 API 合法?** — OpenAI 式 API 在 assistant 有 tool_calls 却缺对应 reply、或某 `tool_call_id` 未应答时会 400;所以中途 stop/预算触顶必须 `backfill_unanswered_tool_calls` 补占位 tool 消息,这些占位是**真 tool 消息**(纯字符串如 `[interrupted: ...]`),下次请求才校验通过。未知工具也返回错误串而非 panic,让模型自我恢复。
- **per-turn 的 note 会污染后续吗?** — 不会。`budget_note`、memory recall、UserPrompt hook 输出只活在 `for_model_edited` 的瞬态视图里,从不落 `session.messages`;持久化一条过期 budget/recall note 才会毒化后续回合,所以刻意保持临时。
- **终止状态怎么对外汇报?** — 唯一契约是 `TurnResult.stop_reason` 字符串:`completed | task_budget | cancelled | blocked | error`,CLI/GUI 只读这一个。

### 自测 · 主动回忆

1. **[L1·理解]** 这条 harness 主线被概括为"有预算、可取消、永不让对话失效"的单一回合循环。请说出它交替推进的两个步骤,以及循环自然结束(而非被强制终止)的判定条件。

2. **[L2·权衡]** 双上限 `max_model_calls=60` 与 `max_tool_calls=120` 处在循环的不同层。当一次模型回合吐出的 tool_call 数超过当前 `remaining_tools` 时,会发生什么?为什么这样设计而不是"等下一次模型调用再截"?

3. **[L4·开放设计]** 取消机制为何要分"边界轮询"和 `execute_cancellable` 两层?只保留其中一层会各自漏掉哪种情况?

4. **[L3·故障]** 一个耗时 80ms 的工具,在用户已点击 stop 的情况下,会被打断吗?请结合 `biased select!` 的 poll 顺序与 cancel 标志的采样位置说明,并指出这个盲区的粒度。

5. **[L2·权衡]** `parallel_run` 把"只读"当作信任边界而非验证。如果一个工具谎报 `is_read_only` 实则写盘,会出什么问题?为什么写工具(或两读之间夹的写)必须单独串行且保序?

6. **[L3·故障]** 中途 stop 或预算耗尽时,若不调用 `backfill_unanswered_tool_calls`,下一次请求会怎样失败?占位回复以什么为键、补的是真 tool 消息还是别的?

7. **[L2·权衡]** 每轮 `schemas_for_query` 封顶 9 个工具。为什么不直接把整个工具注册表塞进 prompt?在注册表很大时,实际可见工具数会不会总是填满到 9?

8. **[L1·理解]** `with_max_iterations(0)` 和 `with_task_budget(0)` 对"是否至少跑一轮"的结果不同。分别说明各自跑几轮,以及差异的根源。

**答案要点**

1. 交替推进 `call_model -> run_tools`;模型返回**无 tool_call** 时即 `completed` 自然结束(否则跑工具继续循环)。强制终止来自双上限触顶。

2. 内层用 `remaining_tools = max_tool_calls.saturating_sub(tools_used.len())`,`==0` 即 `budget_result`;当 tool_call 数超过 `remaining_tools` 时,这批调用被**截在列表中间**,悬空的 call 由 backfill 补占位,而非等下一次模型调用。两上限在不同层(外层 for / 内层逐工具),所以截断发生在同一回合内。只读批次还**额外**受 `batch.len() < remaining_tools`(line 469)约束,不只靠每轮顶部 `remaining_tools == 0`(line 456)早返回。

3. 边界 `cancel_check` 只在每轮顶部/每个工具前后轮询,打不断一个**永久挂起的工具**;`execute_cancellable` 用计时 + drop future 负责"工具运行中"的打断。只留边界层 → 挂死的工具无法放弃;只留计时层 → 工具之间的取消点缺失。两层分别覆盖"工具之间"与"工具内部"。

4. **不会被打断**。`biased`(line 271)让每次唤醒都**先 poll future 臂**(line 272)再 poll 100ms 计时器臂,cancel 标志只在计时器臂采样;80ms < 100ms 的 future 在 tick 前就 resolve,返回真实结果、与 cancel 无关。盲区粒度 100ms——按设计无法打断亚 100ms 工具;翻转后才在 tick 上 drop future 返回 `[interrupted: stopped by user mid-command]`。

5. 谎报只读且写入的工具会被**并发跑而 race**(`read_only` 是信任边界,不验证)。写若与读重排会破坏状态,故写一律单独串行并保序;`parallel_run` 仅当 `calls[idx]` 与 `calls[idx+1]` **都** `is_read_only` 才并行,结果用 `batch.iter().zip(results)` 按原始调用序缝回(`write_between_reads_stays_serial_and_ordered` 锁死 `[r1,w1,r2]`)。

6. OpenAI 式 API 在 assistant 有 tool_calls 却缺对应 reply / 某 `tool_call_id` 未应答时会 **400**;`backfill_unanswered_tool_calls` 给每个悬空 call 补占位。占位以 assistant 消息的 `tool_calls[].id` 为键,补的是**真 tool 消息**(纯字符串如 `[interrupted: ...]`),只修模型真正发出的 call。

7. 大注册表会撑爆 prompt 又降低选工具准确率;`DEFAULT_VISIBLE_TOOL_LIMIT=9`,= 固定 `ALWAYS_VISIBLE_TOOLS`(read_file/apply_patch/update_plan/shell/tool_search/skill,保证永远能读/改/跑/再搜) + `ctx.tool_hints` + 词法 `catalog_score` top 命中填空位。非空 query 下词法回填遇 `catalog_score` 不为正即停(`if score > 0 || q.is_empty()`,line 371),所以**视图可能少于 9 个**,并非总填满。

8. `with_task_budget(0)` 仍跑**一轮**:它把 `max_model_calls` floor 到 1(`.max(1)`,line 200)。`with_max_iterations(0)` 跑**零轮**:它不做任何 floor,运行期有效上限 `max_iterations.min(task_budget.max_model_calls.max(1)) = 0.min(0.max(1)) = 0`(lines 351-353),`for 0..0` 一轮都不跑,直接落到 task_budget 返回。根源:`.max(1)` 只兜 task_budget 一侧,故"预算 0 仍跑一轮"只在 `with_task_budget` 路径成立。

### 一句话收尾
记住:这是**一条"有预算、可取消、永不让对话失效"的循环**,其它一切都是挂在它上面的约束——答题时先抛主线、按 7 根支柱推进、被探到才下钻,绝不在 provider、单工具、压缩算法里游走。


---

## 上下文压缩 · context editing

### 一句话主线 (the single thesis the candidate opens with — prevents both shallowness and sprawl)

> "context editing 是 `Session::edited_body` 在**发送时**算出来的一个**非破坏性历史视图**:用 `.chars().count()`(不是 token)做预算,两步走 —— 先把 keep_recent 窗口之外的旧 `tool` 结果截断到 `max_tool_result_chars`,再在超预算时按干净边界丢弃最老的前缀;`self.messages` 永不改动,只有 `/compact` 走 `compact` 把它落盘。"

一句话锁死四个考点:**send-time、非破坏、字符代理、两步算法**。后面所有追问都挂在这根主线上。

---

### 30 秒 / 2 分钟 / 深挖 三档 (a depth ladder — each level adds specific named mechanisms & numbers, so the candidate scales to the interviewer's probing instead of stopping short)

**30 秒档(机制骨架):**
context editing 由 `ContextEditPolicy{enabled, max_chars=120k, keep_recent_messages=30, max_tool_result_chars=4k}` 驱动。`edited_body` 两步:Pass 1 压缩旧 tool 结果,Pass 2 超预算时丢最老前缀。`for_model_edited` 把它做成临时视图发给 provider,`self.messages` 不动。`/compact` 是它的破坏性孪生,会落盘。

**2 分钟档(加机制名 + 数字):**
- 预算用 **字符代理**:`json_chars` = `serde_json::to_string(msg)` 后的 `.chars().count()`,**没有 tokenizer**。`estimate_tokens`(2 chars/token)是另一套、只给 UI 看的、**不参与门控**。
- **Pass 1**:`recent_cutoff = len - keep_recent_messages`(saturating),对 `i < recent_cutoff` 且 `role=="tool"` 的消息调 `compress_tool_result`:内容是字符串且长于 `max_tool_result_chars`(4k)时,`.chars().take(max_tool_result_chars)` 留头,追加 `[context edited: omitted the rest of prior {name} result; original_chars=N]`。注意:`compress_tool_result` 内部那个本地参数名叫 `max_chars`,但调用处传的是 `policy.max_tool_result_chars`(4k),**不是** 120k 预算 —— 阈值和留头长度是**同一个旋钮**(4k)。
- **Pass 2**:仅当 `total_chars > max_chars` **且** `body.len() > keep_recent` 才触发。`start = len - keep_recent` → 前进到窗口内第一个 `user` → 跳过开头的 `tool` → 若 `0<start<len` 丢前缀并记 `dropped_messages=start`。
- `compact` 复用同一算法,force `enabled=true`,**仅当** `compressed_tool_results>0 || dropped_messages>0` 才覆盖 `self.messages` 并 `rewrite_log()` 重写 JSONL。
- `call_model` **每次迭代**都重算视图(`agent_loop.rs:238-258`),因为一个 turn 内追加 tool 结果会让历史增长。

**深挖档(配置 + 完整性 + 非显然点):**
- 四个旋钮在 `config.rs:98-101/138-141`,通过 `positive_usize(value, fallback)` 映射:`<=0` 或溢出的 i64 **静默回退**到默认 —— 所以 `keep_recent=0`/`max_chars=0` **设不出来**,只有 `enabled=false` 真能关。
- **完整性三件套**:`sanitize_restored_messages`(为孤儿 tool_call 补合成 tool 回复)、`backfill_unanswered_tool_calls`(运行时 cancel/budget 路径)、`redact_image_data`(`data:` 图片转占位)。Pass 2 跳过开头 tool 与这三者守同一个不变量:**每个 tool_call 都有配对 tool 回复 / 没有孤儿 tool**。
- `resume()`/`fork()` 在 sanitize 之前先 **filter 掉 `role=="system"`**,系统提示故意丢弃、用新建的替换。
- 只有 **一套** editing 算法:`enabled` 标志 + caller 是否把结果赋回去,决定了恒等视图 / 临时编辑 / 破坏性落盘三种语义。

---

### 核心机制 · 7 根支柱 (a BOUNDED list — each pillar: 名字 — 机制一句 — 为什么一句.)

1. **ContextEditPolicy + Stats** — 值类型策略 `{enabled, max_chars, keep_recent_messages, max_tool_result_chars}` + `ContextEditStats{original_chars, edited_chars, compressed_tool_results, dropped_messages}`(`session.rs:16-41`)。 — 一个值类型策略让同一算法靠翻 `enabled` 同时服务临时视图和破坏性落盘;默认值与 Python Config / config.rs 完全对齐。

2. **字符代理预算** — `json_chars` = 序列化 JSON 的 `.chars().count()`,`total_chars` 累加 system + notes + 每条 message(`session.rs:443-453`)。 — 不依赖 tokenizer、不联网,每个 turn 都能廉价、确定、离线地算出编辑决策;数 JSON 框架/键大致对应 token 会计入的部分。

3. **Pass 1 — 压缩窗口外旧 tool 结果** — `compress_tool_result` 对 `recent_cutoff` 之前、`role=="tool"` 且字符串长于 `max_tool_result_chars`(4k)的内容用 `.chars().take(max_tool_result_chars)` 留头 + `original_chars=N` 标记(`session.rs:217-225, 493-513`)。阈值和留头长度是**同一个** `max_tool_result_chars`(在函数内被混淆地命名为本地参数 `max_chars`,但绑定的是 4k 值,不是 120k 预算)。 — 旧 tool 转储是上下文膨胀主因,但近期的仍在被推理;`.chars().take` 而非字节切片是为了不在 UTF-8 多字节中间 panic;标记告诉模型被截断了多大,可重新取。

4. **Pass 2 — 干净边界丢最老前缀** — 超预算且 `body.len()>keep_recent` 时,从 `len-keep_recent` 前进到第一个 `user`、跳过开头 `tool`,丢前缀记 `dropped_messages`(`session.rs:227-241`)。 — 必须落在自洽边界:从 `user` 起每个保留 turn 自包含,跳开头 `tool` 防止孤儿 tool 回复(其 assistant tool_call 刚被丢)让历史不符合 OpenAI schema。

5. **for_model_edited — 非破坏视图** — 算出 `(body, stats)` 后拼 `[system] + 每条非空 note 各自一条 system 消息 + body`,重算 `edited_chars`;`self.messages` 不动(`session.rs:167-187`)。 — 完整审计日志必须留在内存与磁盘上,编辑只作用于本轮发出去的内容;notes(预算/hook/记忆)作为临时 system 消息注入,也不持久化。

6. **compact — 破坏性落盘孪生** — clone policy、force `enabled=true`、跑 `edited_body`,**仅当真的变了**才覆盖 `self.messages` 并 `rewrite_log()` truncate+重写 JSONL(`session.rs:189-201, 321-348`)。 — `/compact` 必须持久化,让后续 turn 和 `--resume` 看到更小历史;no-op 守卫避免在未超预算时白白重写日志。

7. **call_model 每轮接线** — `call_model` 每次迭代调 `for_model_edited(system_notes, &context_edit)` 流式发送并返回 stats,喂给 NCX_TRACE(`agent_loop.rs:238-258, 367-392`)。 — 一个 turn 内追加 tool 结果会让历史在单轮内跨过预算,所以编辑必须每次模型调用都重算,而非只跨 turn。

---

### 关键数字 / 必背细节 (the exact constants, thresholds, function names to nail)

- **默认值**:`enabled=true`、`max_chars=120_000`、`keep_recent_messages=30`、`max_tool_result_chars=4_000`(`session.rs:24-33`,与 `config.rs:138-141` 一致)。
- **Pass 2 触发条件(必背)**:`total_chars(system,notes,body) > max_chars` **AND** `body.len() > keep_recent_messages`(`session.rs:227-229`)。
- **Pass 1 cutoff**:`recent_cutoff = body.len().saturating_sub(keep_recent_messages)`,只动 `i < recent_cutoff`。
- **Pass 1 阈值 + 留头长度(同一旋钮)**:都用 `max_tool_result_chars=4_000`;`compress_tool_result` 仅当 `content.chars().count() > 4_000` 才压(`<= 4_000` 不压),压时 `.chars().take(max_tool_result_chars)` 留头(`session.rs:500-503`)。函数内本地参数名叫 `max_chars` 是误导,绑定的是 4k,**不是** 120k 预算。
- **截断标记原文**:`[context edited: omitted the rest of prior {name} result; original_chars={N}]`,`name` 默认 `"tool"`,N 是真实 `.chars().count()`。
- `estimate_tokens`:`CHARS_PER_TOKEN=2` + 每条 message `+8` 框架开销(`session.rs:461-491`)—— **UI 专用,不门控编辑**。
- 干扰项(别和编辑预算混):`context_token_budget=512_000`、`context_window=1_048_576`(`config.rs:136-137`)都是 UI,不被编辑器使用。
- 记忆 note 上限:`MEMORY_RECALL_MAX_ENTRIES=8`、`MEMORY_RECALL_MAX_CHARS=4_000`(`agent_loop.rs:20-21`)。
- `positive_usize(value, fallback)` = `usize::try_from(value).ok().filter(|v|*v>0).unwrap_or(fallback)`(`main.rs:1205-1210`):`<=0`/溢出 → fallback。
- 关键函数名:`edited_body`、`for_model_edited`、`for_model`、`compact`、`compress_tool_result`、`json_chars`/`total_chars`、`rewrite_log`、`sanitize_restored_messages`、`backfill_unanswered_tool_calls`、`redact_image_data`、`context_edit_from_config`。

---

### 取舍与坑 (design trade-offs + soft spots — the material for follow-ups.)

- **字符 ≠ token ≠ 字节**:用 `chars().count()` over 序列化 JSON 做预算。CJK-heavy / code-heavy 文本的真实 token 数会偏离这个 120k 字符门 —— 真实 prompt 大小会漂移。`estimate_tokens` 是**另一套**计算,不是门控路径。
- **两套字符门、两个旋钮别混**:Pass 1 的阈值与留头长度都是 `max_tool_result_chars`(4k,per-result 上限);Pass 2 的超预算判定才用 `max_chars`(120k,整段预算)。函数内 `max_chars` 这个本地参数名是误导 —— Pass 1 实际留的是 4k 头,不是 120k。
- **`keep_recent=0` / `max_chars=0` 设不出来**:`positive_usize` 把它们当"未设"静默回退成 30 / 120_000。只有 `context_edit_enabled=false` 真正关闭。这是有意的(0 会压/丢活跃 turn),但对想极限调小的用户是反直觉的。
- **Pass 1 与 Pass 2 独立**:压缩可能已把总量压到 max_chars 以下,丢前缀就不触发。但压缩看 **content 字符串**的 `chars()`(`<= 4k 不压`),超预算看整条消息的 **`json_chars`** —— 一个刚好 `<= 4k` 的 tool 结果不被压,却仍带着 JSON 框架计入预算。
- **`compress_tool_result` 只对字符串 content 生效**(`session.rs:497`):tool 结果若存成 JSON 数组/对象 content,会被**整个跳过**,不压缩。
- **Pass 2 找 user 不向窗口前搜**:用 `body[start..].position(role==user)`;若近期窗口内**没有** user 消息,`start` 停在 `len-keep_recent` 再跳开头 tool —— 即丢到 keep_recent 边界(减去开头 tool)。
- **`redact_image_data` 只脱敏 `type=="image_url"` 且 url 以 `data:` 开头**(`session.rs:423-428`):http(s) 图片 URL 原样落盘,非数组 content 原样返回。
- **`/compact` 会丢失原始时间戳**:`rewrite_log` 给每条幸存消息重打一个新 `_ts`(`session.rs:343`),compact 后所有消息都是 compaction 时刻的戳。
- **compact 与运行时视图的 char 总量会有微差**:`compact` 用 `edited_body(&[], policy)` **不带 notes**,而运行时 `for_model_edited` 带 budget/hook/memory notes —— 触发前缀丢弃的总量略有不同。

---

### 高频追问与应答 (4-6 likely follow-ups, each with a crisp model answer.)

**Q1:为什么不用 tokenizer,用字符数?**
A:编辑决策要廉价、确定、离线、每轮都能跑,引 tokenizer 要么加依赖要么联网。`chars().count()` over 序列化 JSON 是稳定的跨平台代理,还顺带数进了 JSON 框架/键(token 会计也大致计入)。代价是 CJK/code 文本真实 token 会漂移 —— 这是已知 trade-off,不是 bug。

**Q2:为什么 Pass 2 要前进到 user、再跳开头 tool,不直接砍 `len-keep_recent`?**
A:为了让截断后的历史仍符合 OpenAI tool-call 配对 schema。从 `user` 起保证每个保留 turn 自包含;开头若留着 tool 回复、而它配对的 assistant tool_call 刚被丢进前缀,就成了孤儿 tool,schema 会拒。这跟 `sanitize_restored_messages`/`backfill_unanswered_tool_calls` 从反方向守的是同一个不变量。

**Q3:`/compact` 和自动 editing 什么关系?为什么能随便点?**
A:同一个 `edited_body`,`/compact` 走 `compact` force `enabled=true` 并把结果赋回 `self.messages` + `rewrite_log()`。它有 no-op 守卫:**仅当 `compressed_tool_results>0 || dropped_messages>0`** 才重写 JSONL,所以未超预算时反复 `/compact` 是纯空操作,不动磁盘、不动时间戳(`compact_noops_when_under_budget` 测了)。

**Q4:一个 turn 内会发生编辑吗?**
A:会。`call_model` **每次模型迭代**都重算 `for_model_edited`,因为 turn 内不断追加 tool 结果,单轮就可能跨过 120k 预算 —— 同一 user turn 的连续两次模型调用看到的历史可能不同。

**Q5:`self.messages` 到底动不动?哪条路径会落盘?**
A:自动 editing(`for_model_edited`)永不动 `self.messages`,只产出临时发送视图,磁盘日志保持完整审计。**只有** `/compact` 走 `compact` 会覆盖 `self.messages` 并 truncate+重写 JSONL,让 `--resume` 看到压实后的历史。

**Q6:配置 `max_chars=0` 想彻底关编辑,行不行?**
A:不行。`positive_usize` 把 `0`/负/溢出当"未设",静默回退到 120_000。想真正关只有一条路:CLI `--disable-context-edit`(把 enabled 置 `Some(false)`)或配置 `context_edit_enabled=false`。三个数值旋钮是 `Option<i64>` 覆盖,enabled 被特判。

---

### 自测 · 主动回忆 (5-8 self-test questions tagged [L1]-[L4])

1. **[L1]** `ContextEditPolicy` 的四个字段和默认值分别是什么?
2. **[L1]** Pass 1 压缩一个旧 tool 结果时,留下什么、追加什么标记?它用的是哪个旋钮 —— max_chars 还是 max_tool_result_chars?
3. **[L2]** Pass 2 触发需要哪两个条件同时成立?为什么要前进到 `user`、跳开头 `tool`?
4. **[L2]** `for_model_edited` 和 `compact` 都用 `edited_body`,语义为何相反?靠什么区分?
5. **[L3]** 配置里把 `keep_recent_messages` 设成 0 会发生什么?为什么这么设计?
6. **[L3]** 一个 tool 结果刚好 3.9k 字符,Pass 1 不压它,但它仍可能把会话推过 max_chars,为什么?压缩门和超预算门各用哪个字符量?
7. **[L4]** 编辑用字符代理而非 token,在什么输入下真实 prompt 大小会偏离 120k 门?这是 bug 吗?
8. **[L4]** 截断后的历史如何保证仍能合法发给 provider?有哪三个机制守同一不变量?

**答案要点(看完再对):**
1. `enabled=true`、`max_chars=120_000`、`keep_recent_messages=30`、`max_tool_result_chars=4_000`(`session.rs:24-33`)。
2. 留 `.chars().take(max_tool_result_chars)` 的头(即前 4k 字符),追加 `[context edited: omitted the rest of prior {name} result; original_chars=N]`,N 是真实字符数,name 默认 `"tool"`。用的是 **`max_tool_result_chars`(4k)**,不是 120k 的 `max_chars` —— 函数内本地参数虽叫 `max_chars`,但调用处传的是 `policy.max_tool_result_chars`,绑定 4k。
3. `total_chars > max_chars` **且** `body.len() > keep_recent_messages`。前进到 `user` 让每个保留 turn 自包含;跳开头 `tool` 防止孤儿 tool 回复破坏 OpenAI tool-call 配对 schema。
4. 区分靠 `enabled` 标志 + caller 是否把 body 赋回 `self.messages`。`for_model_edited` 产临时视图、不赋回(非破坏);`compact` force enabled + 赋回 + `rewrite_log()`(破坏性落盘)。同一套算法。
5. 设不出来。`positive_usize` 把 0 当"未设",回退到 30。因为 `keep_recent=0` 会压缩/丢弃正在进行的活跃 turn,是退化行为,故意禁止。
6. 因为 Pass 1 压缩看的是 **content 字符串**长度(`<= 4k 不压`,用 `max_tool_result_chars`),而超预算判定用整条消息的 **`json_chars`**(含 JSON 框架/键,与 `max_chars=120k` 比),多条这样的消息累加 `total_chars` 仍可超 `max_chars`。
7. CJK-heavy 或 code-heavy 文本:它们的真实 token 数与字符数比例偏离,而门控只数 `chars()`,所以真实 prompt 大小会漂移。不是 bug,是已知 trade-off(换取无 tokenizer、可离线、确定性)。
8. Pass 2 丢前缀时跳开头孤儿 `tool`;`sanitize_restored_messages` 在 resume 时为缺回复的 tool_call 补合成 tool 消息;`backfill_unanswered_tool_calls` 在运行时 cancel/budget 路径补。三者守"每个 tool_call 有配对 tool 回复 / 无孤儿 tool"。

---

### 别发散到这 (a short DO-NOT list)

- **tokenizer / 真实计费 token 数** —— 编辑器**不用** tokenizer;`estimate_tokens`(2 chars/token)是 UI 数字,属另一条路径,点到为止。
- **`context_token_budget=512_000` / `context_window=1_048_576`** —— UI 预算与窗口展示,**不被编辑器使用**,别拿来当编辑门控讲。
- **memory recall / prompt hook 的内容生成** —— 它们只是作为 `system_notes` 注入,属 agent_loop 记忆子系统,这里只关心"作为 note 计入 char 预算且不持久化"。
- **resume / fork 的完整流程** —— `read_log`、filter `system`、`redact_image_data` 属会话恢复子系统;这里只引用它们**与 Pass 2 守同一 tool-call 配对不变量**这一点。
- **provider 流式细节 / NCX_TRACE 输出格式** —— 属 provider 与可观测性子系统;这里只说 stats 被喂给它。

### 一句话收尾.

记住主线:**send-time 算出的非破坏视图,字符代理预算,两步(压旧 tool 到 max_tool_result_chars / 超 max_chars 丢老前缀),`self.messages` 不动 —— 只有 `/compact` 落盘** —— 沿这根脊柱按 30 秒 / 2 分钟 / 深挖三档伸缩,既不会答浅,也不会跑偏。


---

## 工具系统 · 动态暴露

### 一句话主线
动态暴露的本质是：**当注册工具数 > 9 时，每一轮只把"必备核心集 ∪ 上一次 tool_search 写入的 hints ∪ 本轮 user prompt 的词法匹配"这三路并集裁剪到 9 个 schema 喂给模型**——用一个无需 embedding 的确定性词法打分器（100/50/20）控制 prompt 体积与选择质量，靠 `read_only` 标志驱动并发批处理、靠 `genome` 在注册期统一改写描述。

### 30 秒 / 2 分钟 / 深挖 三档

**[30 秒]** 工具都实现 `Tool` trait（`#[async_trait(?Send)]`，`execute` 永远返回 `String` 不返回 `Result`，错误即消息）。`ToolRegistry` 持有 `Vec<Box<dyn Tool>>` + `by_name` 索引。当 `tools.len() <= 9`（`DEFAULT_VISIBLE_TOOL_LIMIT`）时暴露全部 schema；超过则裁剪。裁剪 = 核心集（`ALWAYS_VISIBLE_TOOLS` 里**实际已注册**的 5–6 个名字，每个都过 `by_name.contains_key` 守卫）∪ `tool_hints` ∪ 词法匹配，填到 9 个为止。

**[2 分钟]** 加上三条机制：① **`tool_search`** 是发现通道——它 `read_only`，打分整个 catalog，`clear()` 后把命中名写进共享的 `ctx.tool_hints`，于是这些工具在**下一轮**被暴露。② **词法打分** `catalog_score`：名字精确匹配 +100、名字子串 +50、名字或描述子串 +20，按 query 词累加；空 query 全为 0 但仍按 `score>0 || q.is_empty()` 填满。③ **`read_only` 标志**双用途：agent loop 据此把连续只读调用合并成 `join_all` 并发批，写工具默认 `read_only=false` 永远串行。

**[深挖]** 再叠：④ **`genome` (NCX_GENOME) 描述覆盖**在 `register()` 时就 apply 到 catalog entry，并在 `schema_for` 时 apply 到模型 schema——保证 `tool_search` 的打分器看到的文本与模型看到的一致；空/损坏/空白覆盖一律 no-op 回落到硬编码默认。⑤ **`execute` 的 name dispatch + pre/post hooks**：未知名返回字符串 `Error: unknown tool '{name}'.` 不 panic；pre-hook `blocked` 短路成错误串，post-hook stdout 追加在 `[hook output]` 标记下。⑥ 核心集每个名字都用 `by_name.contains_key` 守卫——`skill`/`remember` 未注册时静默掉出，所以实际 seed 进去的是 5 或 6 个、不是固定 6 个。

### 核心机制 · 6 根支柱

1. **`Tool` trait — `?Send` async + `to_schema()`** — 声明 `name/description/parameters/read_only(默认 false)/execute`，默认 `to_schema()` 包成 OpenAI function tool；`execute` 返回 `String`。 — `?Send` 因为整个 REPL 跑在 current-thread tokio 上、共享态是 `Rc<RefCell<…>>`；返回 `String` 让任何失败都变成模型可读可恢复的消息而非崩溃。

2. **`schemas_limited_for_query` — 三路并集裁剪到 9** — `tools.len()<=9` 直接全发；否则先 seed 核心集（`ALWAYS_VISIBLE_TOOLS` 里**已注册**的名字，每个过 `contains_key`，实际 5–6 个），再 seed `tool_hints`，二者无条件保底；剩下的名位再按 `catalog_score` 排序（score desc, name asc）竞争填到 9。 — 全量 schema 会膨胀 prompt 并稀释工具选择；核心集必须常驻（编辑/读/跑/规划/发现的主循环不能消失），`tool_search` 本身必须可见以便重新发现其余。

3. **`tool_search` 写 `ctx.tool_hints`** — `read_only=true`，打分整个 catalog，`hints.clear()` 后把 top `max_results`（默认 8，clamp 1..20）的名字写进 hints + 输出行。 — 这是跨轮发现的唯一通道：它找到的工具在下一轮 schema 视图里出现；`clear()` 保证只反映最近一次搜索、防止无界累积。

4. **`catalog_score` 词法打分** — `tool_words` 小写、按非 `[alnum|_]` 切、trim `_`、留 len>=2、去重；名字 `eq_ignore_ascii_case` 词 +100、`contains` +50、否则 `name+' '+desc` contains +20，按词累加。 — 词法打分廉价、确定、无需 embedding，契合单二进制+快启动目标；名字匹配压过描述匹配（100/50 vs 20）让点名查询优先浮现。

5. **`read_only` → 并发批处理** — `is_read_only(name)` = `get(name).read_only()`，未知工具→`false`。agent loop 把连续两个及以上只读调用贪心聚成一段、用 `futures::join_all` 并发跑；该批的扩展还受**每轮工具预算 `remaining_tools` 上限**约束（不是只看下一个写/未知工具），遇到写工具或未知工具或预算耗尽即结束该批转串行。 — 只读无副作用，连发并发能降延迟且无顺序风险；写工具串行保证文件/状态确定顺序；未知→`false` 是安全默认（当成写）。

6. **`genome` (NCX_GENOME) 描述覆盖** — `schema_for` 与 `register` 都用 `ctx.genome.describe(name, default)`：有覆盖用覆盖、否则用硬编码默认；`parameters()` 与 `to_schema()` 不动。 — ncx-forge 只进化文本（system prompt + 工具描述）不改行为；注册期同步 apply 让 `tool_search` 打分器与模型展示一致；空/空白/损坏 NCX_GENOME 一律 no-op，坏候选不能静默改行为。

### 关键数字 / 必背细节

- `DEFAULT_VISIBLE_TOOL_LIMIT = 9`（裁剪触发阈值）— tools.rs:25
- `ALWAYS_VISIBLE_TOOLS = 6 个名字`：`read_file, apply_patch, update_plan, shell, tool_search, skill`；但实际 seed 进暴露的是其中**已注册**的 5–6 个，`skill` 仅在发现了 SKILL.md 时才算数 — tools.rs:26-33, 346-350
- 默认 `ToolRegistry::new()` 无条件注册 **9 个 base 工具**：`read_file, apply_patch, update_plan, shell, GrepTool, GlobTool, WebSearchTool, WebFetchTool, tool_search`；接了 memory 再 +1（`remember`）、有 skills 再 +1（`skill`）= **9 / 10 / 11** — tools.rs:251-274
- `catalog_score`：名字精确（`eq_ignore_ascii_case`）`+100`；名字子串 `+50`；名字\|描述子串 `+20`；按 query 词累加 — tools.rs:514-520
- `tool_words`：留 token len `>= 2`、去重 — tools.rs:496
- `tool_search` `max_results` 默认 `8`，clamp 到 `1..=20` — tools.rs:457-461
- 选择截止谓词：`selected.len() < limit && (score > 0 || query 为空)` — tools.rs:367-374
- `is_read_only` 未知工具→`false` — tools.rs:324-326
- 错误串字面量：未知工具 `Error: unknown tool '{name}'.`；pre-hook 阻断 `Error: {name} blocked by pre_tool hook.` — tools.rs:384-423
- post-hook 追加标记：`[hook output]` — tools.rs:384-423
- `register` 与 `schema_for` 都过 `genome.describe`；但 trait 自带 `to_schema()` 保持未改写默认 — tools.rs:229-238 vs 286-303/305-318

### 取舍与坑

- **核心集只在"已注册"时才强制可见**：每个 `ALWAYS_VISIBLE_TOOLS` 名字都被 `by_name.contains_key` 守卫，`skill`（仅有 SKILL.md 时注册）、`remember`（接了 memory 时）缺席时静默掉出——所以 seed 进去的核心名是 5 或 6 个、不是固定 6（tools.rs:271-273, 346-350）。
- **`tool_hints` 每次 `tool_search` 都 `clear()`**：只有最近一次搜索的命中留在暴露里，上一次发现的工具在下次搜索时消失（tools.rs:470-471）。
- **传给 `schemas_for_query` 的 query 是整轮的 user prompt 文本（`user_query_text`），不是 tool_search 的 query**：跨轮发现只走 `tool_hints`，词法 query 每轮固定（agent_loop.rs:307,367）。
- **空 query 路径**：`catalog_score` 对空 query 返回 0，但谓词是 `score>0 || q.is_empty()`，仍填满到 9——所以无 user 文本时视图是 核心集 + 任意 catalog 填充，而非只有核心集（tools.rs:371,504）。
- **genome 覆盖只到 registry 的 `schema_for` 和 catalog**：trait 自带 `to_schema()` 仍返回未改写默认——只有 registry 的 `schema_for` 是 genome-aware（tools.rs:229-238 vs 308-318）。
- **裁剪并非"很难触发"——默认就贴着阈值**：默认 `new()` 注册 9 个 base 工具，裸跑（9）正好 `<=9` 不裁剪；但**只要接了 memory 或 skills 任一**就到 10，已越过 `<=9` 阈值、裁剪立即激活；两者都有则到 11。所以"核心 agent 感觉总看得到全部"只在裸 9 的窄情形成立，一旦挂 memory/skill/MCP 就开始裁剪（tools.rs:251-274；阈值分支见 tools.rs:341-342）。
- **`McpTool.read_only` 由名字推断**而非声明：`is_read_only_name` 用**下划线后缀前缀** `read_/get_/list_/fetch_/search_/find_` 做 `starts_with`，外加一个**精确名匹配** `read|get|list|search|find`（注意精确这一支**没有 `fetch`**，前缀这一支也没有裸 `fetch`/`read` 等无下划线形式）——名字像只读但有写副作用的 MCP 工具可能被错误并发批处理（mcp_tool.rs:38-46）。

### 高频追问与应答

**Q1：为什么阈值是 9、核心集偏偏是这几个？**
A：9（`DEFAULT_VISIBLE_TOOL_LIMIT`）把每轮工具面积压小，避免全量 schema 膨胀 prompt、稀释模型选择。核心集是 agent 主循环——读(`read_file`)/写(`apply_patch`)/跑(`shell`)/规划(`update_plan`)/发现(`tool_search`)，外加按需的 `skill`——必须常驻，尤其 `tool_search` 不能消失，否则模型没法重新发现被裁掉的工具。注意核心集是"已注册才 seed"，所以实际常驻的是 5–6 个。

**Q2：被裁掉的工具，模型怎么再用到？**
A：调 `tool_search`。它打分 catalog、`clear()` 后把命中名写进共享 `ctx.tool_hints`，于是这些名字在**下一轮** `schemas_limited_for_query` 的 seed 阶段被纳入暴露。注意是下一轮——同轮不会立刻可见，且每次搜索都 `clear()`，只保留最近一次的命中。

**Q3：词法打分会不会误伤，比如 `lsof` 匹配 `ls`？**
A：打分用的是 `catalog_score`（100/50/20，子串都算），但只读判定用的 `looks_read_only` 是另一套——它要求 `==prefix` 或 `prefix+空格/tab`，所以 `lsof` 不匹配 `ls`。两者别混：一个管"暴露哪些"，一个管"要不要审批"。

**Q4：`read_only` 标志到底管几件事？**
A：双用途。① 驱动 agent loop 的并发批处理（`is_read_only`→`join_all`）；② `tool_search` 输出里的 `(read-only)` 标注（该标注源自 catalog entry 的 `read_only` 字段，注册期由 `tool.read_only()` 填入，所以两个用途都回溯到这一个标志）。但它**不** gate 沙箱写——沙箱写由 `SandboxPolicy.can_write` + approver 决定，与 `read_only` 标志正交。未知工具 `is_read_only` 返回 `false`，当成写、串行执行。

**Q5：genome 改了描述，会不会偷偷改了行为？**
A：不会。genome 只覆盖 description（`schema_for`/`register` 走 `describe`），`parameters()` 和执行逻辑原封不动；空/空白/不可读/格式错的 NCX_GENOME 一律 no-op 回落硬编码默认。这是 ncx-forge 训练的信任锚：坏候选 genome 不能静默改行为。

**Q6：为什么 `execute` 返回 `String` 而不是 `Result`？未知工具也不报错？**
A：让任何失败（坏参数、审批拒绝、解析错、幻觉工具名）都变成模型读得到的消息，turn loop 继续而不崩溃。未知名返回字符串 `Error: unknown tool '{name}'.`，模型可以中途纠正。hooks 也走同样路子——pre-hook `blocked` 短路成错误串。

### 自测 · 主动回忆

1. [L1] `tools.len() <= 9` 和 `> 9` 两种情况下，`schemas_limited_for_query` 分别怎么做？
2. [L2] 列出构成暴露视图的三路来源，以及它们各自的生命周期；哪几路是无条件保底、哪一路只竞争剩余名位？
3. [L2] `catalog_score` 的 100/50/20 分别对应什么匹配？空 query 时为什么视图仍被填满？
4. [L3] 一个工具被裁剪掉后，模型要经过几轮、走什么通道才能重新调用它？
5. [L3] 为什么 `ALWAYS_VISIBLE_TOOLS` 里有 `skill`，但实际视图里有时看不到 `skill`？seed 进去的核心名到底是几个？
6. [L2] `read_only` 标志驱动了哪两件事？它 gate 沙箱写吗？沙箱写真正由谁 gate？
7. [L4] 为什么 genome 只允许进化描述而不允许改 `parameters`/执行？这个限制对 ncx-forge 意味着什么？
8. [L4] 默认 `new()` 注册多少个工具？在什么配置下裸跑不裁剪、什么配置下裁剪立即激活？

<details>
<summary>答案要点</summary>

1. `<=9` 直接对全部工具调 `schema_for`（genome-aware）全发；`>9` 构建 `selected`：seed 已注册核心名（5–6 个）→ seed `tool_hints` → 按 `catalog_score` 填到 9（tools.rs:340-381）。
2. ① 静态核心集 `ALWAYS_VISIBLE_TOOLS`（受 `contains_key` 守卫、已注册即常驻）；② `tool_hints`（每次 `tool_search` 写、`clear()` 后只留最近一次）；③ 词法 query 匹配（每轮固定为 user prompt 文本）。核心集 ∪ hints 无条件保底；词法匹配只**竞争剩余名位**，按 score 排序填到 `limit` 为止（tools.rs:363,367-373）。
3. 名字 `eq_ignore_ascii_case` 词 +100、名字 `contains` +50、`name+desc` contains +20，按词累加。空 query 全为 0，但谓词 `score>0 || q.is_empty()` 仍填满到 9——视图=核心集+任意填充。
4. 至少经 1 轮：本轮调 `tool_search` → 写 `tool_hints` → **下一轮** `schemas_for_query` 的 seed 阶段纳入。通道只有 `tool_hints`（词法 query 不跨轮带发现）。
5. 因为每个核心名都被 `by_name.contains_key` 守卫，`skill` 仅在存在 SKILL.md 时注册；没注册就静默掉出视图。所以实际 seed 进去的核心名是 5 或 6 个、不是固定 6（tools.rs:271-273, 346-350）。
6. ① agent loop 连续只读调用合并成 `join_all` 并发批（批长还受 `remaining_tools` 预算约束，agent_loop.rs:467-469）；② `tool_search` 输出的 `(read-only)` 标注。**不** gate 沙箱写——沙箱写由 `SandboxPolicy.can_write` + approver 决定（ApplyPatchTool ~tools.rs:664/721、`ShellTool::needs_escalation` tools.rs:1148-1158），与 `read_only` 标志正交。
7. genome 只覆盖 description，`parameters()`/`to_schema()`/执行不动；沙箱仍管执行，所以 genome 注入不了新能力。对 ncx-forge：训练只进化文本，坏候选 + 空/损坏 NCX_GENOME 一律 no-op 回落默认，绝不静默改行为——这是训练的信任锚。
8. 默认 `new()` 无条件注册 **9 个 base 工具**，+1 memory（`remember`）、+1 skills（`skill`）= **9–11**。裸跑正好 9，`<=9` 不裁剪；但接了 memory 或 skills 任一就到 10，已越过阈值、裁剪立即激活；两者都有到 11（tools.rs:251-274；阈值分支 tools.rs:341-342）。

</details>

### 别发散到这

- **V4A patch 解析 / 3 级 context 匹配 / 原子 staging**（patch.rs）——属"apply_patch 工具内部"，不是暴露机制。一句带过即可。
- **`looks_read_only` shell 分类器 / 审批升级 / 沙箱 policy**（detect.rs, executor.rs）——属"审批与沙箱"子系统；只在区分"暴露的 read_only 标志 ≠ 沙箱写 gate"时点一句。
- **`read_file` 渲染细节**（`N| TEXT`、`MAX_CHARS=100000`、`DEFAULT_LIMIT=2000`）——属具体工具实现，不是动态暴露。
- **shell 执行器**（`MAX_OUTPUT=16000`、timeout 124、Windows Job 512 进程上限）——属执行层。
- **memory recall / prompt hooks 的计算时机**——属 agent loop 编排，别卷进来。

### 一句话收尾
记住一条主线即可统领全篇：**>9 才裁剪（默认 9 base 工具贴着阈值，挂 memory/skill/MCP 即触发），裁剪=核心集∪hints 保底 + 词法匹配竞争填到 9，`tool_search` 经 `tool_hints` 喂下一轮，`read_only` 管并发不管沙箱，`genome` 只改描述不改行为**——其余细节都是这条线上的挂点。


---

## 沙箱 · 审批状态机

### 一句话主线
这套系统不是一张有跳转的状态图，而是**两层正交的纯函数 + 一层进程容器**：`SandboxPolicy` 判定"物理上是否允许"（三档 sandbox 模式 + 词法路径包含），`Approver::classify` 判定"越界时怎么办"（四档 approval policy → 三态 `Decision`），人类提示（`ApprovalHandler`）作为第三层留在 `ncx-core`，而 `PolicyExecutor` 只做进程级容器（Job Object + timeout），**不碰任何审批逻辑**。

### 30 秒 / 2 分钟 / 深挖 三档

**30 秒（主干）**：审批"状态机"实际是一个纯无状态函数 `Approver::classify(command, needs_escalation) -> Decision`，由四档策略选择（untrusted / on-failure / on-request[默认] / never），输出三态 `AutoApprove | Ask | AutoDeny`。与它正交的是 `SandboxPolicy` 三档模式（read-only / workspace-write / danger-full-access），回答 `can_read`（恒 true）和 `can_write`（词法路径包含）。每次调用都重新计算，无任何跳转或持久状态。

**2 分钟（加机制名）**：谁来判？shell 由 `ShellTool::needs_escalation` 先按 policy 模式算出 escalation bit，再交给 `Approver::classify`；若结果是 `Ask`，才让 `ApprovalHandler` 弹窗（trait 在 `ncx-core/tools.rs`，不在 sandbox crate）。apply_patch 的判官是 `policy.can_write` 逐目标 + `require_edit_approval`。唯一的持久状态是 `SessionGrants`（`HashSet<String>` 精确命令 + `allow_edits` bool），session 级、`Rc<RefCell>`，new/fork/resume 即丢。`PolicyExecutor` 在决策做完之后才被调用，只提供 Windows Job Object 容器和 timeout。untrusted 档还有 `is_trusted` 白名单（29 条 TRUSTED_COMMANDS）+ 7 条 dangerous_patterns 正则 + git 写子命令覆盖（15 条 GIT_WRITE_SUBCMDS）。

**深挖（细节钩子）**：`can_write` 在 workspace-write 下把目标对 workspace 转绝对再 `normalize` 词法化（不碰 FS、不解符号链接），按 component-wise `starts_with` 判定是否在 `writable_dirs`（workspace + writable_roots + 系统 temp 仅当 `allow_temp_write`）。`is_trusted` 先跑 dangerous_patterns（任意位置命中即 false），再剥路径前缀和 `.exe` 取 first_token，base==git 时找首个非 flag token 查 GIT_WRITE_SUBCMDS。shell 决策顺序：resolve workdir（`abs.canonicalize().unwrap_or(abs)`，tools.rs:1197）→ needs_esc → `session_grants.commands.contains(command)` 精确命中强制 AutoApprove，否则 classify → AutoDeny 报 'never' 错 / Ask 走 approver / AutoApprove 直接执行 → 然后才 `PolicyExecutor::run` → on-failure 档失败后追加一次 escalated 提示并**重跑同一条命令**。apply_patch：plan_mode 拒绝 → parse → 逐目标 `canonicalize`（tools.rs:654）后过 can_write 收集 `escaping` → `needs_prompt` → ApprovalRequest 带 full patch 作 details → 最终 `can_write` 闭包是唯一强制点。`step_decision` 已写好且单测但**活流程里没被调用**（dead）。

### 核心机制 · 6 根支柱

1. **三档 sandbox 模式（policy.rs:19-21）** — `read-only` / `workspace-write` / `danger-full-access`；`can_read` 恒 true，`can_write` 在 danger 恒 true、read-only 恒 false、workspace-write 走词法 `starts_with`。— 把"物理是否允许"与"越界怎么办"彻底分开，照搬 Codex 三模式。

2. **四档 approval policy → 三态 Decision（approval.rs:161-188）** — never（escalation? AutoDeny : AutoApprove）/ on-request 默认（escalation? Ask : AutoApprove）/ on-failure（永远 AutoApprove，失败后才问）/ untrusted（is_trusted && !escalation ? AutoApprove : Ask）；未知策略串 → Ask。— 纯函数每次重算，把 yes/no 提示完全踢出 sandbox crate。

3. **untrusted 三重过滤（approval.rs:119-142）** — dangerous_patterns 先跑（任意位置命中即否，7 条正则）→ 剥路径/`.exe` 取 base token 查 29 条白名单 → base==git 再查 15 条写子命令。— 保守白名单：只让已知安全命令自动跑（`git status` 自动、`git push` 仍问），危险模式前置兜底。

4. **ApprovalHandler trait + ApprovalDecision + SessionGrants（tools.rs:44-78）** — `#[async_trait(?Send)]` 的提示抽象住在 ncx-core（避开与纯 `Approver` 重名），返回 `Deny|Once|Always`；"记住决定" = `SessionGrants{commands: HashSet<String>, allow_edits: bool}`，`Rc<RefCell>` 挂在 ToolContext。— 提示是 CLI/GUI 上层关切故用 trait；always-grant 仅 session 级、命令精确匹配，刻意窄。

5. **shell 执行路径与判官顺序（tools.rs:1184-1294）** — needs_escalation 先算（danger→false / read-only-class→`!looks_read_only` / workspace-write→`!can_write(workdir)`，workdir 已 canonicalize），session_grants 精确命中可短路 AutoApprove，再 classify，Ask 才弹窗，最后才 `PolicyExecutor::run`。— escalation 是只有 tool 知道的 policy 模式启发式，算完后把 auto/ask/deny 交给纯 Approver。

6. **PolicyExecutor 只做容器（executor.rs:83-195）** — 最小化 env + cmd.exe /C（或 bash -l -c）+ Windows Job Object（KILL_ON_JOB_CLOSE，+ ACTIVE_PROCESS_LIMIT）+ timeout；**executor.rs 从不 import / 引用 `ncx_sandbox`，不碰任何 approval 类型**（注：ncx-tools crate 的 Cargo.toml 仍把 ncx-sandbox 列为依赖，故 crate 链接了它，但执行器源码不触审批逻辑）。— 设计明确分工：执行器只管进程/资源容器，FS/网络隔离留在 policy+approval 层；决策到此已成定局。

### 关键数字 / 必背细节
- **4** approval policies：untrusted / on-failure / **on-request（默认）** / never（approval.rs:18-21）
- **3** Decision 态：AutoApprove / Ask / AutoDeny（approval.rs:24-32）
- **3** sandbox 模式：read-only / workspace-write / danger-full-access（policy.rs:19-21）
- **3** ApprovalDecision：Deny / Once / Always；`approved() == !Deny`（tools.rs:44-56）
- TRUSTED_COMMANDS = **29** 条；GIT_WRITE_SUBCMDS = **15** 条；dangerous_patterns = **7** 条正则；WRITE_TOOLS = **2**（shell, apply_patch）
- 默认 `approval_policy = on-request`；默认 `timeout_s = 120`，shell timeout 参数限 **1..600**（tools.rs:131, 1178）
- `PolicyExecutor.active_process_limit` 默认 **512**，0 关闭上限（executor.rs:84-94）
- timeout → `exit_code = 124, timed_out = true`（executor.rs:189-191）；spawn 失败 `exit_code = 1`
- `MAX_OUTPUT = 16000` 字符，head/tail 各 **8000**（executor.rs:20, 68-74）
- `CREATE_NO_WINDOW = 0x0800_0000`（executor.rs:135）；TerminateJobObject exit 1（executor.rs:326）
- `allow_temp_write` 默认 false；`network_access` 默认 false，policy 自己绝不强开（policy.rs:30-31, 60-63）
- 关键函数名：`Approver::classify` / `SandboxPolicy::can_write` / `is_trusted` / `looks_read_only`（detect.rs:53-73）/ `split_chain`（detect.rs:39-47，按 `&&` `||` `;` `|` `&` 换行六分隔符切段）/ `step_decision` / `normalize`（policy.rs:134-146）

### 取舍与坑
- **`step_decision`（approval.rs:54-62）是死代码**：已定义 + 单测（248-268），但 shell 和 apply_patch 活流程都没调它。逐步确认实际靠 `ApprovalDecision::Always` + `require_edit_approval` 实现。它是"per-step 覆盖层"的可复用原语，目前未接线。
- **on-failure 重跑的是同一条命令**（tools.rs:1288）：没有第二个"非沙箱"执行模式——PolicyExecutor 根本没有 FS/网络沙箱可放松，重跑只是用户 bless 了一次。
- **`sandbox_denied` / `denial_reason` 是 vestigial 字段**：ExecResult 带着、render() 也处理，但 executor.rs 从不 set true；真正的拒绝在上层以纯错误字符串抛出（tools.rs:1227-1232, 705-707），是为对齐 Python ExecResult 形状。
- **shell always-allow 是精确字符串匹配**（tools.rs:1219，HashSet）：`ls -la` 被授予不会 bless `ls -l`；尾随空格或参数重排即失效。
- **apply_patch 'Always' 仅在 escaping 为空时设 `allow_edits`**（tools.rs:710）：越界 patch 永远不能被一次性 blanket 授权，每次重问。
- **shell 的 needs_escalation 只查 `can_write(WORKDIR)`**（tools.rs:1157），不看命令实际写哪些文件；workdir 在 workspace 内但写到别处的命令不会被这条启发式标记（read-only 模式改靠 `looks_read_only` token 扫描）。
- **"无 approver" 两条路失败模式不同**：shell + Ask 硬报错（tools.rs:1259-1262）；apply_patch + needs_prompt 静默落到 `can_write` 闭包去拒绝越界目标（tools.rs:715-718）。
- **两条写路径的真正差异是粒度/检查对象，不是符号链接解析**：shell 在调 `can_write` 前先 canonicalize workdir（tools.rs:1197），apply_patch 在 can_write 前先 canonicalize 每个目标（tools.rs:654）——**两侧都先 canonicalize 自己的输入再做词法 can_write，符号链接在两条路上都已被解析**。真正的不对称在于：shell 只查 `can_write(WORKDIR)`（工作目录是否可写），不看命令实际写到哪；apply_patch 逐个声明的写目标查 can_write。
- **danger-full-access 只清零 escalation bit，不改 approval policy**：untrusted + danger 下，未知命令仍会 Ask，因为 classify 的 is_trusted 独立于 escalation（approval.rs:179-185）。
- **未知/拼错的 approval policy 串 → Ask**（approval.rs:186），fail-safe；空/默认 ToolContext 用 on-request。

### 高频追问与应答
- **Q：这"状态机"到底有几个状态、怎么跳转？**
  A：严格说没有跳转。它是纯函数 `Approver::classify`，每次调用从 (policy, command, needs_escalation) 重新算出三态之一，不存任何中间态。全子系统唯一持久状态是 `SessionGrants`，且它在 ncx-core 不在 sandbox crate，session 级生命周期。

- **Q：danger-full-access 是不是就完全放行了？**
  A：不是。danger 只让 `can_write` 恒 true、`needs_escalation` 恒 false（清零 escalation bit），但**不改 approval policy**。若 policy=untrusted，未知命令照样 Ask，因为 classify 里 untrusted 分支检查 is_trusted 与 escalation 无关。

- **Q：untrusted 下 `rm -rf /tmp/x` 会怎样？为什么 `rm` 本身不在白名单也被拦？**
  A：`is_trusted` 第一步就跑 dangerous_patterns（`rm -[rf]` 等 7 条正则），任意位置命中直接返回 false，**先于** first_token 解析和白名单查询。所以即使前导是个 trusted token 也会被拦，classify 返回 Ask（不是 AutoApprove）。

- **Q：apply_patch 写到 workspace 外怎么处理？能"总是允许"吗？**
  A：每个目标（含 move_to）resolve 后过 `policy.can_write`，失败的进 `escaping`，触发 escalated 提示（reason 列出越界路径，details 是完整 patch）。即使用户选 Always，只要 escaping 非空就**不会**设 `allow_edits`——越界写每次都重新批。最终 `can_write` 闭包是唯一强制点，连没有 approver 时越界目标也直接失败。

- **Q：on-failure 重跑时是不是放松了沙箱？**
  A：没有。tools.rs:1288 重跑的是**同一条命令**走同一个 PolicyExecutor，没有第二个非沙箱模式。重跑唯一区别是用户 bless 过一次。这也暴露真正的 FS/网络沙箱并未实现，PolicyExecutor 只做进程容器。

- **Q：PolicyExecutor 怎么参与审批决策？**
  A：完全不参与。executor.rs 从不 import / 引用 `ncx_sandbox`、不碰任何 approval 类型（虽然 ncx-tools 的 Cargo.toml 把 ncx-sandbox 列为依赖，crate 链接了它）；`.run()` 被调用时 auto/ask/deny 已成定局。它只负责最小化 env、cmd.exe /C / bash -l -c 启动、Windows Job Object（KILL_ON_JOB_CLOSE）杀整个后代进程树、以及 timeout（超时 exit 124）。Job API 失败时降级为无容器运行而非报错（对齐 Python OSError fallback）。

### 自测 · 主动回忆
1. [L1] 四档 approval policy 分别是什么？默认哪个？
2. [L2] read-only 模式下 shell 命令是用 `can_write` 判 escalation 的吗？不是的话用什么？`split_chain` 按哪些分隔符切段？
3. [L2] `is_trusted` 的三个检查步骤及顺序是什么？为什么 dangerous_patterns 必须放第一步？
4. [L3] shell 写路径和 apply_patch 写路径在符号链接处理上真的不同吗？两者真正的不对称在哪？
5. [L3] "无 approver 配置" 时，shell 和 apply_patch 的失败方式有何不同？
6. [L4] 为什么 `ApprovalHandler` trait 住在 ncx-core 而不是 sandbox crate？这体现了什么分层设计？
7. [L4] `step_decision` 的设计意图是什么？它现在为什么是死代码？
8. [L3] danger-full-access + untrusted policy 组合下，未知命令会 AutoApprove 吗？为什么？

**答案要点**
1. untrusted / on-failure / on-request（默认）/ never；常量在 approval.rs:18-21。
2. 不是。read-only-class（`!writes_allowed`）走 `!looks_read_only(command)`（detect.rs:53-73）；只有 workspace-write 才用 `!can_write(workdir)`。`split_chain`（detect.rs:39-47）按 `&&`、`||`、`;`、`|`、`&`、换行**六个分隔符**切段，逐段比对 READ_ONLY_PREFIXES（且整串先过 WRITE_OR_SUBSHELL token 否决）。
3. ① dangerous_patterns 正则（任意位置命中即 false）② 剥路径前缀 + `.exe`、小写化取 first_token 查 29 条白名单 ③ base==git 时查 15 条写子命令。先跑正则是 fail-safe，保证 `rm -rf` 这类即使被 trusted token 引导或 rm 不在白名单也被拦。
4. 在符号链接处理上**两者其实一致**：shell 在 `can_write` 前先 canonicalize workdir（tools.rs:1197），apply_patch 在 can_write 前先 canonicalize 每个目标（tools.rs:654），符号链接两侧都已解析；`can_write` 本身两边都是纯词法（policy.rs:11-15, 134-146）。真正的不对称是检查对象/粒度：shell 只查 `can_write(WORKDIR)`（工作目录是否可写），不看命令实际写到哪；apply_patch 逐个声明的写目标查 can_write。
5. shell + Decision::Ask 且无 approver → 硬报错 'no approver configured'（tools.rs:1259-1262）；apply_patch + needs_prompt 且无 approver → 静默落到最终 `can_write` 闭包，越界目标被拒、in-workspace 目标可能放行（tools.rs:715-718）。
6. 因为提示是 CLI/GUI 上层关切，把 yes/no round-trip 留在 ncx-core 让 sandbox crate 保持 prompt-free（纯决策）；trait 命名也为避开与纯 `ncx_sandbox::Approver` 重名。体现 policy（物理允许）/ approval（越界怎么办）/ prompt（人类交互）三层解耦。
7. 意图：让用户关掉 auto-approve、对每个状态修改动作强制确认，叠加在沙箱-escalation 决策之上，且**永不软化 AutoDeny**。现为死代码：活流程改用 `ApprovalDecision::Always` + `require_edit_approval` 实现 per-step，故 step_decision 虽有单测但未接线。
8. 不会。danger 只清零 escalation bit，untrusted 分支判 `is_trusted(command) && !needs_escalation`；未知命令 is_trusted=false，结果仍 Ask。danger 不改 approval policy。

### 别发散到这
- **实际 patch 应用 / hunk 匹配算法**（patch.rs 的 staging、re-check）——属于 apply_patch 工具内部，只需知道它逐路径复查 `can_write` 即可。
- **Tauri GUI 的 modal 实现 / bridge.rs 的 GuiApprover**——属于 GUI 层，只需点名"Always/Once/Deny 的提示在这弹"。
- **detect.rs 内 `looks_read_only` 的完整 token 表**——属于命令检测子系统，只需知道它是与 `is_trusted` 独立的第二套"命令是否安全"启发式。
- **agent loop / orchestrator / tool dispatch**——上游怎么决定发出 tool call，不在本子系统。
- **Job Object 的 Win32 API 细节 / POSIX 进程组**——属于 executor 平台实现，知道"KILL_ON_JOB_CLOSE 杀后代树、失败降级"即可。

### 一句话收尾
记住一句话：**两层正交纯函数（policy 判物理、approver 判越界）+ 一层 session-scoped 的 SessionGrants + 一个不碰审批只做容器的 PolicyExecutor**——能把这三块的边界讲清楚，深挖时再用 classify 的四档分支、is_trusted 三步（29 条白名单 / 15 条 git 写子命令）、can_write 词法包含这些细节往下堆，就既不浅也不散。


---

## 主子 agent 如何通讯

### 一句话主线 (the thesis that prevents a shallow answer)

**父 orchestrator 与子节点之间没有消息总线、没有 peer-to-peer 对话——通讯靠三层「单向广播 + 文件落地 + 裁决回灌」的闭环：下行靠 prompt 文本把状态按值序列化进每个全新无状态子会话，代码改动的真实 IPC 是文件系统(每个 worker 改自己的隔离拷贝，verifier 选出 winner 后 `promote_worker` 把它 copy 回真 workspace)，回路靠 verifier 的 `PASS`/`FAIL`/`BEST:<n>` 裁决文本驱动重试与落地。** 「派任务」只是下行通道的一小段。

### 30 秒版 / 2 分钟版 / 深挖版

**30 秒版**
父节点(Orchestrator)通过 `AgentRunner` trait 给每个子节点(classify/plan/decompose/worker/verify)开一个全新的、无状态的一次性 `AgentLoop` 会话。它们之间不直接对话——所有信息只能(A)由父节点用 `build_worker_task`/`build_verify_task`/`build_decompose_task` 拼成 prompt 文本下发，(B)代码改动落到各自隔离的文件系统拷贝里再由 verifier 选 winner 提升回真 workspace。

**2 分钟版**
在 30 秒版基础上补三件事:
- **下行通道是 prompt(状态按值传递)**:没有长会话,子节点需要的一切每次都重新序列化进它的 prompt——`build_worker_task` 拼入「原始 task + 一次性算好的 plan + `(You are worker i of n)` 身份行 + 上一轮 verifier 反馈」(orchestrator.rs:433-445)。plan 只在 pipeline/decompose 里算一次(orchestrator.rs:214,283),分发给所有 worker 和 verifier 当唯一真相源。
- **代码改动的真实通道是文件系统**:live runner 里每个 worker 跑在自己的递归拷贝上(`run_worker`+`copy_tree`,runner.rs:131-151),verifier 用 `BEST:<n>` 点名 winner 后,`promote_worker` 把那一个 scratch 目录 copy 回真 workspace 并删掉所有 scratch(runner.rs:153-162)。隔离-然后-提升本身就是 IPC。
- **回路是裁决文本**:`PASS`/`FAIL`(fail-loud)驱动闭环重试(verdict 原文被当作 `feedback` 灌回下一轮 prompt),`BEST:<n>` 决定提升哪个 workspace。

**深挖版**
再加四层结构性细节:
- **唯一的父↔子边界是 `AgentRunner` trait**(orchestrator.rs:53-78):四个 async 方法——`run`(带工具)、`reason`(默认转 `run`,live 里被覆盖为剥工具)、`run_worker`(隔离,固定 `Tier::Fast`)、`promote_worker`(默认 no-op)。Orchestrator 只持有 `&dyn AgentRunner` + config(orchestrator.rs:147-155),**没有 channel、没有共享可变 agent 状态**。子节点全是全新无状态会话:每次调用都新建 provider+ToolRegistry+Session+AgentLoop(runner.rs:75-106)。
- **reason() 剥工具是代码级强制而非 prompt 劝导**:live `reason()` 走 `ToolRegistry::empty`(runner.rs:95-99,124-129),而 `run`/`run_worker` 走 `ToolRegistry::new`。classify/plan/decompose/verify 拿不到任何工具对象,一个能力很强的 Main 模型在「只该判断/规划」时**物理上无法**去碰文件。
- **copy_tree 是叠加拷贝,不是 git、不是 tempdir-swap**:迭代式 DFS + `std::fs::copy`,跳过 SKIP_DIRS(.git/target/node_modules/.ncx/dist/.venv/__pycache__,isolate.rs:15-53)。提升是**覆盖**匹配文件,但**不删**真 workspace 里 scratch 没有的文件——worker 能增改、不能真正通过提升删文件。
- **递归子任务严格串行,每个先提升再进下一个**:`decompose_and_recurse` 用普通 `for` 循环(orchestrator.rs:321-330),每个子任务的 winner 在循环推进前已 `promote_worker` 落地——所以子任务 k+1 的 worker copy_tree 时看到的是已含子任务 k 改动的 workspace,依赖型子任务得以叠加。

### 通讯的三/四条真实通道

**通道一:Prompt 线程化的文本工件(下行)**
- **机制**:父节点把状态按值序列化进每个子会话的 prompt。`build_worker_task` = `"Task:\n{task}\n\nPlan:\n{plan}\n\n(You are worker {i+1} of {n}.)"`,feedback 非空时再追加 `"\n\nThe previous attempt was rejected. Address this feedback:\n{feedback}"`(orchestrator.rs:433-445)。`build_decompose_task` = `"Task:\n{task}\n\nPlan:\n{plan}"`(orchestrator.rs:447-449)。`build_verify_task` 用 `"--- worker {i+1} ---\n{r}"` 把各 worker 结果拼接(orchestrator.rs:451-459)。
- **为什么**:没有长会话,子节点需要的一切每次都得重新塞进 prompt。plan 只算一次后分发给所有 worker + verifier,共享一个真相源。

**通道二:文件系统状态(代码改动的真实 IPC)**
- **机制**:`run_worker` 给每个 worker idx 分配唯一 scratch 目录 `temp_dir()/ncx_worker_{pid}_{n}`(runner.rs:110-114),记进 `scratch: HashMap<idx,PathBuf>`,删掉该 idx 上一轮拷贝,`copy_tree` 真 workspace → scratch,带工具的 agent 在 scratch 路径下运行(copy 失败则回退真 workspace)。verify 后 `promote_worker(best)` 把 winner 的 scratch `copy_tree` 回真 workspace,再 `remove_dir_all` 掉本轮所有 scratch(runner.rs:153-162),每轮只调用一次(orchestrator.rs:259-260)。
- **为什么**:`run_worker`(runner.rs:131-151)给**每个** idx(含 worker 0)都分配私有 scratch,无 idx==0 特例。注意源码自己的 doc comment(runner.rs:6-9、isolate.rs:4-8)仍写「worker 0 跑在真 workspace,1..N 跑拷贝」——那是**过时注释**,live `run_worker` 已统一隔离所有 worker;若面试官引用该注释,直接指出注释相对实现已 stale。隔离的动机是观察到的真实 bug:并行 worker 抢着 `apply_patch` Add 同一文件会撞车(isolate.rs:4-8)。给每个 worker 私有树让并行写入物理上互不相交;winner 的改动**恰好一次**到达真 workspace,落败的探索拷贝被丢弃,这次提升正是下一个串行子任务能叠加的原因。

**通道三:裁决回路(`PASS`/`FAIL`/`BEST:<n>`/`SUBTASK:`,健壮解析+兜底)**
- **机制**:
  - `PASS`/`FAIL`——`verdict_passed` = `!verdict.to_uppercase().contains("FAIL")`(orchestrator.rs:379-381),是对**整个**大写化 verdict 文本做子串扫描,位置无关:只要全文任意处不含 `FAIL` 子串就算 PASS(**fail-loud**)。VERIFY_SYS 要求模型「以 PASS/FAIL 开头」只是 prompt 约定,**解析器并不检查首 token、也不强制顺序**;verdict 原文被存为 `feedback` 灌回下一轮。
  - `BEST:<n>`——verdict 末行,1-based。`parse_best_worker` 找 `BEST:`、读后续 ascii 数字、parse 成 usize、减 1 转 0-based(saturating)、clamp 到 0..n;**缺失/无法解析→index 0**(orchestrator.rs:486-502;消费它落地的 `promote_worker` 在 runner.rs:153-162)。它直接决定 `promote_worker` 提升哪个 scratch。
  - `SUBTASK:`——DECOMPOSE 节点发出,`parse_subtasks` 大小写不敏感找 `SUBTASK:` 取其后文本(orchestrator.rs:387-410);零条时 fallback 到 `strip_list_marker` 解析编号/项目符号行(orchestrator.rs:414-431),显式 `SUBTASK:` 优先并压制 fallback。
  - `simple`/`medium`/`high`——CLASSIFY 单词输出,`parse_complexity` 子串优先级 high>simple>medium,**无法识别→Medium**(orchestrator.rs:364-376)。
- **为什么**:无状态重跑唯一能从失败中学习的方式就是把 reviewer 的具体抱怨灌进下一次 prompt;闭环有上限(`max_verify_retries`)防止不可满足的任务死循环。所有解析都有兜底,系统在 verifier 输出畸形时**偏向「接受并提升 worker 1」往前推进**而非卡死。

**通道四:tool-stripped 的 reason() 节点(隔离推理与执行)**
- **机制**:reasoning 节点(classify/plan/decompose/verify)走 `reason()` → `run_in(..., with_tools=false)` → `ToolRegistry::empty`(runner.rs:95-99,124-129)。其中 4 个里有 3 个的 system prompt 显式写明 'You have NO tools':`CLASSIFY_SYS`/`PLAN_SYS`/`DECOMPOSE_SYS`(orchestrator.rs:81-92);`VERIFY_SYS`(orchestrator.rs:96-99)**没有**这句话,只说「strict reviewer…以 PASS/FAIL 开头」。真正让全部四个节点拿不到工具的是**代码级**保证——`reason()` 一律走 `ToolRegistry::empty`,prompt 文字只是叠加的劝导。只有 worker 走 `run()`/`run_worker()` 拿全量 `ToolRegistry`。
- **为什么**:判断/规划的节点绝不能动手。剥掉 registry 让「分类时顺手改文件」从物理上不可能,而非仅靠 prompt 劝阻——所以 verify 即便 prompt 没写「NO tools」也照样无工具。

### 如果面试官追问 Python 层

**诚实结论:Python 多 agent 层不是 agent-to-agent 通讯层,而是一个带共享黑板的「父→子任务流水线」(extract.python.is_agent_comm = false)。** 必须明确区分,不要套用 Rust 的心智模型:

- **没有任何子 agent 之间相互寻址/路由/发消息的通道。** 仓库里唯一的 @-mention 代码(mentions.py)是 Codex 风格的 `@path` **文件内联**功能(把文件内容拉进用户 prompt,mentions.py:37),与「寻址 agent」毫无关系——没有 agent 注册表、没有收件人解析、没有消息投递。`@channel` 这类 token 被故意原样保留。
- **真正的跨节点通道是共享黑板 `AgentState`**(state.py:213,挂在 `ctx.agent_state`,base.py:30)。worker 只能通过三个工具写它:`record_fact`(追加 `Fact(text, source_node=node_id)`)、`write_checkpoint`(原子落盘→append→save)、`request_verification`(置 `node.status='verify'` + 写 `outputs['verification_request']`)。父节点再把选定状态**重新广播**进下一个 worker 的 brief(facts 经 node_brief 注入,orchestrator.py:216;corrective_actions 经 node.inputs,orchestrator.py:281)。
- **与 Rust 的根本差异**:Rust 是「prompt 线程化 + workspace 提升」,交换单位是文本轮次与文件树;Python 是「共享可变对象黑板」,交换单位是结构化状态记录(Fact/AgentCheckpoint/VerifyResult)。**Python 父节点从不把 worker 的 chat 输出当上下文摄入**——它只检查 `node.status`/`outputs`/`checkpoints`,再为下一个 worker 合成全新 brief。别把 Python 层描述成「把子 agent 的回复线程进父 context」(那是 Rust 心智模型)。
- **roles = 能力标签的工具隔离,不是会话人格**(roles.py:49-130):`build_role_registry` 只给 worker 与 role `allow_tags` 相交的工具,read-only role 还额外拿 READ_ONLY 沙箱 ctx——planner 干脆没有 `ApplyPatchTool` 对象可调。roles 定义的是**权限**,不是消息端点。
- **task_graph 是依赖排序,不是数据流通道**:`depends_on` 只决定节点何时可运行(`ready_nodes`,task_graph.py:117),**graph 层不会自动把 A.outputs 接进 B.inputs**;跨节点数据传递全靠父节点显式 copy 进 brief。
- **anti-fake-done 门**:worker 没有任何工具能把自己节点设成 done——`request_verification` 只能翻到 'verify',只有父节点的 verifier 通过才置 'done'(orchestrator.py:250)。worker 直接停下会被强制 fail(orchestrator.py:228)。

**一句话**:Python 层是 planner/worker/verifier 流水线,通过一个可持久化的黑板协调,所有路由由父节点用纯代码决定,而非 agent 之间互发消息。

### 并发安全 & 为什么这样设计

- **并行只限单轮内的 best-of-N worker fanout**:`join_all(worker_futs)` 并发跑 n 个 worker(orchestrator.rs:240-244)。它们撞不了共享文件状态,因为 `run_worker` 给每个 idx 一份私有递归拷贝(唯一 temp 路径 `ncx_worker_{pid}_{counter}`),工具写入(apply_patch、shell)落在互不相交的树里。
- **唯一并行触碰的共享可变状态**是 `scratch: RefCell<HashMap>` 和 `counter: Cell`,但它们同步改、借用显式 scoped 到 `.await` **之前**释放,且整个 runtime 是 `?Send`/单线程 async——HashMap 插入和 copy_tree 在任何 worker future 让出前就已完成。
- **其余全串行**:verify→promote 在所有 worker join 后只跑一次;`promote_worker` 只 copy 一个 winner 回去并删所有 scratch;**递归子任务在 `for` 循环里串行,每个先提升再进下一个**(orchestrator.rs:321-330)。真 workspace 在任一时刻**恰好一个写者**(提升,或 copy 失败回退时的单个 worker)。
- **caps 兜底**:递归深度浅(默认 max_depth=1,depth==1 的 high 任务不再分解、改跑 Main 上的 best-of-N,orchestrator.rs:187-193);`<2` 子任务回退到 Main 上单次 best-of-N(orchestrator.rs:294-306);`max_subtasks` 限制过度切分。分解树永远收敛到带工具的 best-of-N,**绝不无限递归**(boxed LocalBoxFuture)。
- **为什么这样设计**:并行化子任务会重新引入隔离本来要消除的撞车;串行提升让依赖型子任务可组合。这是递归不能并行化的设计原因。

### 高频追问与应答

**Q:子 agent 之间能直接对话吗?**
不能。没有消息总线、没有 peer channel。子节点是全新无状态会话,彼此看不到对方的对话历史。它们只能通过父节点拼的 prompt(下行)和文件系统提升(代码改动)间接交换,父节点是唯一的中枢。

**Q:winner 怎么选并落地?**
verifier 在 `build_verify_task` 里看到 `--- worker i ---` 分隔的所有结果,输出末行 `BEST:<n>`(1-based)。`parse_best_worker` 转成 0-based clamp 后,`promote_worker` 把那一个 scratch 目录 `copy_tree` 回真 workspace,删掉本轮所有 scratch。**它不合并多个 worker 输出——只挑一个,这个挑选直接决定哪份文件树变成现实。** 缺失/畸形的 `BEST:` 兜底为 index 0(worker 1)。

**Q:并行 worker 会不会互相覆盖文件?**
不会。每个 worker(含 worker 0)都跑在自己 `ncx_worker_{pid}_{n}` 的私有递归拷贝里,写入物理上互不相交;`run_worker`(runner.rs:131-151)对所有 idx 一视同仁,无 worker 0 特例。真 workspace 只在 verify 之后被 `promote_worker` 写一次。这正是为修复「并行 `apply_patch` Add 同名文件撞车」(isolate.rs:4-8)而做的隔离。(若被引用 runner.rs:6-9 / isolate.rs:4-8 的「worker 0 在真 ws」注释,指出该注释相对 live 代码已过时。)

**Q:为什么 classify/plan/decompose/verify 不给工具?**
因为这些节点该判断/规划而非执行。live `reason()` 给它们 `ToolRegistry::empty`(runner.rs:95-99),能力强的模型在分类时**物理上拿不到** `ApplyPatchTool` 去改文件——是代码级强制,不是 prompt 劝阻。(顺带:classify/plan/decompose 的 prompt 还显式写了 'You have NO tools',verify 的 prompt 没写,但靠空 registry 同样无工具。)

**Q:验证没过会怎样?会死循环吗?**
进闭环重试:fan out→verify→不过则把 verdict 原文当 `feedback` 灌回下一轮 worker prompt,直到 PASS 或 `rounds>max_verify_retries`。注意一个坑:**重试用尽后的 FAIL 退出也会 promote best**(unverified),final_text 被打上 `[unverified after retries — reviewer said: ...]` 标签(orchestrator.rs:256-261)。系统偏向「往前推进」而非卡死。

**Q:promote 是 git 操作还是快照交换?能删文件吗?**
都不是。`copy_tree` 是纯递归 `fs::copy` 叠加拷贝,跳过 .git 等 SKIP_DIRS,只复制不删除目标。所以 winner 能增改、但**不能通过提升真正删掉真 workspace 的文件**;落败拷贝直接 `remove_dir_all`。.git 被跳过意味着提升碰不到历史。

**Q:为什么递归子任务不并行?**
因为子任务可能相互依赖。串行 `for` 循环保证子任务 k 的 winner 在 k+1 的 worker `copy_tree` 之前已提升进真 workspace,k+1 才能基于已提交的工作叠加。并行化会重新引入隔离本要消除的撞车。

### 自测 · 主动回忆

1. **[L1]** 这份 sheet 的「一句话主线」反复强调主子 agent 之间「没有」什么？它用哪三层闭环来替代它？

2. **[L2]** 为什么下行通道选择把全部状态「按值序列化进每个子会话的 prompt」，而不是维持一条长会话？这种设计的代价是什么？（结合 `build_worker_task` 每次都要重塞 task+plan+身份行+feedback 来说明）

3. **[L2]** sheet 说「文件系统才是代码改动的真实 IPC」。为什么文件隔离-然后-提升本身就构成一种 IPC？相比让所有 worker 直接写真 workspace，它解决了哪个被观察到的真实 bug？

4. **[L3]** 假如 verifier 输出了一段畸形 verdict——既没有清晰的 `PASS`/`FAIL` 首 token，`BEST:` 行也缺失或无法解析——系统会怎么走？请分别说明 `verdict_passed` 和 `parse_best_worker` 的兜底行为，以及由此体现的总体设计倾向。

5. **[L3]** `VERIFY_SYS` 的 prompt 里并没有写 'You have NO tools'（不像 classify/plan/decompose），那 verify 节点到底是靠什么拿不到任何工具的？如果有人把「verify 能用工具」当成漏洞来质疑，你怎么用代码级事实反驳？

6. **[L2/L3]** 为什么 best-of-N worker fanout 可以安全地用 `join_all` 并行，而递归子任务却必须用普通 `for` 循环串行？把这两个并发决策背后的同一条原则讲清楚。

7. **[L3]** 源码里 runner.rs:6-9 / isolate.rs:4-8 的 doc comment 仍写「worker 0 跑在真 workspace，1..N 跑拷贝」。这与 live `run_worker` 的实际行为有什么冲突？面试官若引用该注释，正确的回应是什么？

8. **[L4]** 有人想把 Rust 的「prompt 线程化 + workspace 提升」心智模型直接套到 Python 多 agent 层上，说「Python 也是子 agent 互相发消息」。请论证为什么这是错的：Python 层的真实协调机制是什么，交换单位是什么，以及为什么它不算 agent-to-agent 通讯。

**答案要点**

1. 没有消息总线、没有 peer-to-peer 对话。三层闭环：下行 = prompt 文本把状态按值序列化进全新无状态子会话；代码改动的真实 IPC = 文件系统（worker 改隔离拷贝，`promote_worker` 把 winner `copy_tree` 回真 workspace）；回路 = verifier 的 `PASS`/`FAIL`/`BEST:<n>` 裁决文本驱动重试与落地。「派任务」只是下行通道的一小段。

2. 因为子节点是全新无状态会话（每次调用新建 provider+ToolRegistry+Session+AgentLoop），没有长会话可继承，所需一切只能每轮重新塞进 prompt；`build_worker_task` 拼入「原始 task + 一次性算好的 plan + `(You are worker i of n)` 身份行 + 上一轮 feedback」。plan 只在 pipeline/decompose 里算一次后分发给所有 worker+verifier 当唯一真相源。代价：每轮重复序列化、token 开销，无法靠会话记忆增量。

3. worker 各跑自己的私有递归拷贝（`run_worker`+`copy_tree`，唯一 temp 路径 `ncx_worker_{pid}_{n}`），winner 经 `promote_worker` 恰好一次到达真 workspace——「隔离 → 选 winner → 提升」就是状态在进程/会话间传递的通道，故是 IPC。它修复的真实 bug：并行 worker 抢着 `apply_patch` Add 同名文件会撞车（isolate.rs:4-8）；私有树让并行写入物理互不相交。

4. `verdict_passed` = `!verdict.to_uppercase().contains("FAIL")`，是对整段大写化文本做位置无关子串扫描（fail-loud）；不检查首 token 也不强制顺序，所以畸形但不含 FAIL 子串 → 当作 PASS。`parse_best_worker` 找不到 / 无法解析 `BEST:` → 兜底为 index 0（即 worker 1），再 clamp 到 0..n。总体倾向：解析全有兜底，verifier 输出畸形时系统偏向「接受并提升 worker 1 往前推进」而非卡死。

5. 靠代码级强制——live `reason()` 一律走 `ToolRegistry::empty`（runner.rs:95-99，124-129），而 `run`/`run_worker` 才走 `ToolRegistry::new`。classify/plan/decompose/verify 都经 `reason()`，物理上拿不到任何工具对象。反驳：verify 的 prompt 没写 'You have NO tools' 只是少了一句叠加劝导，但空 registry 让它即便想调 `ApplyPatchTool` 也无对象可调，不存在「能用工具」的漏洞。

6. 同一条原则：真 workspace 在任一时刻只能有「恰好一个写者」。best-of-N 能并行是因为每个 idx 有私有递归拷贝（`run_worker`），工具写入落在互不相交的树里，撞不了共享文件状态（仅 `scratch: RefCell<HashMap>`/`counter: Cell` 在 `.await` 前同步改完，且 runtime 是 `?Send` 单线程）。递归子任务必须串行，因为子任务可能相互依赖：`for` 循环保证子任务 k 的 winner 先 `promote_worker` 落地，k+1 的 worker `copy_tree` 才能看到 k 的改动叠加；并行化会重新引入隔离本要消除的撞车。

7. 冲突：注释说 worker 0 在真 workspace、1..N 跑拷贝，但 live `run_worker`（runner.rs:131-151）给每个 idx（含 worker 0）都分配私有 scratch，无 idx==0 特例。该注释相对实现已 stale（过时）。正确回应：直接指出注释落后于代码，并以「真 workspace 只在 verify 后被 `promote_worker` 写一次」说明所有 worker 一视同仁地隔离。

8. 错在套用 Rust 心智模型。Python 层是「带共享黑板的父→子任务流水线」（is_agent_comm = false），没有任何子 agent 互相寻址/路由/发消息的通道——唯一的 @-mention 代码（mentions.py）是 Codex 风格 `@path` 文件内联，与寻址 agent 无关。真实协调机制是共享可变黑板 `AgentState`（worker 只能用 `record_fact`/`write_checkpoint`/`request_verification` 写它），父节点再把选定状态重新广播进下一个 worker 的 brief；交换单位是结构化状态记录（Fact/AgentCheckpoint/VerifyResult），不是 chat 轮次。父节点从不把 worker 的 chat 输出当上下文摄入，只查 `node.status`/`outputs`/`checkpoints`；roles 是能力标签的工具隔离、task_graph 的 `depends_on` 只做依赖排序不自动接数据——路由全由父节点用纯代码决定，故不算 agent-to-agent 通讯。

### 一句话收尾

**子 agent 不「对话」——父节点用 prompt 下发状态、用隔离 workspace 的 `promote_worker` 落地唯一 winner、用 `PASS`/`FAIL`/`BEST:<n>` 回灌闭环:文本是下行通道,磁盘才是代码的真正 IPC,裁决文本是回路。**


---

## 项目记忆 · 自进化

### 一句话主线
"项目记忆·自进化" 是两个正交的层：MEMORY（`memory.rs`，机器写、可演化、按 query 每轮注入的经验库）和 INSTRUCTIONS（`project_instructions.rs`，人写、静态、启动整块注入的 CLAUDE.md/AGENTS.md）；"自进化" 只指 MEMORY 一侧的 `remember` 写入 + `consolidate`/`summarize_consolidate` 去重折叠，INSTRUCTIONS 永远是不可变的人类输入。

### 30 秒 / 2 分钟 / 深挖 三档

**30 秒（主线 + 双层 + 一句存储真相）**
MEMORY 是 agent 自己写、可自演化的经验库，落地为单个人类可读 markdown 文件 `.ncx/memory/LEARNINGS.md`（不是 JSONL），每条是 `<!-- ts:<epoch> tags:<a,b> -->` 注释头 + 正文；INSTRUCTIONS 是人写只读的 CLAUDE.md/AGENTS.md，启动一次整块进系统提示。两者区别一句话：机器写 vs 人写、每轮按 query 召回 vs 启动整块注入。

**2 分钟（加上三条数据通路 + 关键数字）**
- 写：模型调 `remember(text,tags,now)` 工具 → trim → `normalize()`（折叠空白 + 小写）精确去重 → 追加 → 按 ts 升序排 → 超过 `MAX_ENTRIES=200` 从最旧端 drain → 全文件重写。
- 召回：每轮 `recall(query, 8, 4000)`，用 `semantic_score`（tag 精确 +8 / 子串 +4 / 相邻词 phrase +6 / Jaccard×20）混合排序，query 先过 `semantic_aliases` 同义词扩展；对单个 query 词，tag 精确命中(+8) 与正文 substr 命中(+4) 是 else-if 互斥、不叠加，phrase(+6) 与 Jaccard(×20) 再单独累加；结果包成 `[memory recall for this prompt]` 临时系统 note，永不写进 session 历史。
- 维护（自演化）：每次 CLI/GUI 启动跑 `consolidate(0.85)`（启发式，只丢近似重复，保最新）；`--memory-merge` 显式触发 `summarize_consolidate(0.85)`（用 fast model 把同簇合并成一条）。

**深挖（机制内核 + 取舍 + 坑）**
- 存储：每次 mutation 调 `write_all` 全文件重写（`# Project memory (nanocodex)` 标题 + 每条 header），`parse_entries` 靠 `strip_prefix("<!-- ")`/`strip_suffix(" -->")` 识别头、split token 解 `ts:`/`tags:`，文件缺失/不可读 → 空 Vec。
- 召回打分细节：keyword 长度 ≥3、phrase 窗口 = 2 相邻词、最终排序键 `overlap*1_000_000 + min(ts,999_999)`；字符截断是 greedy break（一条超了直接停，不跳过）。
- 自演化两档：`consolidate` 只能 DROP（newest-first 贪心：每个条目与 *所有已建立的簇代表* 逐一比 Jaccard，落入第一个满足阈值的簇即 first-match，无匹配则自成新簇代表，保最新、idempotent）；`summarize_consolidate` 同样是 first-match 贪心单链聚类，但能真正 MERGE（取簇内 max ts + tags 并集），summarizer 返回 None/空串 → 退化为保最新。两者比对对象都是全部代表而非单一 FIRST 代表，是 order-dependent、非传递的单链聚类。
- 召回与去重共用同一套 `word_set`/`jaccard` 原语：检索的 "相似" 和去重的 "重复" 是同一个 Jaccard 度量，只是阈值不同（召回无下限，consolidate=0.85）。

### 核心机制 · 6 根支柱

1. **单文件 markdown 存储** — `.ncx/memory/LEARNINGS.md`，每条 `<!-- ts: tags: -->` 头 + 正文，每次 mutation 全文件 `write_all` 重写 — 显式选人类可读 markdown（不是 JSONL）让开发者能手开手改，≤200 条规模下原子正确性比追加吞吐更重要。
2. **写 + 精确去重 + newest-N 截断** — `remember` 用 `normalize()` 阻止同事实大小写/空格变体重存，超 200 条丢最旧 — 保持库可信 + 有界增长 + 近期偏置；`now` 由调用方传入便于测试确定性。
3. **混合词法语义召回（每轮注入）** — `recall` = tag+8/substr+4（同词互斥 else-if）/phrase+6/jaccard×20，query 经 `semantic_aliases` 扩展，按 query 注入临时 note — ≤200 条上嵌入模型是杀鸡用牛刀，零依赖的关键词+Jaccard 混合 "够用"；按 query 召回只把相关 note 入 context 并限 token。
4. **启发式近重折叠 consolidate** — Jaccard≥0.85，newest-first 贪心：每条与所有已建簇代表逐一比、落入第一个达标的簇（first-match），只丢近似重复保最新，idempotent，每次启动自动跑 — 模型会写语义重叠的同一教训变体，折叠保召回信号纯净且库小；幂等且廉价所以可无脑每次启动跑。
5. **LLM 折叠 summarize_consolidate** — 同样的 first-match 贪心聚类（与全部代表逐一比），但把 size>1 的簇用 fast model 合成一条（max ts + tags 并集），None → 退化保最新，仅 `--memory-merge` 触发 — 启发式只能丢，LLM 能真正把多条同主题事实合成更丰富的一条；因有 LLM 成本所以不每次启动跑。
6. **INSTRUCTIONS 层（与 memory 分离）** — `load_project_instructions` 按 `~/.codex/AGENTS.md`→`~/.claude/CLAUDE.md`→repo-root 向下到 workspace 的 AGENTS/CLAUDE/.claude-CLAUDE 顺序拼接、16000 字符封顶，启动一次进系统提示 — 镜像 Codex/Claude-Code 生态的人写持久指引惯例；parent-before-child 让嵌套指令能细化 repo 级。

### 关键数字 / 必背细节
- `MAX_ENTRIES = 200` — 存储硬上限，超出从最旧端丢（`memory.rs:31`）。
- `MEMORY_RECALL_MAX_ENTRIES = 8` / `MEMORY_RECALL_MAX_CHARS = 4_000` — 单 prompt 召回块的条数与字符预算（`agent_loop.rs:20-21`）。
- consolidate / summarize_consolidate 阈值 = **0.85**（Jaccard）；单测里另有 0.8（`memory.rs:549` 等，仅测试用）。
- 打分权重：tag 精确 +8 *或* substr +4（对同一 query 词二者 else-if 互斥）、phrase +6、jaccard×20（四舍五入）（`memory.rs:327-339`）。
- keyword / word_set 最短长度 = **3** 字符；phrase 窗口 = **2** 相邻词（`memory.rs:276,354,362`）。
- recency 打包：`overlap*1_000_000 + min(ts,999_999)`（`memory.rs:109`）。
- project instructions 字符封顶 = **16_000** — 加载 + 封顶在 `runner.rs:100`（`load_project_instructions(workspace, 16_000)`）、`bridge.rs:318`（GUI 的 `load_workspace_instructions(..., 16_000)`）；注入/拼接在 `runner.rs:101`（`compose_system_prompt`）。
- 存储路径 = `<workspace>/.ncx/memory/LEARNINGS.md`（`memory.rs:53`）。
- 必背函数名：`remember` / `recall` / `consolidate` / `summarize_consolidate` / `normalize` / `semantic_score` / `semantic_aliases` / `parse_entries` / `write_all`；`Summarizer` trait 是 `#[async_trait(?Send)]`、单方法 `merge(&[String]) -> Option<String>`。
- 三个 0.85 自动/手动触发点：`consolidate` 在每次 CLI start（`main.rs:167`）、每次 LiveRunner 构造（`runner.rs:39`）、GUI 按钮（`lib.rs:879`）；`summarize_consolidate` 仅 `--memory-merge`（`main.rs:128`）。

### 取舍与坑
- **召回无最低分下限**：overlap=0 也照样按 recency 排序并填满 8/4000 caps，空/离题 query 会把 8 条最新 note 当相关推出（`memory.rs:104-129`）。
- **字符 cap 是 greedy break 不是 skip**：某条超 `max_chars` 就 `break`，一条长的早 note 会切掉本可放下的更短相关 note（`memory.rs:123-125`）。
- **recency tie-break 对真实数据基本失效**：`min(ts,999_999)` 对真实 epoch（~1.7e9）饱和到 999_999，所有真实时间戳低位相同，tie-break 实际只在 ts<999_999（即测试）下生效 — 待跟进。
- **`semantic_aliases` 单向硬编码**：是一张固定小表（如 search→web/tavily），只对这几个领域帮到语义召回（`memory.rs:295-308`）。
- **聚类是 order-dependent 的 first-match 贪心单链**：每条与 *所有* 已建簇代表逐一比 Jaccard、落入第一个达标的簇（不是只比单一 FIRST 代表），newest-first、非传递完整，渐变漂移的 note 链可能不全折叠（`memory.rs:148-160`、`193-209`）。
- **`parse_entries` 把缺失/畸形 `ts:` 静默归 0**：该条会排成最旧、最先被 cap 丢掉（`memory.rs:418`）。
- **`write_all` 全文件重写且非原子**（`std::fs::write`，无文件锁）：写到一半崩溃可能截断 `LEARNINGS.md`，单 checkout 并发 session 会 race（即已知 shared-worktree 隐患）。
- **CLI 会吃用户全局 `~/.claude/CLAUDE.md`，GUI 不会**：GUI 用 `load_workspace_instructions`（home=None）故意排除开发者个人配置，CLI 不排除，所以 CLI 跑会带上用户的 HANDOFF/GLM-debug 规则（`project_instructions.rs:24-33`）。

### 高频追问与应答

**Q1：自进化具体指什么？是 LLM 自动从对话提炼吗？**
不是自动提炼。写触发是模型驱动 —— agent 必须主动调 `remember` 工具，没有任何东西自动从一轮对话抽 learning；唯一 "VERIFIED, not guesses" 的约束就在工具描述里。"自演化" = `remember` 写入 + `consolidate`/`summarize_consolidate` 去重折叠，且只发生在 MEMORY 一侧，INSTRUCTIONS 不演化。

**Q2：为什么不用 embedding / 向量库做召回？**
≤200 条规模上嵌入模型是杀鸡用牛刀。用 tag+substr+phrase+Jaccard 的廉价混合加一张手curated 同义词表（`semantic_aliases`）就拿到 "够用" 的语义检索，零依赖。注意打分里同一 query 词的 tag(+8) 与 substr(+4) 是 else-if 互斥、不叠加，phrase(+6) 与 jaccard(×20) 才另算。代价是 `semantic_aliases` 覆盖窄、且召回无最低分下限。

**Q3：consolidate 和 summarize_consolidate 区别？为什么要两个？**
聚类逻辑相同（Jaccard≥0.85 的 first-match 贪心单链：每条与所有已建簇代表逐一比、落入第一个达标的簇），但 `consolidate` 只能 DROP 近似重复保最新，启发式、幂等、廉价，所以每次启动无脑自动跑；`summarize_consolidate` 用 fast model 把同簇真正 MERGE 成一条（取 max ts + tags 并集），更丰富但有 LLM 成本，所以只在 `--memory-merge` 显式触发，且 None/空串 → 退化为保最新，保证模型不可用时安全降级。

**Q4：召回为什么每轮算、且不写进 session？**
按 query 召回（用 `tool_query` 不是原始 user_input）只把相关 note 入 context、限住 8/4000 token；包成 `[memory recall for this prompt]` 临时 note 注入 per-iteration `notes`，never persisted —— 否则会污染对话历史并跨轮累积。它每轮 model call 都重现，但每 turn 只算一次。

**Q5：memory 和 project instructions 怎么区分？容易混吗？**
很容易混但接线完全分开：instructions 是人写、静态、启动一次加载并 16_000 封顶（`runner.rs:100`）后整块拼进系统提示（`compose_system_prompt`，`runner.rs:101`）；memory recall 是机器写、每轮临时、永不入 session（`agent_loop.rs:701-703`）。一个 whole-block startup，一个 query-scoped per-turn。

**Q6：存储为什么用单 markdown 文件而不是 JSONL/DB？**
显式选人类可读 markdown，让开发者能直接打开手改 store；注释头保持元数据机器可解析、正文是纯 prose。≤200 条小库下全文件重写简单且原子正确性优先于追加吞吐 —— 代价是非原子写、无锁、并发会 race。

### 自测 · 主动回忆

1. [L1] `LEARNINGS.md` 单条的格式是什么？文件每次怎么写？
2. [L2] `remember` 的精确去重靠什么？超过上限丢哪端？
3. [L2] `recall` 的四类打分权重各是多少？哪两类对同一 query 词是互斥的？最终排序键怎么打包 recency？
4. [L3] recency tie-break 在真实 epoch 数据下为什么基本失效？
5. [L3] 召回的字符 cap 有什么坑？无最低分下限会导致什么？
6. [L4] consolidate 与 summarize_consolidate 的聚类比对对象是什么？两者在 "能做什么" 和 "何时触发" 上各自的设计取舍？
7. [L4] 为什么召回每轮算且不写进 session，而 instructions 启动一次整块注入？这反映了什么设计意图？
8. [L2] CLI 和 GUI 在加载 project instructions 上有什么故意的差异？为什么？

**答案要点**
1. `<!-- ts:<epoch> tags:<a,b> -->\n<正文>\n\n`，前面带 `# Project memory (nanocodex)` 标题；每次 mutation 由 `write_all` 全文件重写（非原子 `std::fs::write`）。
2. 靠 `normalize()`（折叠空白 + 小写）对比已有条目，任一相等则 Ok(false) 不写；先按 ts 升序排，超 `MAX_ENTRIES=200` 从最旧（FRONT）drain。
3. tag 精确 +8、子串命中 +4、相邻词 phrase +6、`jaccard×20`（四舍五入）；其中对同一 query 词 tag(+8) 与 substr(+4) 是 else-if 互斥、不叠加，phrase 与 jaccard 再单独累加；排序键 `overlap*1_000_000 + min(ts,999_999)` 降序。
4. `min(ts,999_999)` 对 ~1.7e9 的真实 epoch 饱和到 999_999，所有真实条目低位相同，tie-break 退化为 no-op，只在 ts<999_999（测试）下生效。
5. cap 是 greedy `break` 而非 skip，一条长的早 note 会切掉后面本可放下的更短相关 note；无最低分下限 → overlap=0 也按 recency 填满 8/4000，空/离题 query 推出 8 条最新 note。
6. 两者都是 first-match 贪心单链聚类：每条与 *所有* 已建簇代表逐一比 Jaccard、落入第一个达标的簇（非只比 FIRST 代表，order-dependent）。`consolidate` 只能 DROP 近似重复（保最新），启发式/幂等/廉价 → 每次启动自动跑；`summarize_consolidate` 用 fast model 真正 MERGE 成一条（max ts + tags 并集），有 LLM 成本 → 仅 `--memory-merge` 触发，None/空串退化为保最新。
7. recall 按 query 限相关 + 限 token，临时注入不持久化避免污染历史与跨轮累积；instructions 是稳定的人写指引，整块进系统提示一次即可。意图：机器经验做动态、按需、可丢；人类指引做静态、全局、不可变。
8. GUI 用 `load_workspace_instructions`（home=None）故意排除 `~/.codex`/`~/.claude` 全局文件，让 end-user 的 chat 跟随打开的项目而非开发者个人配置；CLI 用 `load_project_instructions`（含 home），不排除，会吃用户全局 CLAUDE.md。

### 别发散到这
- session 历史 / 对话持久化机制 —— 那是 Session/AgentLoop 范畴，这里只需说 "recall 不写进 session"。
- `tool_query` 怎么选 tool schema —— 属工具路由子系统，这里只借它当召回 query。
- fast/pro 分层 orchestrator、模型路由 —— 另一个子系统；summarize_consolidate 只是 "用 fast model" 一句带过。
- skill / prompt-hook 注入细节 —— recall 只是和 budget note、prompt-hook 输出并列被 append，不展开。
- 嵌入模型 / 向量检索实现 —— 本系统明确不用，点到 "杀鸡用牛刀、零依赖" 即止。

### 一句话收尾
记住这条主线就不会浅也不会散：MEMORY 机器写、可演化、按 query 每轮临时召回；INSTRUCTIONS 人写、静态、启动整块注入；"自进化" = `remember` + `consolidate`/`summarize_consolidate`（均为 first-match 贪心、与全部簇代表逐一比对），全部围绕同一套 Jaccard 原语，阈值 0.85、上限 200、召回 8/4000。


---

## Skills · MCP · 视觉

### 一句话主线
这三者都是「可选/外部能力」插进同一套 Tool + turn 机器：共享同一个设计直觉——常驻 context 永远便宜，重内容只在相关时才拉进来。Skills 用渐进披露(只把 name+description 常驻，body 按需 load)、MCP 把外部 JSON-RPC server 的工具包成本地 `Tool`(走同一条 Approver 路)、视觉用 per-turn 路由(带图的 turn 整体切到 vision provider)。三者都不改主循环——loop 对 provider 和工具来源是无感的。

### 30 秒 / 2 分钟 / 深挖 三档

**30 秒**
- Skills:两/三层渐进披露。L1 把 `discover_skills` 收集的 skill 的 `- name: description` 索引注入 system prompt(常驻、便宜);L2 模型调 `skill` 工具按名 load 全文 body;L3 用返回的目录路径配 `read_file` 读 bundled 资源。
- MCP:`McpClient` 是 stdio 上的 JSON-RPC-2.0 客户端,spawn server → initialize 握手 → list/call;`register_mcp_server` 连一次、列工具,把每个工具包成本地 `McpTool`,写类工具走 `Approver` 审批。
- 视觉:每个 turn 开头算 `use_vision_this_turn = vision_provider.is_some() && has_image_block(user_input)`,为真则该 turn 所有 model 调用走 vision provider,否则走主 provider。

**2 分钟**(加机制名+数字)
- Skills 影子链三层:builtin(`include_str!` 烤进二进制) < home(`~/.ncx/skills`) < workspace(`<ws>/.ncx/skills`),`BTreeMap<String,Skill>` 按 name 去重、同名后者覆盖,`.into_values()` 出来按名排序。`skills_index_block` 头是固定 `INDEX_HEADER`,每行 `- name: description`,无 skill 返回空串。`skill` 工具 `read_only()=true`、入参只有一个 string `name`、大小写不敏感匹配,且写死在 `ALWAYS_VISIBLE_TOOLS` 里不被 9 工具可见性上限砍掉。
- MCP:协议版本 `2024-11-05`,每个 request 30s timeout,`clientInfo={name:'nanocodex',version:'0.1'}`。`request()` 写一行 newline-delimited JSON,然后 read-until-matching-id(跳过 id 不符的通知/响应)。一个 server 的所有工具共享 **一个** `McpClient`,包在 `Rc<Mutex<McpClient>>` 里——`Mutex` 串行化单条 stdin/stdout 管道,`Rc`(非 `Arc`)因为整个 REPL 单线程。`is_read_only_name` 是纯名字启发式:6 个前缀 + 5 个精确词判读。
- 视觉:`has_image_block` 只认顶层 JSON 数组里 `type=='image_url'` 的块。flag 在 `run_turn_inner` 入口设一次,贯穿该 turn 所有迭代(包括 tool-result 跟进),保证整个 turn 不换 provider。无 vision provider 配置时图片 turn 静默留在主 provider——优雅降级。

**深挖**(实现细节与边界)
- `Skill::load_body()`:builtin(`embedded.is_some()`)直接返回已解析的 clone、不碰文件系统;否则 read 文件 + `strip_frontmatter().trim()`。`builtin_skills()` 用 const `BUILTINS` 数组的 `include_str!`,目前仅 1 个(commit-message),无 name 的 builtin 防御性跳过。
- frontmatter 解析容错:`frontmatter_lines` 判首行用 `l.trim() == "---"`,即先 trim 再比较,故首行带前后空白(如 `--- `)仍算合法 fence——它并不要求字节级精确等于 `---`(下面自测答案 8 给出的正是这个精确说法);无闭合 fence 当 malformed(空)。`strip_frontmatter` 则剥 BOM、要求开头 `---` 后跟 `\n`/`\r`、找闭合 fence,找不到则整篇当 body。`scan_root` 缺 name 回退目录名,trim 后空 name 跳过,读不了的目录/缺 SKILL.md 跳过(`let-else` 守卫)。所以纯 markdown 无 frontmatter 也能用。
- `format_content` 把 MCP 的结构化 content 数组拍平成单字符串:`type:'text'` 块(`Some("text")`)用 `\n` join;`type` 是别的非空字符串(`Some(other)`)渲染成 `[<type> content]`;而 **缺 `type`(或 `type` 非字符串)的块走 `None => {}` 分支被静默丢弃、不渲染**。有非空 `structuredContent` 时追加;空内容按 `isError` 给 `(tool error with no content)` 或 `(no content)`。逐行对齐 Python `format_result`。
- `McpTool.execute()` 审批分流:非 read_only 时先从 approval_policy 造一个 Approver——`Approver::new(&ctx.approval_policy).classify(&self.def.name, true)`(`classify` 是 Approver 实例方法,第二个参 `true` 是 `needs_escalation`) → AutoDeny 返错误串、Ask 走 `ctx.approver.request(command:'mcp:<name> <args>', escalated:true)` 不批则中止、AutoApprove/无 approver 放行;然后 `client.lock().await` + `call_tool`。和 ShellTool 同一条逃逸/审批路。
- 视觉:`active_provider()` 仅当 flag 为真 **且** `vision_provider` 是 Some 才返回 vision provider(`NCX_TRACE` 时打 `[ncx-trace] routing image turn -> vision provider`),否则主 provider;`call_model()` 永远 `active_provider().chat_streaming(...)`,所以循环本身 provider-agnostic。

### 核心机制 · 6 根支柱
- **Skills L1 索引块** — `discover_skills` 收集后 `skills_index_block` 只把 name+description 拼成索引注入 system prompt — 让常驻成本扁平,再多/再大的 skill body 也不进 prompt。
- **Skills L2/L3 `skill` 工具** — read-only、单 string `name`、大小写不敏感 find,命中返 `body + 目录字符串`,L3 靠该目录 `read_file` 读 bundled 文件 — 全文只在模型显式请求时才展开进 context。
- **`Skill::load_body` 双源** — builtin 走 `include_str!` 嵌入二进制、返回已解析 clone 不碰盘;filesystem 源 read + strip frontmatter — builtin 无需安装、零文件系统命中,贴合单二进制目标。
- **`McpClient` stdio JSON-RPC** — spawn server,initialize+notifications/initialized 握手,`request()` 同步 write+read-until-matching-id,30s timeout,Drop 时调 `start_kill()`(fire-and-forget 发起 kill、非阻塞等待、忽略结果) — 因 agent 工具调用本就串行,简单同步循环比后台 reader+响应表「少很多机器、行为相同」。
- **`McpTool` + 共享 client** — 一个 server 全部工具共享一个 `Rc<Mutex<McpClient>>`,`is_read_only_name` 名字启发式分流审批 — 单条 stdin/stdout 管道必须串行;MCP 无机器可读副作用标志,故用名字让明显读类跳审批、写类全走 ShellTool 那条升级路。
- **Per-turn 视觉路由** — turn 入口算一次 `use_vision_this_turn`,带图整 turn 切 vision provider,文本 turn 留主 provider — 单次多模态调用发给真能看图的模型,且 flag 一 turn 设一次保证整 turn 不换 provider,无 vision 配置时优雅降级。

### 关键数字 / 必背细节
- MCP 协议版本 `PROTOCOL = "2024-11-05"`;每 request `REQ_TIMEOUT = 30s`。
- `clientInfo = {name:'nanocodex', version:'0.1'}`。
- `DEFAULT_VISIBLE_TOOL_LIMIT = 9`;`ALWAYS_VISIBLE_TOOLS = 6` 个(read_file, apply_patch, update_plan, shell, tool_search, **skill**)。
- `McpToolDef.input_schema` 缺省 = `{"type":"object"}`(server 省略 inputSchema 时)。
- read-only 名字判定 = 6 前缀(read_/get_/list_/fetch_/search_/find_)+ 5 精确词(read|get|list|search|find)。
- builtin skill 现 = 1 个(commit-message);skill 发现根 = 2 个有序(`~/.ncx/skills` 然后 `<ws>/.ncx/skills`)。
- 视觉触发 = 顶层数组块 `type == "image_url"`。
- home 目录解析顺序:`USERPROFILE` 然后 `HOME`。

### 取舍与坑
- **MCP id 匹配脆弱**:`request()` 用 `v.get("id").and_then(|x| x.as_u64()) != Some(id)` 来决定是否 `continue` 跳过(`v.get("id")` 返 `Option<&Value>`,本身没有 `.as_u64()`,故 `.and_then` 是承重的;实现里用的是取反的 `!=` 形式)。`next_id` 是 u64 从 0、用前自增(故 id 是 1,2,3…)。若 server 把 id 返成 JSON 字符串,`as_u64()` 得 None,该响应被跳过 → 最终 30s timeout。
- **MCP server stderr 被丢**:spawn 时 stderr → `Stdio::null()`,server 诊断信息全丢,只能拿到泛泛的 `closed stdout`/`timeout` 错误。
- **`is_read_only_name` 纯名字启发式**:无 server 副作用信号。`getOrCreate` 会被当只读跳审批;`read_and_delete` 也被当只读。匹配在 `to_lowercase()` 上;但 `getx`(无下划线、不是精确词)**不**算只读。
- **`skill` 工具仅当 `ctx.skills` 非空才注册**(tools.rs:271)。`builtin_skills()` 恒 ≥1,故经 `discover_skills` 实际总在;但未调 `with_skills()` 构的 `ToolContext` skills 为空、无 skill 工具。
- **视觉 flag 粘整 turn**:`use_vision_this_turn` 在 turn 入口设一次,贯穿所有迭代,带图开局的 turn 即使后续是纯文本 model 调用也仍用 vision provider。
- **skill 同名大小写**:工具匹配 `eq_ignore_ascii_case`,但 `BTreeMap` 按精确 name 去重——仅大小写不同的两个 skill 都能存活发现,工具返回 iterator 先找到的那个。
- **frontmatter fence 检查不一致**:`strip_frontmatter` 用字节前缀(开头 `---` 后须跟 `\n`/`\r`),`frontmatter_lines` 用 `l.trim() == "---"`,首行 `--- `(带尾空格)在两条路上行为微妙不同:`frontmatter_lines` 因 trim 仍判合法,byte-prefix 路因尾空格判定不同。

### 高频追问与应答
- **Q:为什么不把 skill 全文都放进 system prompt?**
  A:常驻成本要扁平。L1 只放 name+description 索引,body 可能很大,只在模型调 `skill` 工具时按需 load(L2),bundled 资源再靠返回目录 + `read_file`(L3)。和 tool_search 同一两层模式。
- **Q:一个 MCP server 的多个工具如何共享连接?并发安全吗?**
  A:`register_mcp_server` 只 connect 一次,把单个 `McpClient` 包成 `Rc<Mutex<McpClient>>`,每个 `McpTool` 持 `shared.clone()`。stdio 是单进程单管道,必须串行,`Mutex` 负责串行化;用 `Rc` 不用 `Arc` 因 REPL 单线程(!Send 运行时),`Mutex` 不是为跨线程而是为管道独占。
- **Q:MCP 怎么决定哪个工具要审批?**
  A:无 server 端副作用标志,只能用 `is_read_only_name` 名字启发式(6 前缀+5 精确词)。判为只读的放行,其余走 `Approver`——和 ShellTool 完全同一条:`Approver::new(&approval_policy).classify(name, true)` → AutoDeny 拒 / Ask 调 `approver.request(escalated:true)` / AutoApprove 放行。
- **Q:带图的 turn 里后续 tool-result 跟进调用走哪个 provider?**
  A:仍走 vision provider。`use_vision_this_turn` 在 `run_turn_inner` 入口设一次、贯穿该 turn 全部迭代,`call_model` 每次都 `active_provider()`,所以整 turn 不换。这是刻意的——避免一 turn 内 provider 抖动。
- **Q:没配 vision provider 时发图会怎样?**
  A:优雅降级、不报错。`active_provider()` 要求 flag 为真 **且** `vision_provider` 是 Some 才切,缺 provider 时 `has_image_block` 检测永不改变行为,图片 turn 静默留在主 provider。
- **Q:一个没有 frontmatter 的纯 markdown SKILL.md 能用吗?**
  A:能。`scan_root` 缺 name 回退目录名;`strip_frontmatter` 找不到合法 fence 就把整篇当 body。malformed 文件被跳过而非 crash discovery。

### 自测 · 主动回忆
1. [L1] Skills 的三层渐进披露分别在哪一步把什么放进 context?
2. [L2] `skills_index_block` 的影子覆盖顺序是什么,用什么数据结构去重?
3. [L2] 一个 MCP server 的多个工具为什么、怎么共享同一个 `McpClient`?为何用 `Rc` 不用 `Arc`?
4. [L3] `McpClient.request()` 如何把响应和请求对上号?这个机制在什么输入下会触发 30s timeout?
5. [L2] 视觉路由的触发条件是什么?flag 在哪里、何时设,作用域多大?
6. [L3] `is_read_only_name` 的判定规则是什么?举一个误判为只读和一个该只读却不算的例子。
7. [L4] `Skill::load_body()` 对 builtin 和 filesystem 源分别做什么?builtin 的 body 从哪来、为何不碰文件系统?
8. [L4] frontmatter 解析里 `frontmatter_lines` 与 `strip_frontmatter` 在首行 `--- `(尾空格)上的行为差异。

**答案要点**
1. L1:`discover_skills`→`skills_index_block` 把 name+description 索引注入 system prompt(常驻);L2:模型调 `skill` 工具按名 `load_body()` 把全文 body 放进 tool result;L3:用返回的目录 + `read_file` 拉 bundled 资源。
2. 顺序 builtin < home(`~/.ncx/skills`) < workspace(`<ws>/.ncx/skills`),同名后者覆盖;`BTreeMap<String,Skill>` 按 name 去重并排序,`.into_values()` 输出。
3. `register_mcp_server` connect 一次,单个 client 包进 `Rc<Mutex<McpClient>>` 由各 `McpTool` `clone` 共享;stdio 单管道必须串行,`Mutex` 串行化;`Rc` 因 REPL 单线程、!Send 运行时,无需跨线程的 `Arc`。
4. 写一行 JSON 后 read-until-matching-id:实现用 `v.get("id").and_then(|x| x.as_u64()) != Some(id)`(取反形式,匹配不上就 `continue`),id 不符的(通知/其它响应)跳过。若 server 把 id 返成 JSON 字符串,`as_u64()` 得 None,永远匹配不上 → 整个 read 在 30s timeout 内耗尽。
5. 条件 `vision_provider.is_some() && has_image_block(user_input)`,`has_image_block` 认顶层数组块 `type=='image_url'`;flag `use_vision_this_turn` 在 `run_turn_inner` 入口设一次,作用域是整个 turn(含所有 tool-result 迭代)。
6. 小写后:前缀 read_/get_/list_/fetch_/search_/find_ 或精确词 read|get|list|search|find 判只读。误判只读:`getOrCreate`、`read_and_delete`;该只读却不算:`getx`(无下划线,不是精确词)。
7. builtin(`embedded.is_some()`)返回已解析 body 的 clone、不碰盘;filesystem 源 read 文件 + `strip_frontmatter().trim()`。builtin body 来自 const `BUILTINS` 的 `include_str!`(编译期烤进二进制),故零文件系统命中、无需安装。
8. `frontmatter_lines` 判首行用 `l.trim() == "---"`,故 `--- ` 经 trim 后仍当合法 fence,且它并不要求字节级精确 `---`;`strip_frontmatter` 用字节前缀、要求开头 `---` 后紧跟 `\n`/`\r`,`--- ` 因尾空格在 byte-prefix 路上判定不同——两条路在该输入上行为微妙不一致。

### 别发散到这(属于其它子系统)
- tool_search 的可见性过滤/9 工具上限本体机制(这里只借「skill 在 ALWAYS_VISIBLE_TOOLS」这一点)。
- fast/pro 分层 orchestrator、`-o` 标志、fast_model 路由(那是 capability/orchestrator 子系统)。
- `ShellTool` 本身的 readonly 沙箱与 Approver 内部 classify 细节(这里只说 MCP 复用了同一条路)。
- memory layer / `MEMORY_RECALL_MAX_*`(虽在 agent_loop.rs 同文件,但与这三能力无关)。
- provider 自身的 `chat_streaming` 实现、SSE 解析、DeepSeek/具体 vision provider 的 wire 协议。
- `apply_patch`/`read_file` 的补丁格式与文件读语义(只作为 L3 的下游消费者出现)。

### 一句话收尾
记住一根脊:三个能力都是「外部/可选能力插进同一套 Tool+turn 机器」,统一靠「索引常驻、重内容按需」——Skills 按需 load body、MCP 按需 spawn+串行调用、视觉按需切 provider,主循环始终无感。


---

## ncx-forge · 骨架训练框架

### 一句话主线
ncx-forge 是一个 **API-only / 黑盒** 的训练框架：它不碰模型权重，而是把 coding agent 的「骨架/基因组」(genome = base `system_prompt` + 每个工具的 DESCRIPTION) 当作一个**纯文本替换面**来进化——通过 shell 调用真实 Rust agent `ncx.exe`、用教师面板(codex/claude/DeepSeek)提议 TOML 覆盖、以 bench 通过率的 delta 当「梯度」做爬山/Pareto 搜索;而 Rust 侧的 `unwrap_or_default` no-op 保证 + 部署前确定性 SENTINEL 自检门,是整套优化在统计上可信的前提。

### 30 秒 / 2 分钟 / 深挖 三档

**[30 秒]** 进化的是 agent 的「文本骨架」不是权重:基因组 = 基础 system_prompt + 每工具描述,从环境变量 `NCX_GENOME` 注入 Rust 侧;它只改文本不改工具行为,加载失败时硬保证 no-op。Python trainer 用 `ncx --dump-genome` 拿到真实默认基因组,教师面板提议 TOML 覆盖,再用真实 agent 跑 bench 任务打分。两种优化器:`train()` 单冠军噪声感知爬山,`evolve()` 小种群 NSGA-II Pareto(通过率↑、成本↓)。

**[2 分钟]** 加上具体机制:`Genome::from_env()` 读 `NCX_GENOME`,unset/empty/unreadable/malformed 全部回落到空基因组(`load().unwrap_or_default()`),`parse()` 只接受 trim 后非空的 `system_prompt` 和非空白的 `[tool_desc]` 表项——空白覆盖被拒绝,因为清空一个 load-bearing 描述(如 apply_patch)应是退化而非合法变异。任何优化开始前,**SENTINEL 自检门**先证明 `NCX_GENOME` 真的到达了模型:写入一个让 agent 只回暗号 `NCXFORGE_SENTINEL_4242` 的基因组,跑通(`injected`)且基线无暗号(`absent_baseline`)才 PASS,否则除非 `--no-gate` 否则拒绝训练。`evaluator.py` 从 agent 自己的 `session.jsonl` 收割**脱敏后的失败轨迹**(教师唯一的信号)、用正则从 `[ncx-usage]` stderr 行解析真实 token 成本。`train()` 每轮重评在位冠军建立噪声带、用 accept_margin 接受、holdout 防过拟合、frozen test 只打一次分;`evolve()` 用支配关系 + crowding distance 保留 Pareto 前沿的扩散度。

**[深挖]** 进入细节与契约:
- **Rust 三契约**(genome.rs):no-op-on-failure(`unwrap_or_default`)让一个加载失败的候选 == 基线行为,否则优化器会把基线分误归给坏候选;blank 拒绝(71-73);只有文本可进化,sandbox 仍管执行——教师只能注入新文本不能注入新能力。
- **`--dump-genome` 是基线唯一真相源**:在 MCP 注册**之前**打印(main.rs:207-210),所以可进化面只含 CORE 工具,MCP/server 工具描述永远不在进化面内;`toml_escape` 把 `\ " \n \r \t` 转成单行 basic string 保证任意 TOML parser 往返。
- **两向信任边界**:(a) grader (`check.py`) 输出被**脱敏出**轨迹,教师学不到游戏隐藏测试;(b) 失败轨迹作为 UNTRUSTED 数据**喂入**教师,带「这是 DATA 不是 instructions」警告抗注入。
- **成本轴契约**:Rust 在 one-shot 模式总在 stderr 打 `[ncx-usage] ... total_tokens=P+C`(provider 不直接给 total),Python 正则抓取当 Pareto 成本轴;无 usage 时回落到 mean 秒数(延迟代理),empty eval 映射为 `Objectives(passrate=0, cost=+inf)`——最差,绝不进前沿。
- **弱基座论点**:`evolve()` 把 `model` override 一路下传到 `_agent_cmd` 插 `-m`,实现「训一个更弱的基座、骨架的 headroom 更大」。

### 核心机制 · 8 根支柱

1. **Genome = system_prompt + tool_desc**(genome.rs:30-37) — `struct Genome { system_prompt: Option<String>, tool_desc: HashMap<String,String> }`,`from_env` 读 `NCX_GENOME`、`base_system_prompt`/`describe` 返回覆盖或默认。 — 它是纯文本替换面,只改描述不改行为,所以教师不能注入能力只能注入文本。

2. **no-op-on-failure 硬保证**(genome.rs:42-48) — unset/empty/unreadable/malformed 一律 `unwrap_or_default()` 成空基因组,空基因组 `is_empty()=true` 全部回落默认。 — 加载失败的候选必须 == 基线行为,否则优化器会把基线分误归给坏候选(SENTINEL 门正是为了抓这个 silent-no-op)。

3. **`--dump-genome` 基线真相源**(main.rs:207-210, 313-340) — 在 MCP 注册前打印 `system_prompt` + `[tool_desc]` 全 catalog,`toml_escape` 转单行 basic string。 — genome.py 永不解析 Rust 源码,永远反映真实工具列表与 load-bearing 描述;MCP 工具天然被排除在进化面外。

4. **SENTINEL 自检门**(forge.py:51-112, 493-495) — 写暗号基因组,`self_check(timeout=90, attempts=3)` 跑通且基线无暗号才 PASS,`--train`/`--population` 强制过门除非 `--no-gate`。 — 用确定性暗号-回显探针代替「拒绝所有任务」式探针(后者会和行为变更竞争产生噪声);重试因为 agent 偶尔即使注入成功也不回显。

5. **train() 单冠军噪声感知爬山**(forge.py:122-277) — baseline=extract_current 定义校验上限与合法工具集;每轮重评在位冠军建噪声带,best 候选按 total_passes、accept iff margin>=accept_margin 且 holdout 不回归;frozen test 只打一次分。 — agent 非确定性,和单个陈旧 gen0 比不可靠;holdout 是真正的防过拟合门,test 从不用于接受以给无偏的「训练有没有用」。

6. **evolve() Pareto 小种群搜索**(forge.py:302-438, pareto.py) — pop_cap=4,`_objectives` 映射 (passrate, cost),NSGA-II 支配 + crowding_trim 保留前沿扩散度,best=max passrate tie-break min cost。 — 单冠军丢弃权衡,Pareto 同时保留「便宜但还行」和「慢但强」;crowding distance 保边界点存活。

7. **Evaluator 失败轨迹收割 + token 成本解析**(evaluator.py:104-197) — 读 `session.jsonl`,留**最后一条 assistant 消息 + 最后 12 个工具调用**(arg preview ≤120 字符),`_redact` 把含 GRADER_MARKERS 的行替换为 `[redacted]` 并截 2000 字符;在 grade() 拷 `_check.py` 进来之前收割;timeout 无轨迹则合成一条。 — bench/run.py 只留 70 字符 grader 尾巴并 rmtree 工作区,不收割教师就是盲的;grader 输出永不外泄。

8. **Teacher 面板 · 3 个探针门控后端**(teacher.py:60-244) — Codex(model 从 `~/.codex/config.toml` 读、fallback `gpt-5`、`-o` 文件)、Claude(`opus`,由结构化 `is_error is False` 判可用而非 rc)、Api(DeepSeek,temperature 0.4);`build_teacher_prompt` 嵌当前基因组 + UNTRUSTED-fenced 失败轨迹;`parse_candidate` 取**最后一个** ```toml 块合并到 baseline。 — claude 鉴权失败也返回 rc=0,必须看 `is_error`;教师只发文本(TOML 覆盖)永不改文件。

### 关键数字 / 必背细节
- `SENTINEL = 'NCXFORGE_SENTINEL_4242'`(forge.py:51)
- `self_check` 默认 `timeout=90, attempts=3`;`injected and absent_baseline` 才 PASS(forge.py:81,106)
- train/main 默认:`--rounds=3, --repeats=1, --timeout=120s, --budget-s=1800.0, --accept-margin=1, --teacher=panel`(forge.py:468-476)
- 接受规则:候选 `margin = cev.total_passes - champ_train.total_passes >= accept_margin` **且** `chold.total_passes >= champ_hold.total_passes`(forge.py:233-237)
- `evolve --pop-cap` 默认 4(forge.py:457)
- 大小上限:`SIZE_CAP_MULTIPLIER=3, SIZE_CAP_FLOOR=12000`;`_field_cap = max(baseline_len*3, 12000)`(genome.py:26-27,89-90)
- 教师吃 top-3 失败轨迹(evaluator.py:79, forge.py:187)
- `MAX_TRAJECTORY_CHARS=2000`;arg preview 截 120 字符;留最后 12 个工具调用(evaluator.py:39,130,136)
- `GRADER_MARKERS = ('check.py','_check.py','grader','hidden test')`(evaluator.py:38)
- `_USAGE_RE = \[ncx-usage\][^\n]*\btotal_tokens=(\d+)`(evaluator.py:142)
- empty eval → `Objectives(passrate=0.0, cost=+inf)` 最差(forge.py:291)
- Codex model fallback `'gpt-5'`;Api temperature `0.4`(teacher.py:76,164)
- Codex/Claude propose 超时默认 240s;`available()` 探针超时 60s(teacher.py:98,101,125,128)
- splits `_PATTERN = [train,train,train,val,train,train,test,val]`(8-wide round-robin,train-heavy)(splits.py:26)
- taskgen:reference 跑 **两次** 查非确定性、seed 必须失败;`-n=3`,超时 240s,6 DIMENSIONS(taskgen.py:128-137)
- export `SCHEMA = 'ncx-forge-trajectory/v1'`;`reward = 1 if bench pass else 0`(export.py:38,111)
- 函数名要记:`Genome::from_env`/`parse`/`base_system_prompt`/`describe`/`is_empty`;`self_check`/`_ask`;`_objectives`;`extract_trajectory`/`_redact`/`_parse_tokens`;`parse_candidate`;`load_splits`/`_derive`;`Objectives.dominates`/`pareto_front`/`crowding_trim`/`select_population`/`best`;`taskgen.validate`/`admit`

### 取舍与坑
- **`self_check` 签名 vs 调用错位**:签名是 `self_check(timeout=90, attempts=3)`,但 main() 用 `self_check(a.timeout)` **位置传参**,所以 `--timeout`(默认 120)覆盖了 90s 的自检超时,而 attempts 仍是 3(forge.py:81,488,493)。
- **`_ask()` 泄漏 temp 目录**:用 `tempfile.mkdtemp` 建临时工作区但从不删,自检会漏目录(对比 evaluator._run_task_once 会 rmtree)(forge.py:64)。
- **基线 absence-check 只跑一次**(无重试):若基线偶发回显一次暗号,即使注入正常也会挂门(forge.py:102)。
- **holdout 只能否决不能改排名**:accept_margin 用在 TRAIN,holdout 仅「不回归」(`>=`),所以 train 涨、holdout 平的候选会被接受——holdout 不能提升排名只能 veto(forge.py:233-237)。
- **`_objectives` 成本单位会跨 run 翻转**:有任一任务报 `[ncx-usage]` 就用 tokens,否则用 mean 秒数;跨基因组比成本默认单位一致,若一个基因组报 usage 另一个不报就崩(forge.py:293-298)。
- **`mean_tokens` 跳过零 usage 任务**:只对 tokens>0 的任务求平均,部分-usage 的 run 得到的 token 成本忽略了静默任务(evaluator.py:68-72)。
- **`parse_candidate` 取最后一个 fence**:教师先发示例基因组没问题,但尾随一个无关 ```toml 块会被误解析为候选(teacher.py:230-234)。
- **Rust trim 导致非字节级往返**:`parse` trim 了值,带首尾空白的 prompt 往返不会逐字节一致;genome.py 的 `__main__` 往返断言能过仅因 `--dump-genome` 已输出 trim/escape 的单行串(genome.rs:62,72)。
- **`--from-genome` 起点不在 train() 里 validate**:只有教师候选在 211 行被 validate,退化起点带未知/空白工具键也能开跑(forge.py:159)。
- **`--repeats` 默认 1 噪声平均默认关**:CLI 默认 1,但 `evaluate()` 签名默认 3——除非用户调高 `--repeats` 否则不做噪声平均(evaluator.py:232, forge.py:469)。
- **pareto_front 是 O(n²)**,支配用精确 float `>=`/`<=`;token 成本几乎不并列,但秒数回落(舍入到 0.1)常并列,此时由 crowding-trim 决定存活(pareto.py:33-34)。

### 高频追问与应答

**Q:「加载失败时为什么必须 no-op,而不是报错退出?」**
A:因为优化器是按通过率 delta 当梯度的。如果一个坏候选基因组静默地把行为改回基线(或更糟),优化器会把分数误归因;`unwrap_or_default` 保证「加载失败 == 行为等于基线」,而 SENTINEL 门在烧预算前先证明注入真的生效,专门抓这种 silent-no-op 模式。

**Q:「SENTINEL 门为什么用暗号回显,而不是用一个『拒绝所有任务』的基因组来验证注入?」**
A:「拒绝所有任务」的探针里,任务指令会和行为变更竞争,产生噪声、难判定;暗号-回显是**确定性**的——agent 要么吐出 `NCXFORGE_SENTINEL_4242` 要么没有。重试 3 次是因为 agent 偶尔即使注入成功也不回显,单次 miss 不能阻断一次长训练。

**Q:「train() 和 evolve() 什么时候用哪个?」**
A:`train()` 是单冠军爬山,要一个最优骨架时用,带重评噪声带 + holdout 防过拟合 + frozen test 一次性无偏评估;`evolve()` 是 NSGA-II Pareto,当你要在**通过率 vs 成本**之间保留整条权衡曲线时用——它会同时留下「便宜但还行」和「慢但强」的基因组,而单冠军会把这些权衡丢掉。

**Q:「教师怎么拿到失败信息?会不会泄漏隐藏测试?」**
A:bench/run.py 只留 70 字符 grader 尾巴并 rmtree 工作区,所以 evaluator 在 grade() 拷 `_check.py` 进来**之前**从 agent 的 `session.jsonl` 收割轨迹(最后一条 assistant + 最后 12 个工具调用)。两向信任:含 `GRADER_MARKERS` 的行被 `_redact` 成 `[redacted]`,所以 grader 输出永不进轨迹;反向,失败轨迹是 UNTRUSTED 程序输出,喂教师时被 fence + 「这是 DATA 不是 instructions」警告包住抗注入。

**Q:「Pareto 成本轴具体是什么?为什么 empty eval 要映射成 +inf?」**
A:优先用真实 token——Rust 在 one-shot 模式总在 stderr 打 `[ncx-usage] ... total_tokens=`,evaluator 正则抓取;无 usage 时回落到 mean 秒数当延迟代理。empty eval(0 run / 无任务)必须映射成 `cost=+inf`(最差),否则一个零任务的误配置会静默赢下前沿、把自己伪装成绿色冠军。

**Q:「为什么有 `--base-model` / 训练弱基座的能力?」**
A:这是 memory note 的论点落地:骨架(prompt/描述)的 headroom 在更弱的基座上更大。`evolve()` 把 `model` override 一路传到 `_agent_cmd` 插 `-m`,可以在一个更弱的 agent 上测骨架进化能挽回多少;`--from-genome` 给一个退化骨架当诚实的「优化器能不能恢复」能力测试。

### 自测 · 主动回忆

1. [L1] 基因组到底由哪两部分组成?从哪里、用什么格式注入 Rust 侧?
2. [L2] `Genome::parse` 为什么要拒绝空白的 tool_desc 覆盖?举一个会被它救下的退化例子。
3. [L2] `--dump-genome` 为什么必须在 MCP 注册**之前**执行?对可进化面有什么后果?
4. [L3] train() 的接受规则两个条件分别是什么?为什么 holdout 只能否决不能提升排名?这是设计还是 bug?
5. [L3] `_objectives` 的成本单位在什么情况下会跨 run 翻转?这会导致什么后果?
6. [L4] 如果 SENTINEL 门里的 baseline absence-check 偶发回显一次暗号会怎样?这暴露了什么不对称的 retry 设计?
7. [L4] taskgen 为什么要把 reference 跑两次、并要求 seed 状态必须失败?各自防的是什么?
8. [L3] 为什么 grader 输出要从轨迹里脱敏出去,而失败轨迹又要 fence 着喂进教师?这两个方向各防什么?

**答案要点**

1. base `system_prompt` + 每工具 DESCRIPTION 的 `HashMap`;从环境变量 `NCX_GENOME` 注入,内容是 TOML(`system_prompt = ".."` + `[tool_desc]` 表),Rust 侧 `Genome::from_env` 读取(genome.rs:30-48)。
2. 因为清空一个 load-bearing 描述(如 apply_patch 的长 V4A 描述)应当被视为**退化**而非合法变异——若接受空白覆盖,agent 会被静默削弱而不是产生一个可比较的变异(genome.rs:71-73,130-136)。
3. 因为 dump 后立即 exit,在 MCP 注册前跑保证只 dump CORE 工具面;后果是 MCP/server 工具描述**永远**不在可进化面内,基因组只能覆盖核心工具描述(main.rs:206-210)。
4. ①train 上 `margin >= accept_margin`(默认 1)②holdout `chold.total_passes >= champ_hold.total_passes`(不回归)。holdout 是 `>=` 的 veto 门不参与 best 排序(best 按 train total_passes),所以它只能否决不能提升排名——这是**设计**(holdout 当防过拟合门,test 一次性无偏),但「train 涨 holdout 平也接受」是其已知软肋(forge.py:233-237)。
5. 当一个基因组的某 run 报了 `[ncx-usage]`(→tokens)而另一个的 run 都没报(→mean 秒数)时,两者成本单位不一致;跨基因组比成本会把 tokens 和秒数直接比较,Pareto 支配判断失真(forge.py:293-298)。
6. 即使注入实际正常,门也会 FAIL——因为 absence-check 只跑一次无重试,而 injection-check 有 3 次重试。这是**不对称 retry**:injection 容忍单次 miss(model noise),absence 却不容忍单次偶发回显(forge.py:96-102)。
7. reference 跑两次防**非确定性 grader**(随机/时钟),这是手写任务靠构造避免、机器任务必须主动查的失败模式;seed 状态必须 FAIL 证明任务有**真实工作量**(不是已解状态),否则一个已经满足 check 的任务会污染语料(taskgen.py:128-137)。
8. grader 脱敏出去防教师**学会游戏隐藏测试**(否则会优化出针对 check.py 的描述);失败轨迹 fence 进教师是因为它含任意 agent/程序输出属 UNTRUSTED 数据,要抗**prompt 注入**。两向信任边界(evaluator.py:12-15,89-101; teacher.py:43-47)。

### 别发散到这
- **sandbox / executor / approval 的执行管控**——基因组只改文本不改行为,执行边界归 sandbox,不要在这里展开。
- **Rust agent 的 loop / context_edit / 工具实现细节**(`max_chars=120000` 等仅作消费方提一句即可)——属 ncx core,不是 forge。
- **provider 实现(deepseek.py 的流式/重试)**——teacher 的 ApiBackend 只是 stdlib urllib 调用,别钻 provider 内部。
- **Tauri/Svelte GUI、storyboard pipeline**——完全不同子系统,只名字带过。
- **bench/run.py 的 grading 内部**——只需记住「留 70 字符尾巴 + rmtree」这一个事实(它正是 evaluator 要收割的原因),不要深入 grader 实现。

### 一句话收尾
记住开场那句主线——「进化文本骨架不动权重,no-op 保证 + SENTINEL 门让通过率 delta 当梯度变得可信」——其余所有细节都是这条主线下的具体兑现,顺着支柱往下挂数字,不要横向漂到 sandbox / core / GUI。
