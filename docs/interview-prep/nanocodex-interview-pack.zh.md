# nanocodex 技术面试 · 出题包（给模型读）

> 用法：把本文件整篇喂给一个强模型，它就能据此出面试题。下面是给模型的指令 + 设计资料。

## 给模型的指令

你是一位资深的系统/AI-infra 面试官。请**只依据下面《设计资料》**对候选人进行关于 **nanocodex**
（一个用 Rust 从零写的单二进制编码 agent + `ncx-forge` 骨架训练框架）的技术面试出题。要求：

1. **按子系统组织**，覆盖全部 8 个子系统；每个子系统出 3–5 题。
2. **按难度分层**标注每题：`[L1 理解]` / `[L2 权衡]` / `[L3 深挖]` / `[L4 开放设计]`。
3. **每题给出四件套**：① 题目；② 考察点（一句话）；③ 参考答案要点（bullet，扣住"为什么这么设计/约束是什么"）；④ 1–2 个追问（follow-up）。
4. **重设计、轻记忆**：优先考"为什么这么设计、放弃了哪个备选、被什么约束逼出来的"，而不是背 API。
5. **代码标识符保留英文**（如 `AgentLoop`、`apply_patch`、`!Send`、`ContextEditPolicy`、`NCX_GENOME`），中文叙述。
6. 末尾另出 **2 道跨子系统综合题**（串起 ≥2 个子系统的设计取舍）。

可套用的出题维度（每个子系统都适用）：
- **机制题**：解释 X 是怎么工作的（数据/控制流）。
- **权衡题**：为什么选 A 不选 B？背后的约束是什么？
- **故障题**：如果去掉 / 改坏 / 调错 X 会发生什么？怎么暴露？
- **对比题**：X 和 Y（如单 champion vs Pareto 种群、refuse-genome vs sentinel 自检）有何不同？
- **设计题**：要新增能力 Z，你会怎么改？会触碰哪些约束？

---

## 设计资料（8 个子系统）

### 1. Harness · Agent 回合循环

**一句话**：一个单线程、`?Send` 的智能体回合循环（`AgentLoop`），交替执行 call-model / run-tools——把连续的只读工具调用批量并发执行、对写操作串行化、强制实施模型/工具调用预算、轮询一个 cancel 闭包、把含图像的回合路由到独立的视觉 provider，并依据 `tool_search` 提示加上词法打分，为每个回合重新选择工具 schema 视图。

**工作原理**：`AgentLoop::run_turn` 先把事件 sink 从 `&mut self` 中取出（以规避 sink 闭包与 `&mut self` 之间的借用冲突），运行 `run_turn_inner`，随后应用 Stop hook 并恢复 sink。内层逻辑：如果存在视觉 provider 且用户内容携带 `image_url` 块（`has_image_block`），则设置 `use_vision_this_turn`；触发 `user_prompt` hook（它能以 stop_reason `blocked` 直接短路整个回合）；追加用户消息；然后以 `for iteration in 0..max_model_calls` 迭代，其中 `max_model_calls = max_iterations.min(task_budget.max_model_calls.max(1))`。每次迭代：检查 cancel 闭包；通过 `schemas_for_query(tool_query)` 重建工具 schema 列表（使得 `tool_search` 命中的工具在下一回合浮现）；在前面拼上一条合成的 system `budget_note` 以及任何 prompt-hook 输出；调用 `call_model`，后者先构建一个非破坏性的、经过上下文编辑的消息视图（`Session::for_model_edited`），再分派到 `active_provider()`（视觉或主 provider）。当 `finish_reason==\"error\"` 时返回一个 error TurnResult；当没有工具调用时记录助手文本并返回 `completed`；否则把携带 OpenAI 形态 `tool_calls` 的助手消息持久化，并按索引遍历 `response.tool_calls`。对每个位置，重新检查 cancel 与剩余的工具预算，然后决定 `parallel_run`：仅当当前调用与下一个调用按注册表都为 `read_only` 时才为 true——此时它贪心地收集连续只读调用的最长序列（受剩余预算限制），通过对 `execute_cancellable` 的 `join_all` 运行它们；否则把单个（写/未知）调用串行运行。结果按原始顺序以 `add_tool_result(id, name, result)` 追加。从 model-call 循环中正常退出会得到 stop_reason `task_budget`。`ToolRegistry::execute` 按名称分派（未知名称 → 一个反馈给模型的错误字符串），并在工具周围运行 pre/post hooks。

**设计理由（为什么）**：最关键的约束是 `Provider`、`Tool` 和 `ApprovalHandler` trait 上的 `#[async_trait(?Send)]`：整个智能体被设计为在 Tokio current-thread 运行时上运行，以便能持有非 `Send` 的状态——`Rc<RefCell<…>>` 共享上下文（计划、工具目录、工具提示）、测试中以 boxed `FnMut` 形式存在的 mock/闭包 provider，以及 GUI 的 `FnMut` 事件 sink——而无需承受多线程运行时强加的 `Send + Sync + 'static` 代价（那会把一切都推向 `Arc<Mutex<…>>` 和原子引用计数）。工作负载是 I/O 受限的（一次进行中的模型调用，随后是一小簇工具 I/O），因此多线程毫无收益；只读批量并发是用单线程上对 futures 的 `join_all` 实现的，而非 OS 线程，所以从不需要 `Send`。仅对只读调用做批量化是出于安全考量：纯读工具没有顺序依赖、可以自由地竞争执行，但写（或未知）工具必须保持串行且按声明顺序，以便文件编辑及其前后的读取能观察到一个确定性的序列（由 `write_between_reads_stays_serial_and_ordered` 验证）。双预算（模型调用 vs 工具调用）加上一条作为 system 消息注入的自描述 `budget_note`，让模型能自我调速，并保证即使它陷入循环也能终止；在 cancel/预算耗尽时回填合成的工具结果，使消息历史保持 API 合法（每个助手 tool_call 都必须有匹配的工具回复，否则下一次请求会 400）。每回合动态选择 schema（上限 `DEFAULT_VISIBLE_TOOL_LIMIT=9`、始终可见的核心集合 + `tool_search` 提示 + 词法匹配）让 prompt 保持精简、随着工具集增长而保持函数列表聚焦，否定了每回合都倾倒全部 schema 的备选方案。

**关键机制**：
- **!Send / current-thread design（单线程设计）** — `Provider`/`Tool`/`ApprovalHandler` 上的 `#[async_trait(?Send)]` 意味着返回的 future 无需是 `Send`。这让 `Box<dyn Provider>`、`EventSink = Box<dyn FnMut(LoopEvent)>` 以及 `Rc<RefCell<…>>` 共享状态能够在单线程运行时上跨越 await 存活。理由：I/O 受限的工作负载 + 廉价的 `Rc` 共享；否定了 `Arc<Mutex>`/`Send` 约束，认为它们是不必要的开销。
- **call-model → run-tools iteration（调用模型与执行工具的迭代）** — `run_turn_inner` 最多循环 `max_model_calls` 次。每一遍：`call_model`（schema + 预算/hook 的 system 注记）→ 如果 `!response.has_tool_calls()` 则返回 `completed`；否则持久化助手的 `tool_calls` 并执行它们、追加结果、继续循环。正常走到末尾则返回 `task_budget`。
- **tool-call parse + dispatch（工具调用的解析与分派）** — provider crate 把原始 JSON 解析为 `ToolCall{id,name,arguments}`（无效的/非对象的 arguments 会塌缩为 `{}`，见 types.rs）。循环把它们重新序列化为 OpenAI `function` 形态以记入历史，然后按名称通过 `ToolRegistry::execute` 分派；未知名称会向模型返回 `Error: unknown tool '<name>'.` 而非崩溃。
- **concurrent read-only batching（只读并发批量化）** — `parallel_run` 仅在 `is_read_only(calls[idx])` 且 `is_read_only(calls[idx+1])` 时触发。它贪心地收集连续只读调用的最长序列（受 `remaining_tools` 限制），用 `futures_util::future::join_all` 对 `execute_cancellable` 运行它们，并把结果按序拼接回去。非只读调用或末尾的单个调用串行运行。由 `read_only_calls_run_concurrently` 证明（4×300ms 的读取在 <800ms 内完成）。
- **task budget (dual cap)（任务预算，双重上限）** — `TaskBudget{max_model_calls:60, max_tool_calls:120}`（默认值）。`max_model_calls` 限制外层循环；在每个工具/批次之前，`remaining_tools = max_tool_calls.saturating_sub(tools_used.len())`——为 0 则触发 `budget_result`（stop_reason `task_budget`，回填未应答项）。一条 `budget_note` system 消息报告实时用量，让模型自我调速。
- **cancellation（取消）** — `run_turn` 接收一个 `Option<&dyn Fn()->bool>`。在循环顶部、每个工具/批次之前以及每个工具之后被检查。`execute_cancellable` 用 `tokio::select!`（biased）让工具 future 与一个 100ms 的轮询竞争；如果在运行途中 cancel 翻转，它返回 `[interrupted...]` 并丢弃（取消）工具 future。`stop_interrupts_a_hanging_tool` 证明了一个 `pending()` 工具会被弃置。
- **vision routing（视觉路由）** — `use_vision_this_turn = vision_provider.is_some() && has_image_block(user_input)`。`active_provider()` 仅在该回合返回视觉 provider；若无视觉 provider，含图像的回合仍留在主 provider 上。`has_image_block` 扫描内容数组寻找 `type == "image_url"` 的块。
- **dynamic per-turn schema selection（每回合动态 schema 选择）** — 每次迭代都调用 `schemas_for_query(tool_query)`。如果 `tools.len() <= 9` 则发送全部 schema；否则视图 = `ALWAYS_VISIBLE_TOOLS`（read_file、apply_patch、update_plan、shell、tool_search、skill）+ `ctx.tool_hints` 中的名称（由上一次 `tool_search` 设置）+ 经 `catalog_score` 的顶部词法匹配，上限为该 limit。`tool_search` 填充 `tool_hints`，使被发现工具的 schema 在下一回合出现。
- **non-destructive context editing（非破坏性上下文编辑）** — `call_model` 运行 `Session::for_model_edited(notes, policy)`，它构建一个发送时视图：把超过 `keep_recent_messages` 的旧（`tool`）结果压缩到 `max_tool_result_chars`，并在超过 `max_chars` 时丢弃最旧的前缀（重新对齐到一个 user 边界）——而不修改 `session.messages`。Trace 会打印原始/编辑后的字符数 + 压缩/丢弃的计数。

**控制 / 数据流**：
1. run_turn：把 event_sink 从 &mut self 中取出（避免借用冲突），调用 run_turn_inner，然后 apply_stop_hook，再恢复 sink。
2. 计算 use_vision_this_turn = 存在视觉 provider 且用户内容含 image_url 块；从用户输入中提取 tool_query 文本。
3. 触发 user_prompt hook；如果被 blocked，记录助手注记并返回 stop_reason=blocked（不进行模型调用）。否则把任何 hook 输出暂存为 system 注记，并追加用户消息。
4. 计算 max_model_calls = max_iterations.min(max_model_calls.max(1))；进入 model-call 循环。
5. 循环顶部：如果 cancel() → 回填 + 返回 cancelled。通过 schemas_for_query(tool_query) 构建 schema；在前面拼上 budget_note + prompt-hook 注记。
6. call_model：构建经过上下文编辑的消息视图（for_model_edited），选择 active_provider()（视觉 vs 主），await chat()；累计 token 用量。
7. 如果 finish_reason==error → 返回 error。如果没有 tool_calls → 记录助手文本，发出 AssistantText，返回 completed。
8. 持久化携带 tool_calls 的助手消息（OpenAI function 形态，含 reasoning_content）。
9. 按索引遍历 tool_calls：重新检查 cancel；如果剩余工具预算==0 → 回填 + 返回 task_budget。
10. 如果当前+下一个调用都是 read_only → 收集连续的只读序列（≤ 剩余预算），为每个发出 ToolStart，join_all execute_cancellable，然后按序追加结果；否则串行运行一个工具（覆盖写/未知）。
11. execute_cancellable：用 select! 让工具 future 与一个 100ms 的 cancel 轮询竞争；取消时返回一个 [interrupted] 字符串并丢弃该 future。
12. 在每个工具/批次之后重新检查 cancel（某个工具可能已挂起）；取消时回填未应答的调用并返回 cancelled。
13. 循环持续，直到模型不带工具地作答、出现 error、被取消、触及某个预算上限，或 max_model_calls 耗尽（→ task_budget 消息）。

**面试话术点 / 候选人应能说出**：
- 为何 `?Send` 是承重决策：该智能体是 I/O 受限的，运行在 current-thread Tokio 运行时上，因此可以用 `Rc<RefCell>` 共享状态、并跨越 await 持有 `FnMut`/mock provider——无需 `Send+Sync+'static`，无需 `Arc<Mutex>`。真正需要并发的地方（并行读取）用的是单线程 `join_all`，而非 OS 线程。
- 只读批量化是由正确性而非仅由速度驱动的：只有*连续*的只读调用序列才会被并行化；任何写或未知工具都会强制串行、按序执行，使编辑及其前后的读取保持确定性。该批次还会被裁剪到剩余的工具预算之内。
- 两个独立的预算（模型调用与工具调用）保证终止，并以一条实时的 system `budget_note` 暴露给模型，把一个硬性上限转化为自我调速的指引；耗尽时回填合成的工具回复，使历史保持 API 合法。
- 取消是协作式且分两层的：一个在循环/工具边界轮询的 `Fn()->bool`，外加 `execute_cancellable` 的 100ms `select!`，后者能通过丢弃工具 future 在运行途中弃置一个挂起的工具（测试用 `future::pending`）。
- 每回合动态的工具 schema 选择（上限 9、始终可见的核心集 + `tool_search` 提示 + 词法打分）让函数列表随工具增长而保持精简；`tool_search` 写入 `ctx.tool_hints`，使被发现的工具在下一回合变得可见——这是一个工具副作用与下一次 prompt 之间的反馈回路。
- 视觉路由是每回合且选择性启用的：只有当配置了独立的视觉 provider*且*该回合携带 `image_url` 块时才生效，从而把更廉价的文本模型保留为默认。
- 消息历史的健壮性：未知工具名返回一个错误字符串（而非 panic），上下文编辑是一个非破坏性的发送时视图，cancel/预算路径会回填悬空的 tool_calls，从而使 OpenAI 不变式（每个 tool_call 都有一个工具回复）永不被违反。

**取舍与坑（适合做故障题/追问）**：
- 取消的粒度是 100ms（`execute_cancellable` 的轮询）；一个比首次轮询更快返回的工具无法被中断，而且循环本身没有硬性的挂钟超时（按工具的超时存在于 `shell` 工具中，而非 harness 里）。
- 只读批量化完全依据注册表的 `read_only()` 标志；一个被错误标记为只读（实际会改动状态）的工具会被竞争执行并重排序。信任边界在工具作者那里。
- `with_max_iterations` 会覆盖 `task_budget.max_model_calls`，而 `with_task_budget` 会覆盖 `max_iterations`；两个 setter 会依调用顺序互相覆盖——有效上限始终是 `max_iterations.min(max_model_calls.max(1))`，因此一对不匹配的设置可能悄无声息地降低上限。
- 工具调用预算是在每次调用/批次*之前*检查的，所以一个并行只读批次会被调整大小以适配剩余预算，但循环永远不会部分地运行超出预算的批次——这没问题，但意味着单个超预算的助手回合会截断剩余的调用，并把它们回填为 'task budget exhausted'。
- 视觉路由在每回合上是全有或全无的：一个含图像的回合会把*整个*编辑后的历史发送给视觉 provider，而不仅仅是图像；不存在按消息粒度的模型拆分。
- 工具调用的参数解析被委托给 provider crate——无效的/非对象的 JSON 在那里塌缩为 `{}`，所以一个畸形的 arguments 数据块会悄无声息地变成空，而非在分派时报错。
- Schema 选择的词法打分（`catalog_score`）只是简单的子串/词匹配（名称精确=100、名称包含=50、内容包含=20）；若某项能力用查询不共享的同义词描述，则除非显式调用 `tool_search`，否则不会浮现。

**代码引用**：`D:/agent_prac/ncx-train/rust/crates/ncx-core/src/agent_loop.rs:22 (trait Provider, #[async_trait(?Send)])` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/agent_loop.rs:71 (TaskBudget, defaults 60/120)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/agent_loop.rs:173 (active_provider — vision routing)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/agent_loop.rs:185 (call_model — context-edited view + provider dispatch)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/agent_loop.rs:203 (execute_cancellable — 100ms select! cancel poll)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/agent_loop.rs:225 (run_turn — sink take/restore + stop hook)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/agent_loop.rs:284 (max_model_calls cap composition)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/agent_loop.rs:300 (schemas_for_query per turn)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/agent_loop.rs:379 (tool-call walk: budget check, parallel_run gather, serial else)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/agent_loop.rs:573 (has_image_block)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/agent_loop.rs:1093 (read_only_calls_run_concurrently test), 1149 (write_between_reads_stays_serial test), 1179 (stop_interrupts_a_hanging_tool test)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:25 (DEFAULT_VISIBLE_TOOL_LIMIT=9 + ALWAYS_VISIBLE_TOOLS)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:279 (schemas_for_query / schemas_limited_for_query selection)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:327 (ToolRegistry::execute — dispatch by name + pre/post hooks, unknown→error)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:396 (ToolSearchTool::execute — writes ctx.tool_hints)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/session.rs:167 (for_model_edited non-destructive view), 263 (backfill_unanswered_tool_calls)` · `D:/agent_prac/ncx-train/rust/crates/ncx-provider/src/types.rs:9 (ToolCall, arguments→{} on invalid), 39 (has_tool_calls)`

### 2. 上下文压缩 · context editing

**一句话**：一个双模式的上下文编辑器：每次调用模型时都会得到一份非破坏性的、受预算约束整形的"provider view"（先截断旧的工具结果，再在超过字符预算后丢弃最早的消息前缀），而可选启用的 /compact 则把同样的变换固化进活动历史并重写 JSONL 日志。

**工作原理**：Session 以 OpenAI chat 形态把完整对话存为 Vec<serde_json::Value>（`messages`），系统提示则单独存放在 `system` 中。ContextEditPolicy 携带四个旋钮：`enabled`、`max_chars`（默认 120_000）、`keep_recent_messages`（默认 30）、`max_tool_result_chars`（默认 4_000）。核心算法位于 `edited_body(system_notes, policy)`：它把 `self.messages` 克隆进 `body`，通过 `total_chars` 记录 `original_chars`（system + notes + 每条序列化为 JSON 的消息的字符数），随后若 `policy.enabled` 则运行两趟处理。第 1 趟（工具结果收缩）：计算 `recent_cutoff = body.len() - keep_recent_messages`（饱和减法），对每条索引 `i < recent_cutoff` 且 role 为 "tool" 的消息调用 `compress_tool_result`，它把 `content` 字符串截断为前 `max_tool_result_chars` 个字符并追加一个标记 `\n[context edited: omitted the rest of prior {name} result; original_chars=N]`，返回 true 以便 `stats.compressed_tool_results` 递增。第 2 趟（前缀丢弃）：仅当 `total_chars(...) > max_chars` 且 `body.len() > keep_recent_messages` 时，设 `start = body.len() - keep_recent_messages`，然后把 `start` 推进到该 cutoff 处或之后第一条 "user" 消息（`position(role==user)`），再跳过任何打头的 "tool" 消息，若 `0 < start < body.len()` 则设 `dropped_messages = start` 并只保留 `body[start..]`。`for_model_edited` 随后把系统消息以及任何非空的 `system_notes` 作为额外的系统消息前置，追加被编辑过的 body，并上报 `edited_chars`。`for_model` 不过是 `for_model_edited(&[], policy{enabled:false})`。`compact` 复用 `edited_body` 但把 `enabled` 强制设为 true，且仅当确实发生变化（compressed 或 dropped > 0）时才覆盖 `self.messages` 并调用 `rewrite_log` 截断并重写 JSONL 文件。这套发送时视图在 agent_loop.rs 的 `call_model` 中接入，它在每次调用 provider 的 `chat` 之前调用 `for_model_edited(system_notes, &self.context_edit)`；破坏性的 `/compact` 路径则是 CLI 的 `compact_session_text` 调用 `agent.session.compact`。

**设计理由（为什么）**：核心的设计选择是非破坏性的发送时编辑：`for_model_edited` 构建一份临时的、被裁剪过的视图，而保持 `self.messages`（规范日志）原封不动，因此尽管模型只会看到一份受预算约束的切片，完整的对话记录始终为 `--resume` 和 JSONL 日志保留下来。测试 `context_edit_compresses_old_tool_results_without_mutating_session` 正是断言了这一点（session 保留了 200 字符的原文）。这两趟处理刻意按先廉价后昂贵的顺序排列：工具结果截断先跑，因为冗长的工具输出（shell 转储、文件读取）在编码 agent 的历史中是占主导、价值最低的大块内容，所以收缩它们往往就能回到预算之内而不损失任何对话回合；只有当这还不够时，才退而去丢弃整条消息。前缀丢弃很保守：它始终保留 `keep_recent_messages`，而且与其在任意索引处切断，不如把切点推进到一个 "user" 边界并跳过打头的孤立 "tool" 消息——这维护了 OpenAI 的不变式，即历史以干净的方式开头、且没有任何 "tool" 消息缺失其前置的 assistant tool_call（畸形的前缀会被 API 拒绝）。`compact` 是可选启用的，且仅在确有工作完成时才会变更/重写（`compress > 0 || dropped > 0` 这个守卫），因此在预算之内的一次空操作 compact 会让日志逐字节保持不变（测试 `compact_noops_when_under_budget`）。字符计数（`json_chars` 作用于序列化后的 JSON，通过 `.chars().count()` 统计 Unicode 标量）是对 token 预算刻意采用的廉价代理——它避免了对分词器的依赖，以精确性换取了一个快速、与 provider 无关的启发式。截断使用 `.chars().take(n)` 而非按字节切片，以避免在多字节 UTF-8 边界上 panic。

**关键机制**：
- **ContextEditPolicy** — 四字段配置（enabled、max_chars=120k、keep_recent_messages=30、max_tool_result_chars=4k）。在 session.rs 的 Default 实现和 ncx-config 中都有默认值；CLI 通过 context_edit_from_config 用 positive_usize 回退来构建它，因此一个非正的配置值会静默回退到默认值。
- **edited_body (the algorithm)** — 两趟、由字符预算驱动。第 1 趟压缩比近期窗口更旧的工具结果；第 2 趟仅在 total_chars > max_chars 且消息数多于 keep_recent_messages 时丢弃最早的前缀。返回 (body, ContextEditStats)。
- **compress_tool_result** — 对一条 'tool' 消息的原地变更：若 content 字符数 > max_chars，则把 content 替换为 head(max_chars) 加上一个 '[context edited: ... original_chars=N]' 标记，好让模型知道发生了截断。返回 bool 以驱动 compressed_tool_results 计数器。
- **for_model_edited (non-destructive view 非破坏性视图)** — 前置 system 加上非空的 system_notes（各自作为独立的系统消息），随后是被编辑过的 body。每个回合都通过 AgentLoop::call_model 使用。保持 self.messages 不变。
- **compact (destructive materialization 破坏性物化)** — 强制 enabled=true，运行 edited_body，且仅当它确实改变了什么时才覆盖 self.messages 并对 JSONL 执行 rewrite_log()——这样 /compact 以及后续的 --resume 看到的是被压缩后的历史，而不只是一份临时视图。
- **prefix-drop boundary safety (前缀丢弃的边界安全)** — start 从 len-keep_recent 起算，推进到第一条 'user' 消息，再跳过打头的 'tool' 消息，以防被裁剪历史的头部出现一条孤立的 tool 回复（OpenAI 形态不变式）。
- **ContextEditStats** — original_chars / edited_chars / compressed_tool_results / dropped_messages——由 /compact 以 'chars X -> Y; compressed_tool_results=..; dropped_messages=..' 的形式呈现给用户。
- **resume / log integrity (恢复 / 日志完整性)** — 日志是仅追加的 JSONL，带 _ts 时间戳，且内联的 base64 图片被脱敏（redact_image_data）。resume() 读取日志，丢弃 system 行，sanitize_restored_messages 为任何悬空的 tool_call 回填合成的 '[interrupted...]' 工具回复，从而让被恢复的历史符合 API 规范。

**控制 / 数据流**：
1. AgentLoop::call_model 在某个回合被调用，并调用 session.for_model_edited(system_notes, &self.context_edit)。
2. edited_body 把 self.messages 克隆进 body，并记录 original_chars = total_chars(system, notes, messages)。
3. 若 policy.enabled：第 1 趟计算 recent_cutoff = len - keep_recent_messages，并对每条更旧的 'tool' 消息调用 compress_tool_result（截断为 max_tool_result_chars 加上编辑标记），统计 compressed_tool_results。
4. 仍在第 2 趟：若 total_chars（压缩后）> max_chars 且 len > keep_recent_messages，计算 start = len - keep_recent_messages。
5. 把 start 推进到该 cutoff 处或之后第一条 'user' 消息，然后跳过任何打头的 'tool' 消息，以保持前缀符合 API 规范。
6. 若 0 < start < len，设 dropped_messages = start 并切片 body = body[start..]。
7. 设置 edited_chars 并返回 (body, stats)。
8. for_model_edited 前置系统消息加上每条非空的 system_note 作为系统消息，追加 body，对序列化后的输出重新计算 edited_chars，并返回 ContextMessages。
9. call_model 把 edited.messages 发送给 provider 的 chat()；这条路径绝不会变更 self.messages。
10. 另外，在 /compact 时 CLI 调用 session.compact(policy)：同样的 edited_body 但把 enabled 强制设为 true；若有任何变化，self.messages 会被覆盖，且 rewrite_log 截断并重写 JSONL，从而让 resume 看到被压缩后的历史。

**面试话术点 / 候选人应能说出**：
- 最重要的区分：发送时视图（for_model_edited，每个回合，非破坏性）vs. 物化压缩（compact，通过 /compact 可选启用，会重写日志）。完整的对话记录是唯一可信源；模型看到的是它的一份受预算约束的投影。
- 先廉价后昂贵的顺序：先收缩冗长的工具结果（编码 agent 历史中的主体），仅在仍超预算时才丢弃整条消息。这把对话的损失降到最低。
- 为什么用字符数而非 token：json_chars/total_chars 使用序列化后 JSON 的 Unicode 标量计数，作为一个无分词器、与 provider 无关的预算代理——以精确性换取速度与零依赖。
- API 形态的不变式驱动着丢弃逻辑：keep_recent 下限、在 'user' 边界处切断、跳过打头的孤立 'tool' 消息，从而让被裁剪的历史绝不会以一条缺失其 assistant tool_call 的 tool 回复开头。
- 截断标记（original_chars=N）告诉模型内容被编辑过，而不是悄悄就一个变短的结果撒谎；通过 .chars().take() 做到 UTF-8 安全。
- compact 的变更守卫（compress>0 || dropped>0）让一次空操作的 compact 成为真正的空操作——日志逐字节保持不变，已由一个测试验证。
- 相邻的完整性机制：backfill_unanswered_tool_calls 和 sanitize_restored_messages 为悬空的 tool_call 合成占位的工具回复，从而让被中断/被恢复的会话保持有效；日志会脱敏内联的 base64 图片。

**取舍与坑（适合做故障题/追问）**：
- 字符数预算是对 token 的代理——它可能高估或低估真实的 token 用量，取决于语言/JSON 开销；循环中没有分词器。
- 压缩只作用于 content 为纯字符串的 'tool' 消息；多模态/数组形态的工具内容或大块的 assistant/user 消息永远不会被 compress_tool_result 收缩（只能通过前缀切割丢弃）。
- 如果近期窗口（keep_recent_messages）本身就超过了 max_chars，前缀丢弃的守卫 body.len() > keep_recent_messages 可能让视图仍然超预算——该策略从不在保留的尾部内部做裁剪，所以这个预算是软的、而非硬的。
- 第 2 趟只对压缩后的 body 重新计算 total_chars；该检查也把 self.system 加 notes 计入，因此一个很大的系统提示会算入预算却永远不会被丢弃。
- compact 通过截断+写入重写整个 JSONL；在 resume 时原始的冗长工具输出永久丢失（非破坏性视图没有这个后果）。标记文本是被裁剪内容的唯一记录。
- positive_usize 回退意味着配置为 0（例如有人试图禁用 keep-recent）会静默地恢复为默认的 30/4000，而不是按 0 的行为执行。
- 每个回合通过重新序列化每条消息来统计 edited_chars，是按每次调用模型 O(历史总大小) 的工作量——在当前规模下没问题，但它不是增量的。

**代码引用**：`D:/agent_prac/ncx-train/rust/crates/ncx-core/src/session.rs:16 (ContextEditPolicy struct + Default at :24)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/session.rs:167 (for_model_edited — non-destructive provider view)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/session.rs:192 (compact — destructive materialization + rewrite_log)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/session.rs:203 (edited_body — the two-pass algorithm)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/session.rs:455 (compress_tool_result — tool-result truncation + edit marker)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/session.rs:443 (json_chars) and :449 (total_chars — char-budget proxy)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/session.rs:375 (sanitize_restored_messages) and :263 (backfill_unanswered_tool_calls — resume integrity)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/session.rs:299 (append_log) / :321 (rewrite_log) / :414 (redact_image_data)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/agent_loop.rs:185 (call_model wires for_model_edited into every turn)` · `D:/agent_prac/ncx-train/rust/crates/ncx-cli/src/main.rs:969 (compact_session_text — /compact handler) and :1176 (context_edit_from_config)` · `D:/agent_prac/ncx-train/rust/crates/ncx-config/src/config.rs:72 (config knobs + defaults at :107)`

### 3. 工具系统 · 动态暴露

**一句话**：一个单线程的工具子系统：每一项 agent 能力都实现同一个 `?Send` 的异步 `Tool` trait，集中存放在一个基于廉价克隆的 `ToolContext` 之上的 `ToolRegistry` 中，并通过一个动态裁剪、按词法打分的 schema 视图暴露给模型，同时支持 genome 可覆盖的描述以及 pre/post hook 包裹。

**工作原理**：每一项能力都是一个实现 `trait Tool`（`tools.rs:159`）的单元结构体，模型把它看作一个 OpenAI function tool：`name()`、`description()`、`parameters()`（JSON Schema）、一个默认返回 false 的 `read_only()`，以及 `async fn execute(&self, ctx: &ToolContext, args: &Value) -> String`。该 trait 标注为 `#[async_trait(?Send)]`，因为整个 REPL 跑在 current-thread runtime 上，所以共享的可变状态（plan、tool catalog/hints）使用 `Rc<RefCell<…>>` 而非 `Arc<Mutex<…>>`。`ToolContext`（`tools.rs:55`）是一个 `Clone` 的“能力袋”，装着工具所需的一切——workspace、`SandboxPolicy`、approval policy 字符串、timeout、共享的 plan、一个可选的 `Rc<dyn ApprovalHandler>`、可选的 `MemoryStore`、搜索后端配置、`tool_catalog`、`tool_hints`、hooks、skills，以及 `Genome`——通过一系列 `with_*` builder 方法逐步构建。`ToolRegistry::new`（`tools.rs:194`）注册核心工具集（read_file、apply_patch、update_plan、shell、grep、glob、web_search、web_fetch、tool_search），并有条件地注册 `remember`（仅当 `ctx.memory.is_some()`）和 `skill`（仅当发现了 skills）——这种能力门控让模型永远看不到一个无法工作的工具。在 `register`（`tools.rs:229`）时，registry 通过 `genome.describe(name, tool.description())` 计算出生效的描述，并压入一个 `ToolCatalogEntry{name, description, read_only}`；这个 catalog 正是 `tool_search` 打分所依据的内容，因此搜索看到的文本与模型看到的文本一致。schema 的暴露是动态的：`schemas_limited_for_query`（`tools.rs:283`）在工具数量 ≤ `DEFAULT_VISIBLE_TOOL_LIMIT`（9）时返回每个工具的 schema，否则总是包含 `ALWAYS_VISIBLE_TOOLS`（read_file、apply_patch、update_plan、shell、tool_search、skill），加上 `tool_hints` 中记录的任意名字（由上一回合的 `tool_search` 设置），再用与用户查询词法匹配度最高的工具（`catalog_score`）填满剩余预算，并通过 `schema_for`（它会重新应用 genome 覆盖）渲染每一个。`execute`（`tools.rs:327`）对调用做包裹：先跑 pre_tool hooks（被拦截则以一个错误字符串短路），再跑工具，再跑 post_tool hooks，并把任何 hook 的 stdout 追加在一个 `[hook output]` 横幅之下。loop（`agent_loop.rs:300,392`）每次迭代都调用 `schemas_for_query(user_prompt)`，并用 `is_read_only` 把连续的 read-only 调用批量起来并发执行。

**设计理由（为什么）**：`?Send` trait + `Rc<RefCell>` 的选择是被 current-thread runtime 强制决定的：加上 `Send` 约束会到处都要求 `Arc<Mutex>` 却毫无收益，因为根本没有别的线程可以把它发送过去。动态 schema 暴露之所以存在，是因为每回合都暴露所有工具既浪费 prompt token，又会随着 registry 增长（MCP 工具、skills、search）而劣化模型的工具选择；常量 `ALWAYS_VISIBLE_TOOLS` 保证那批不可替代的核心始终在场（你永远能 read、patch、plan、跑 shell、搜索更多工具、加载一个 skill），而 `tool_search` 提供了逃生通道——它的结果会变成 `tool_hints`，让匹配到的 schema 在下一回合浮现。genome 的描述覆盖路径被刻意设计成一个纯文本替换面：训练用的 harness（ncx-forge）可以演化描述和 base prompt，但永远改不了工具的*行为*，因为执行始终由 sandbox 把关——这既阻止了候选 genome 注入新能力，又让一次失败的加载成为有保证的 no-op（空 `Genome` → 字节级一致的 schema，由一个测试断言）。`schema_for` 是一个区别于 trait 的 `to_schema()` 的 registry 方法，这样面向模型的 schema 可以携带覆盖，而 trait 仍然返回它未经修改的默认值。`read_only()` 默认为 false 是 fail-safe：一个未分类的工具会被当作有副作用的写操作，永远不会被自动并发批处理。`remember`/`skill` 的有条件注册避免了为不存在的能力打广告。apply_patch 的描述是逐字承重的：把它裁短会让模型发出 V4A parser 拒绝的 git/unified-diff 语法，从而让这一回合空转。

**关键机制**：
- **Tool trait (?Send async)（?Send 异步工具 trait）** — tools.rs:158-182。name/description/parameters/read_only（默认 false）/execute 返回一个 String 结果；to_schema() 构建 OpenAI function-tool JSON。之所以 ?Send，是因为 REPL 是单线程的 current-thread runtime。
- **ToolContext as a cloneable capability bag（作为可克隆能力袋的 ToolContext）** — tools.rs:54-155。持有 policy、approver(Option<Rc<dyn ApprovalHandler>>)、plan(Rc<RefCell<Vec<Value>>>)、tool_catalog、tool_hints、hooks、skills、genome、memory、search 配置；带 with_* builder 方法。廉价的 Rc 克隆，无锁。
- **Conditional registration / capability gating（有条件注册 / 能力门控）** — tools.rs:194-218。核心九件始终注册；RememberTool 仅当 ctx.memory.is_some()；SkillTool 仅当 !ctx.skills.is_empty()。测试 skill_tool_registered_only_when_skills_present 断言了这一点。
- **DEFAULT_VISIBLE_TOOL_LIMIT + ALWAYS_VISIBLE_TOOLS（默认可见工具上限 + 始终可见工具集）** — tools.rs:25-33。上限 9。若 registry 大小 <= 上限，则暴露全部；否则强制纳入这 6 个核心名字（read_file、apply_patch、update_plan、shell、tool_search、skill）外加 hints 外加最高的词法匹配，直到达到上限。
- **tool_search lexical scoring（tool_search 词法打分）** — tools.rs:432-466。tool_words 转小写、按非字母数字字符切分（保留 '_'）、丢弃 <2 字符的词与重复词。catalog_score：name 精确匹配 +100，name 子串 +50，name-or-description 子串 +20；降序排序，平手时按 name 决胜。ToolSearchTool 把匹配项写入 tool_hints 以便下一回合暴露。
- **Genome description override (schema_for)（Genome 描述覆盖）** — tools.rs:251-261 + genome.rs:94。register() 把 genome.describe() 烘焙进 catalog；schema_for() 把它重新应用到面向模型的 schema，而 Tool::to_schema() 保持默认。空 genome = 字节级一致（已测）。
- **Hook wrapping in execute()（execute() 中的 hook 包裹）** — tools.rs:327-366。run_matching_hooks(PreTool)——被拦截 => 返回错误字符串，不执行;运行工具;run_matching_hooks(PostTool);把非空的 notes 拼接在 '\n\n[hook output]\n' 之下。
- **read-only classification feeding concurrent batching（read-only 分类驱动并发批处理）** — tools.rs:166-168,267-269；agent_loop.rs:392-405。read_only() 是按工具的；is_read_only(name) 让 loop 收集一段连续的 read-only 调用并行执行;写操作保持串行。
- **apply_patch sandbox-escape escalation（apply_patch 沙箱逃逸升级）** — tools.rs:572-645。先用 parse_patch 解析；任何未通过 policy.can_write 的目标会被收集起来;若有 approver，则用一个 ApprovalRequest(escalated=true) 来对它们门控，被批准的路径会被加入 can_write 闭包;若无 approver，写操作直接以越出沙箱的消息失败。

**控制 / 数据流**：
1. loop 迭代计算 tool_query = 用户 prompt 文本（agent_loop.rs:248），并调用 tools.schemas_for_query(tool_query)（agent_loop.rs:300）。
2. schemas_for_query -> schemas_limited_for_query(query, 9)。若 tools.len() <= 9，返回每个工具的 schema_for()（已应用 genome）并结束。
3. 否则先用存在的 ALWAYS_VISIBLE_TOOLS 给 `selected` 播种，再加入 ctx.tool_hints 中存在的每个名字。
4. 用 catalog_score 针对 tool_words(query) 给剩余的 catalog 条目打分；降序排序（name 决胜）；不断加入名字直到 `selected` 达到上限，并以 score>0 为门控（query 为空时除外）。
5. 为恰好这些被选中的工具返回 schema_for()——这就是发给模型的 `tools` 字段。
6. 模型返回 tool 调用；loop 把连续的 is_read_only 调用批量起来并发运行，写操作串行运行（agent_loop.rs:392）。
7. registry.execute(name,args)：运行 PreTool hooks；若被拦截，则返回错误字符串而不执行。
8. tool.execute(&ctx,&args) 运行该能力（例如 apply_patch 解析、检测越出沙箱的目标、向 approver 升级，然后应用）。
9. 运行 PostTool hooks；把任何非空的 pre+post hook notes 追加在一个 [hook output] 横幅之下；返回结果字符串。
10. 如果该调用是 tool_search，ctx.tool_hints 会被重写为匹配到的名字，使得那些 schema 在下一次迭代中变得可见。

**面试话术点 / 候选人应能说出**：
- 为什么用 ?Send + Rc<RefCell> 而不是 Arc<Mutex>：runtime 是 current-thread，所以 Send 毫无收益，而锁纯属开销——是这个约束（单 REPL 线程）决定了类型选择。
- 动态 schema 暴露是一种 token 预算 + 工具选择准确率的优化：ALWAYS_VISIBLE_TOOLS 保证那批不可替代的核心，tool_search + tool_hints 是发现机制的逃生通道，词法打分在 DEFAULT_VISIBLE_TOOL_LIMIT=9 之下挑出与任务相关的额外工具。
- 两层描述路径：register() 把 genome 覆盖烘焙进 catalog（这样 tool_search 打分用的就是模型读到的同一份文本），而 schema_for() 把它重新应用到面向模型的 schema，Tool::to_schema() 则刻意保持默认——把面向模型的表面与 trait 默认值分离开。
- Genome 作为一个纯文本替换面：训练能演化描述和 base prompt，但永远改不了行为;执行始终由 sandbox 把关，而一个空的/失败的 genome 是有保证的字节级一致 no-op（由测试断言）——正是这个 no-op 保证让一次训练运行可被信任。
- read_only() 默认 = false 是 fail-safe：未分类的工具被当作写操作，永远不会被自动并发运行;loop 只并行化连续 read-only 调用构成的段。
- 能力门控：remember 与 skill 只在其支撑资源存在时才注册，所以模型永远看不到一个它用不了的工具。
- Hook 包裹把 registry 变成确定性的策略收口点：pre_tool 可以硬拦截一个调用，post_tool 的输出会被回喂给模型——所有工具都通过 ToolRegistry::execute 统一获得这一点。
- apply_patch 的逐字描述是承重的 prompt engineering：把它裁短会让模型退回到 V4A parser 拒绝的 git/unified-diff 语法，导致这一回合空转——这一点直接记录在源码注释里。

**取舍与坑（适合做故障题/追问）**：
- 仅有词法（子串）打分——没有词干提取/嵌入;对 'installer' 做 catalog_score 不会匹配到 'install'，而且 tool_words 丢弃 <2 字符的 token，所以单字母查询什么都匹配不到。
- tool_hints 是全局可变状态，每次 tool_search 调用都会清空（tools.rs:413-414）;一次搜索会覆盖之前的 hints，所以只有最近一次搜索的匹配项会在下一回合保持可见。
- 当 query 非空时，额外的工具需要 score>0 才会被纳入;如果每个非核心工具都得 0 分，即便还有预算富余，模型也只能看到 ALWAYS_VISIBLE_TOOLS + hints（即 tools.rs:314 处的 `score > 0 || q.is_empty()` 门控）。
- schema_for 在每次迭代中对每个被选中的工具都重新运行 genome.describe 和 tool.parameters()——是重算，不是缓存（在这个规模下、单线程里没问题）。
- 这里的 read-only 分类是按 Tool（trait）的，与 MCP 工具按名字启发式分类（mcp_tool.rs 里的 is_read_only_name）以及 shell 的运行时 looks_read_only 命令启发式各不相同——三套不同的 read-only 概念并存。
- 没有 approver 的 apply_patch 会悄悄降级为一次普通的策略拒绝而不是发起询问;只有当 ctx.approver 为 Some 时升级才会启动。
- ALWAYS_VISIBLE_TOOLS 是一个硬编码的名字 &[&str];一个被改名/遗漏的核心工具会悄无声息地从这套保证集中掉出（contains_key 守卫意味着缺失的名字只是被跳过，不报错）。

**代码引用**：`D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:25 (DEFAULT_VISIBLE_TOOL_LIMIT / ALWAYS_VISIBLE_TOOLS)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:48 (ApprovalHandler ?Send trait)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:55 (ToolContext + builders)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:159 (Tool trait, read_only default, to_schema)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:194 (ToolRegistry::new conditional registration)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:229 (register: genome-baked catalog entry)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:251 (schema_for genome override)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:283 (schemas_limited_for_query selection)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:327 (execute: pre/post hook wrapping)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:396 (ToolSearchTool.execute writes tool_hints)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:432 (tool_words) / :446 (catalog_score)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:537 (apply_patch load-bearing description) / :572 (escape escalation)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/genome.rs:94 (Genome::describe)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/agent_loop.rs:300 (schemas_for_query per-iter) / :392 (read-only batching)` · `D:/agent_prac/ncx-train/rust/crates/ncx-sandbox/src/approval.rs:161 (Approver::classify) — shell decision used by ShellTool` · `D:/agent_prac/ncx-train/rust/crates/ncx-tools/src/detect.rs:53 (looks_read_only) — shell escalation heuristic`

### 4. 沙箱 · 审批状态机

**一句话**：两个正交的纯决策层——`SandboxPolicy`（物理上允许什么：读 / 写 / 网络）与 `Approver`（当某个动作超出沙箱范围时该怎么办：在四种审批策略下给出 AutoApprove/Ask/AutoDeny）——并且刻意把所有实际的强制执行与交互提示都下推到消费方 crate 中。

**工作原理**：`SandboxPolicy`（policy.rs）持有一个 `mode` 字符串（`read-only`/`workspace-write`/`danger-full-access` 三者之一）、一个已归一化的 `workspace` PathBuf、额外的 `writable_roots`、`network_access`，以及一个可选开启的 `allow_temp_write` 标志。`can_read` 对全部三种模式都无条件返回 `true`（敏感文件保护交由工具层负责）。`can_write` 采用短路逻辑：danger-full-access 返回 `true`，read-only 返回 `false`；否则它先把目标路径相对 workspace 转成绝对路径，再做词法归一化，然后当且仅当它 `starts_with` `writable_dirs()` 中的任一目录（workspace + writable_roots + 仅在 `allow_temp_write` 开启时才加入的系统临时目录）时返回 true。路径比较通过 `normalize` 纯粹按词法进行（丢弃 `.`、遇到 `..` 则弹出、从不触碰文件系统），因此一个决策永远不依赖某个路径是否真实存在。`Approver`（approval.rs）是对一个 `policy` 字符串的薄封装；它的 `classify(command, needs_escalation)` 返回一个 `Decision` 枚举：`never` -> 需要升级则 AutoDeny，否则 AutoApprove；`on-request` -> 需要升级则 Ask，否则 AutoApprove；`on-failure` -> 始终 AutoApprove（先在沙箱内运行，只有失败后才询问，由上游处理）；`untrusted` -> 仅当 `is_trusted(command)` 为真且无需升级时才 AutoApprove，否则 Ask。`is_trusted` 首先拒绝任何命中 `dangerous_patterns` 正则的命令（rm -rf、dd、mkfs、fork 炸弹等），然后用首个 token（去掉路径、去掉 `.exe`）与 `TRUSTED_COMMANDS` 比对，并且对 `git` 进一步拒绝 `GIT_WRITE_SUBCMDS` 中的写子命令。`step_decision(base, is_write, require_step_approval)` 在此之上叠加逐步确认：当用户禁用了自动审批时，它会把一个 AutoApprove 的写操作升级为 Ask，但绝不软化 AutoDeny，也绝不把已有的 Ask 降级。关键在于，`needs_escalation` 这个输入以及只读 shell 启发式判断（`looks_read_only`）都不在本 crate 内——消费方 `ncx-core`（tools.rs 的 `ShellTool::needs_escalation`）根据策略计算升级需求，而 `ncx-tools`（detect.rs）拥有那个保守的只读分类器。

**设计理由（为什么）**：核心设计决策是把两个正交的维度分离开（lib.rs 的文档明确这样说）：沙箱回答“这在物理上被允许吗？”，而审批器回答“当它不被允许时我们怎么办？”。这借鉴了 Codex 的做法，使同一套策略能在不同信任级别下工作，而不会把强制执行与交互提示纠缠在一起。两个层都被刻意设计为纯决策函数——没有 I/O、没有 UI——因为实际的是 / 否提示属于上层关注点（CLI=文本提示，GUI=模态往返，测试=预设答案），而强制执行（真正去应用一个补丁 / 派生一个进程）则位于 executor/tools crate 中。路径比较采用词法方式而非规范化 / 解析符号链接，是为了让 `can_write` 对不存在的路径也是全函数且确定的（你必须对一个即将创建的文件进行门控）——文档注释指出这与 Python 端的 `Path.resolve(strict=False)` 相匹配。`allow_temp_write` 默认为 false（“收紧的默认值”），用以堵住这样一个漏洞：workspace 本身可能就位于系统临时目录之下，但一个同级的临时文件仍应被拒绝。`untrusted` 的允许清单保守且偏只读，并且 dangerous-pattern 检查首先执行，因此一个受信任的首个 token（裸 `rm`，或 `git clean`）无法把一条破坏性命令偷渡过门。`step_decision` 在安全方向上是单调的（只会提升审批，绝不放松），因此加入逐步确认永远不会让系统变得更不安全。

**关键机制**：
- **Two orthogonal axes (两个正交维度) (lib.rs)** — `SandboxPolicy` = 物理权限（can_read/can_write/network）；`Approver` = 升级决策。两者皆为纯函数；强制执行与交互提示存在于别处（executor/tools crate、CLI/GUI）。这一拆分就是整个架构。
- **can_write short-circuit + lexical starts_with (短路 + 词法 starts_with) (policy.rs:86-100)** — danger-full-access -> true；read-only -> false；workspace-write -> `normalize(make_absolute(workspace, path))` 然后判断是否有任一 writable_dir 满足 `starts_with`。按组件逐段的 starts_with 既覆盖 target==root 也覆盖祖先目录，并且不会像字符串前缀那样把 /a/bc 误判为在 /a/b 之下。
- **writable_dirs composition (可写目录组合) (policy.rs:70-77)** — 始终包含 workspace，加上 writable_roots，再加上仅当 `allow_temp_write` 被开启时才纳入的系统临时目录。这种临时目录默认不可写的设定正是被 `workspace_write_denies_system_temp_by_default` 测试覆盖的、刻意收紧的行为。
- **can_read always true (can_read 恒为 true) (policy.rs:81-83)** — 全部三种模式都允许读取；敏感文件保护被明确划归为工具层的职责，而非沙箱的工作。这让策略保持最小化。
- **Decision enum + classify per-policy (Decision 枚举 + 按策略分类) (approval.rs:24-32, 161-188)** — AutoApprove/Ask/AutoDeny。never=升级时拒绝，on-request=升级时询问（默认），on-failure=始终批准（失败后重试由上游处理），untrusted=由允许清单门控。未知策略落到 Ask（安全默认）。
- **is_trusted allowlist with dangerous-patterns-first (允许清单 + 危险模式优先) (approval.rs:91-142)** — 正则 `dangerous_patterns()` 在允许清单之前检查，因此即便首个 token 受信任，破坏性命令仍会失败。首个 token 经过路径与 `.exe` 归一化（C:\\tools\\git.exe -> git）。`git` 被特殊处理：第一个非 flag 子命令会与 `GIT_WRITE_SUBCMDS` 比对。
- **step_decision monotone upgrade (step_decision 单调升级) (approval.rs:54-62)** — 当 `require_step_approval` 开启时，一个 AutoApprove 的写操作（WRITE_TOOLS = shell、apply_patch）会变为 Ask。AutoDeny 绝不被软化；已有的 Ask 被保留。它是在升级决策之上的纯粹叠加层。
- **Escalation computed by the consumer, not this crate (升级由消费方计算，不在本 crate) (ncx-core tools.rs:988-998)** — `ShellTool::needs_escalation`：danger-full-access 从不升级；read-only 对任何非 `looks_read_only` 的命令都升级；workspace-write 仅当工作目录本身不是 `can_write` 时才升级。随后用该布尔值调用 `classify`（tools.rs:1056-1057）。
- **Read-only shell classifier lives in ncx-tools, not here (只读 shell 分类器位于 ncx-tools，不在这里) (detect.rs)** — `looks_read_only`：拒绝重定向 / 命令替换的元字符，按链式操作符切分，要求每一个分段都以一个 READ_ONLY_PREFIX 开头。存疑时保守判定。它与 approval.rs 中 untrusted 用的 `TRUSTED_COMMANDS` 允许清单是不同的东西。

**控制 / 数据流**：
1. 调用方构建一个 `SandboxPolicy::new(mode, workspace)`，并可选地链式调用 `with_writable_roots` / `with_allow_temp_write` / `with_network_access`。
2. 对于一条 shell 命令，消费方（ncx-core `ShellTool::needs_escalation`）判定是否升级：danger -> false；read-only -> `!looks_read_only(command)`（ncx-tools detect.rs）；workspace-write -> `!policy.can_write(workdir)`。
3. 一个 `Approver::new(approval_policy).classify(command, needs_escalation)` 返回一个 `Decision`。
4. 对于 untrusted，classify 会运行 `is_trusted`：先跑 `dangerous_patterns` 正则（任一命中 -> 不受信任），然后做首个 token 归一化并判断 `TRUSTED_COMMANDS` 成员资格，最后拒绝 git 的写子命令。
5. 若启用了逐步确认，`step_decision(base, is_write, require_step_approval)` 会把一个 AutoApprove 的写操作升级为 Ask（AutoDeny 不动）。
6. AutoApprove -> 在沙箱内运行；Ask -> 上层通过一个 `ApprovalRequest`（command/reason/cwd/escalated/details）向人类发起提示；AutoDeny -> 拒绝并上报。
7. 对于 apply_patch（ncx-core tools.rs:577-630）：解析补丁，把每一个未通过 `policy.can_write` 的目标收集进一个“escaping”集合；若该集合非空且存在 approver，则发送一个携带完整补丁作为 details 的、已升级的 `ApprovalRequest`；批准后这些路径会被加入一个 can_write 闭包，否则 apply_patch 以越出沙箱的错误信息失败。
8. apply_patch（ncx-tools patch.rs:347）在内存中暂存所有 hunk，并在向磁盘写入任何内容之前，让每一个被触及的路径（以及任何 move_to 目标）都经过 can_write 闭包的门控——原子化的全有或全无。

**面试话术点 / 候选人应能说出**：
- 为什么是两层？`SandboxPolicy` 回答“物理上允许吗？”，`Approver` 回答“当不允许时怎么办？”——正交、两者皆纯、强制执行 / 交互提示被推出去。这是头号设计，并在 lib.rs 中被逐字陈述。
- 为什么用词法路径归一化而非 canonicalize？因为 `can_write` 必须对尚不存在的文件也有效（你要对一次创建做门控），所以它不能依赖文件系统；它对应 Python 的 `Path.resolve(strict=False)`。权衡：它不跟随符号链接，因此符号链接逃逸是一个被推给强制执行层去处理的已知局限。
- 按组件逐段的 starts_with 与字符串前缀对比：代码注释里有解释——/a/bc 正确地不在 /a/b 之内。一个朴素的字符串前缀检查会是一个真正的逃逸 bug。
- `allow_temp_write` 默认 false：一个微妙的漏洞——workspace 可能位于系统临时目录之下，但一个同级的临时文件除非显式开启否则仍必须被拒绝。这一点有一个专门的测试。
- 四种审批策略干净地对应：never（拒绝升级）、on-request（询问，默认）、on-failure（运行后由上游再询问）、untrusted（允许清单门控）。未知策略 -> Ask，一个安全的兜底。
- `is_trusted` 中的防御顺序：危险模式在允许清单之前检查，因此一个受信任的首个 token（裸 rm、git clean）无法绕过——由 `dangerous_pattern_blocks_trusted_leading_token` 和 `untrusted_blocks_dangerous_even_if_leading_token_trusted` 测试覆盖。
- `step_decision` 是单调的：它只会增加审批阻力（AutoApprove 写 -> Ask），从不减少，也从不触碰 AutoDeny——因此逐步确认不可能降低安全性。
- 存在两个不同的只读分类器，不应被混为一谈：approval.rs 的 `TRUSTED_COMMANDS`（用于 untrusted 策略）和 ncx-tools detect.rs 的 `looks_read_only`（用于 read-only 模式的升级判定）。两者都是保守的允许清单，但服务于不同的门控。
- `needs_escalation` 不在本 crate 内——它由消费方根据策略计算。沙箱 crate 保持为一个纯决策库；这正是它可被测试、可在 CLI/GUI/测试间复用的原因。
- apply_patch 升级的交互体验：`ApprovalRequest.details` 字段携带完整的 diff，因此人类审查的是实际的改动而不只是文件名——这是一个刻意的“信任但出示证据”的选择。

**取舍与坑（适合做故障题/追问）**：
- 仅词法归一化意味着符号链接不会被解析——workspace 内一个指向外部的符号链接会通过 `can_write`。该 crate 的文档把强制执行界定为 executor 的职责，因此符号链接逃逸的缓解不在本层范围内。
- `can_read` 在每种模式下都返回 true——本层没有任何读沙箱；读取机密内容只在工具层（若有的话）才会被阻止。很容易过度假设沙箱保护了读取。
- `TRUSTED_COMMANDS` 为 untrusted 策略纳入了 `python`/`python3`/`node`，它们可以执行任意代码；注意 detect.rs 明确做了相反的事（排除 python -c / node -e）。这两份清单体现了不同的威胁假设，可能令人意外。
- `is_trusted` 解析首个 token 的 git 子命令时按空白切分；一个未加引号、含空格的可执行路径会在空格处被切开（与 Python 的 shlex 行为相同）——只有不含空格的路径前缀才能正确归一化，正如测试注释所指出的。
- on-failure 在 classify 中始终返回 AutoApprove——“沙箱内失败后询问是否在沙箱外重试”这一行为完全在上游；单看 classify 会让 on-failure 看起来从不提示。
- `step_decision` 以调用方提供的 is_write/WRITE_TOOLS（shell、apply_patch）为依据；沙箱 crate 自身并不知道当前运行的是哪个工具，因此正确的逐步门控取决于消费方传入正确的 is_write 标志。
- `network_access` 是策略从不强制执行的一个普通字段；即便 danger-full-access 也会保持 network_access=false，除非某个调用方（load_config）翻转它（由 `danger_full_access_allows_everything` 测试覆盖）。

**代码引用**：`D:/agent_prac/ncx-train/rust/crates/ncx-sandbox/src/lib.rs` · `D:/agent_prac/ncx-train/rust/crates/ncx-sandbox/src/policy.rs` · `D:/agent_prac/ncx-train/rust/crates/ncx-sandbox/src/approval.rs` · `D:/agent_prac/ncx-train/rust/crates/ncx-sandbox/src/policy.rs:86-100 (can_write short-circuit + lexical starts_with)` · `D:/agent_prac/ncx-train/rust/crates/ncx-sandbox/src/policy.rs:70-77 (writable_dirs incl. opt-in temp)` · `D:/agent_prac/ncx-train/rust/crates/ncx-sandbox/src/policy.rs:132-146 (normalize, lexical)` · `D:/agent_prac/ncx-train/rust/crates/ncx-sandbox/src/approval.rs:161-188 (classify per-policy)` · `D:/agent_prac/ncx-train/rust/crates/ncx-sandbox/src/approval.rs:91-142 (dangerous_patterns + is_trusted + GIT_WRITE_SUBCMDS)` · `D:/agent_prac/ncx-train/rust/crates/ncx-sandbox/src/approval.rs:54-62 (step_decision)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:988-998 (ShellTool::needs_escalation, consumer)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:577-630 (apply_patch escalation + ApprovalRequest with full-diff details)` · `D:/agent_prac/ncx-train/rust/crates/ncx-tools/src/detect.rs (looks_read_only, separate read-only classifier)` · `D:/agent_prac/ncx-train/rust/crates/ncx-tools/src/patch.rs:347-404 (apply_patch gates every path through can_write, staged in memory)`

### 5. 分层编排器 (flash / pro)

**一句话**：一个分层、递归的节点图，按任务风险来调配投入的努力程度——`classify`（用 fast 模型）将任务路由到 single-run / plan→best-of-N-workers→verify / 递归 decompose——把"廉价模型+强模型"这一对组合省下来的成本花在结构上（并行尝试、闭环的 verify-retry 循环、分治），从而提升完成率。

**工作原理**：`Orchestrator::handle` 调用 `handle_at(task, 0)`，即递归核心（用 `LocalBoxFuture` 装箱，因为一个会递归调用自身的 `async fn` 在类型上会无限膨胀，同时这样也保留了 `?Send`/current-thread 特性）。它首先运行 `classify`——一个无工具的 `reason(Tier::Fast, CLASSIFY_SYS, ...)`，其文本由 `parse_complexity` 解析（对 high/simple/medium 做子串匹配；任何不明确的情况默认归为 Medium，即安全的中间档）。Simple 任务走单次 `run(Tier::Fast, WORKER_SYS, ...)`，不做 plan/verify。Medium 走 `pipeline` = `plan(Tier::Main)`，然后用 `self.cfg.workers`（默认 2）个并行 fast worker 跑 `run_attempts`，并在 Fast 档上做 verify。High 任务在 `depth < cfg.max_depth` 时走 `decompose_and_recurse`；否则（深度预算用尽，或 max_depth=0）回退到用 `high_workers`（默认 3）跑 `run_attempts`，并在 Main 档上做 verify。`run_attempts` 是那个闭环：它通过对 `run_worker(i, n, ...)` future 做 `join_all` 来启动 N 个 worker（每个 worker 由 `build_worker_task` 喂入 plan 加上上一轮的 verifier 反馈），对所有结果跑一次 `reason(verify_tier, VERIFY_SYS, ...)`，而 `verdict_passed` 除非文本中包含 'FAIL' 否则一律判通过（fail-loud）。在通过时，或当 `rounds > max_verify_retries` 时，它通过 `parse_best_worker` 解析 `BEST:<n>`（从 1-based 转 0-based，并做 clamp），调用 `promote_worker(best)`，`synthesize` 返回最佳结果（如果始终没有通过，则标注为 '[unverified after retries]'）；否则该裁决文本成为反馈并继续循环。`decompose_and_recurse` 先做 plan（Main），运行 `reason(Tier::Main, DECOMPOSE_SYS, ...)`，再做 `parse_subtasks`（优先识别 'SUBTASK:' 行，回退到识别带编号/项目符号的列表）；少于 2 个子任务即视为原子任务 → 复用该 plan 走 best-of-N 回退；否则截断到 `max_subtasks`（默认 6），将每个子任务依次（SEQUENTIALLY）通过 `handle_at(st, depth+1)` 处理（这样每个子任务被提升的胜出结果对下一个子任务可见），对所有的 `verify_passed` 做 AND 组合，最后对拼接后的子结果做一次 Main 档的最终 verify。

**设计理由（为什么）**：模块文档字符串陈述了明确的前提："能力的瓶颈在于模型，而非 harness"，因此 orchestrator 用结构而非更聪明的模型来换取可靠性——它把廉价模型省下来的成本花在并行尝试、verify 和分解上。它有意接受一个硬性上限：plan 与 verify 都跑在 Main 上，所以它"无法超越主模型的推理上限"——所获得的收益是 simple/medium 任务上的完成率，以及 high 任务上分治带来的覆盖范围。努力程度按风险匹配，使廉价任务保持廉价（Simple 完全跳过 plan/verify；Medium 在 Fast 而非 Main 上做 verify）。`AgentRunner` trait 抽象了每一次模型调用，使该模块与 provider 无关，并可用一个脚本化的 `MockRunner` 做单元测试（无需网络）。单独存在 `reason()` 方法正是为了让实时 runner 在 classify/plan/decompose/verify 节点上不挂载任何工具——以防一个有能力的模型在本应是判断/规划的步骤中就开始去*实现*任务（这在 runner.rs 中得到确认：`run_in(..., with_tools=false)` 构建的是 `ToolRegistry::empty`）。在廉价模型上做 best-of-N，是作为一个比单次昂贵运行更便宜的降方差杠杆而被选中的。带反馈的闭环 verify→retry 循环由 `max_verify_retries` 约束以控制成本；裁决解析有意做得宽松（子串匹配 FAIL、宽松的 SUBTASK/列表解析、clamp 处理的 BEST），因为 LLM 的输出格式并不可靠，不应因一次格式上的失误就丢掉一个可用的结果。`max_subtasks` 防范模型过度拆分（每个子任务本身就是一条完整的 pipeline）。递归深度被封顶（`max_depth`，默认 1），使那些自身又被重新分类为 high 的 high 子任务不会无限地分解下去。

**关键机制**：
- **AgentRunner trait (?Send, async_trait)** — 4 个方法——run/reason/run_worker/promote_worker——抽象了所有模型调用，使节点图与 provider 无关且可测试。reason/run_worker/promote_worker 都有默认实现（委托给 run / 忽略隔离 / 空操作），因此简单的 mock 或只读 runner 只需实现 run。`?Send` 使其保持 current-thread。
- **Tool-less reasoning nodes (reason()) (无工具推理节点)** — classify/plan/decompose/verify 都走 reason()，LiveRunner 重写它以构建 `ToolRegistry::empty(ctx)`（with_tools=false）。一个有能力的模型在判断步骤中字面上没有任何工具可调用，所以它无法在 classify 或 verify 期间就开始执行任务。
- **Best-of-N parallel workers + verifier selection (best-of-N 并行 worker + verifier 选优)** — run_attempts 通过 `futures_util::join_all` 派生 N 个 worker future；每个都是一次隔离的尝试，因此并行执行不会破坏共享状态。VERIFY_SYS 要求给出 PASS/FAIL 以及一行 'BEST:<n>'；parse_best_worker 将其转为 0-based 索引，且只有这个胜者被提升。
- **Closed verify→retry loop (闭环 verify→retry)** — 在 FAIL 且仍在 max_verify_retries 之内时，完整的裁决文本通过 build_worker_task 作为 `feedback` 带入下一轮（'The previous attempt was rejected. Address this feedback:'）。verdict_passed 是 fail-loud 的：除非裁决中包含 'FAIL' 否则一律判通过。
- **Recursive decomposition with sequential promotion (带顺序提升的递归分解)** — 拥有 ≥2 个子任务的 High 任务通过 handle_at(st, depth+1) 依次（SEQUENTIALLY）递归；每个子任务先把自己由 verifier 选出的胜者提升到真实工作区，再运行下一个，因此各子任务能在已提交的工作之上构建，而不会发生并行写冲突。深度由 max_depth 约束，数量由 max_subtasks 约束。
- **Workspace isolation + promotion (LiveRunner) (工作区隔离 + 提升)** — run_worker 通过 copy_tree 把工作区复制到每个 worker 专属的临时 scratch 目录；promote_worker 把胜者的 scratch 复制回真实工作区，然后删除所有 scratch 副本。copy_tree 失败时回退到在真实工作区中运行（无隔离）。因此递归在实时环境下是安全的。
- **Lenient parsing throughout (全程宽松解析)** — parse_complexity（子串匹配，默认 Medium）、parse_subtasks（先识别 SUBTASK: 前缀，再通过 strip_list_marker 回退到带编号/项目符号的列表）、parse_best_worker（做 clamp，默认 0）——全都容忍不可靠的 LLM 格式，以免丢掉一个可用的结果。
- **Tier mapping (LiveRunner.model_for) (档位映射)** — Tier::Main → cfg.model（pro）；Tier::Fast → cfg.fast_model（flash），当 fast_model 为空时回退到 cfg.model。每个节点都构建一个全新的 DeepSeekProvider + Session + 工具注册表（一次性回合）。

**控制 / 数据流**：
1. handle(task) → handle_at(task, depth=0)，为支持自递归用 LocalBoxFuture 装箱。
2. classify：reason(Fast, CLASSIFY_SYS) → parse_complexity → Simple | Medium | High（不明确→Medium）。
3. Simple：单次 run(Fast, WORKER_SYS)；verify_passed=true，verify_rounds=0，无 plan。
4. Medium：pipeline = reason(Main, PLAN_SYS)，然后 run_attempts(verify_tier=Fast, n=cfg.workers=2)。
5. High 且 depth<max_depth：decompose_and_recurse；否则（深度耗尽 / max_depth=0）：run_attempts(verify_tier=Main, n=high_workers=3)。
6. run_attempts 循环：通过 join_all(run_worker(i,n)) 派生 N 个 worker，每个都喂入 plan + 上一轮反馈（build_worker_task）。
7. verify：reason(verify_tier, VERIFY_SYS, build_verify_task) → verdict_passed（除非出现 'FAIL' 否则为 true）。
8. 若通过 或 rounds>max_verify_retries：parse_best_worker(BEST:<n>)，promote_worker(best)，synthesize → OrchestratorOutcome；否则 feedback=裁决文本并继续循环。
9. decompose_and_recurse：plan(Main) → reason(Main, DECOMPOSE_SYS) → parse_subtasks。
10. <2 个子任务 → 复用该 plan 在 Main 上走原子 best-of-N 回退；>max_subtasks → 截断。
11. 对每个子任务依次：handle_at(st, depth+1)；对所有 verify_passed 做 AND；收集 '[subtask]...' 结果；每个子任务先提升自己的胜者。
12. 最后：对拼接后的子结果做一次 reason(Main, VERIFY_SYS)；verify_passed = all_passed && verdict_passed；synthesize_subtasks → OrchestratorOutcome。

**面试话术点 / 候选人应能说出**：
- 核心论点（来自模块文档）：能力瓶颈是模型而非 harness，因此 orchestrator 用廉价模型省下的成本去换取结构（并行、verify、分解）来提升可靠性——并坦承它无法超越 Main 模型的推理上限，因为 plan+verify 都跑在那里。
- 努力程度按风险伸缩：Simple 同时跳过 plan 和 verify（一次 fast 运行）；Medium 在 Fast 上 verify；High 在 Main 上 verify 并获得更多尝试（high_workers=3 对比 workers=2）。这是编码在 OrchestratorConfig 中的一个成本/可靠性旋钮。
- 为什么要有单独的 reason() 方法而不是只用 run()：无工具节点。如果没有它，一个被交给 classify 提示词外加工具的有能力模型可能就直接开始编辑文件了。LiveRunner 用 ToolRegistry::empty 来强制执行这一点——干净地把'判断/规划'与'执行'分离开。
- best-of-N + verifier 挑选 BEST 是对廉价模型的降方差：比单次昂贵运行更便宜，而且 verifier 既做门禁（PASS/FAIL）又做选优（BEST:<n>）；只有胜者那份隔离的工作区被提升。
- 闭环 verify→retry 循环把 verifier 的不满反馈进下一批 worker 的提示词中（由 max_verify_retries 约束）——这是一个自我纠正的循环，而非盲目重采样。
- 递归安全性是微妙之处：子任务依次（SEQUENTIALLY）运行，每个在下一个开始前把胜者提升到真实工作区，使它们在已提交的工作之上构建；而一次尝试内的并行 worker 则隔离在临时副本中（copy_tree）以避免写冲突。装箱的 LocalBoxFuture 是必需的，因为一个 async fn 不借助间接层就无法递归。
- 随处可见的防御式 LLM 输出解析：parse_complexity 在不明确时默认→Medium，verdict_passed 是 fail-loud，parse_subtasks 从 SUBTASK: 回退到带编号/项目符号的列表，parse_best_worker 做 clamp 并默认为 0——该设计假定模型有时会无视格式要求。
- 可测试性：整个图都用一个脚本化的 MockRunner（无网络）做单元测试，断言精确的 (tier, stage) 调用计数——例如 medium = 1 次 main plan + 2 个 fast worker + 1 次 fast verify；各项上限（max_depth, max_subtasks）都各有一个专门的测试。

**取舍与坑（适合做故障题/追问）**：
- 文档中承认的硬性上限：plan 和 verify 都跑在 Main 上，所以 orchestrator 无法在推理上胜过 Main 模型——它只能提升完成率/可靠性以及分解所能触及的范围，而非原始能力。
- 成本可能迅速翻倍：一个 high 任务可能扇出为（子任务数 × 每个子任务的 pipeline × worker 数 × 重试轮数）次模型调用，外加额外的 plan/decompose/verify 推理调用。max_subtasks 和 max_depth 的存在正是为了约束这一点。
- verdict_passed 使用朴素的子串检查——任何包含字面 'FAIL' 的 verifier 回复（即便是 'no FAILures'）都会被当作失败；反之，一个遗漏了 FAIL 的格式错误 verify 反而会通过。它用稳健性换取了简单性。
- parse_complexity 通过子串先检查 'high' 再检查 'simple'/'medium'；一个同时提到多个词的啰嗦分类器回复可能导致路由错误，而任何无法识别的回复都会被静默归为 Medium。
- 工作区隔离依赖于为每个 worker 每一轮 copy_tree 整棵树——对于大型仓库可能开销很大；在复制失败时它会静默回退到真实工作区，从而重新引入了它本应防止的并行写冲突。
- 分解路径的验证只在拼接后的子结果上做一次最终 Main verify（verify_rounds 硬编码为 1，该层级没有重试循环），且 best_worker 被强制为 0——逐子任务的自我纠正只发生在每次递归的 handle_at 内部，而不在汇合处。
- 默认 max_depth=1 意味着那些自身又被分类为 High 的子任务不会再次分解（它们会走 best-of-N）——这是 depth-cap 测试有意为之的，但也限制了真正的深度递归。

**代码引用**：`D:/agent_prac/ncx-train/rust/crates/ncx-core/src/orchestrator.rs:53-78 (AgentRunner trait: run/reason/run_worker/promote_worker + defaults)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/orchestrator.rs:81-99 (node system prompts: CLASSIFY/PLAN/DECOMPOSE/WORKER/VERIFY_SYS)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/orchestrator.rs:101-129 (OrchestratorConfig + Default: workers=2, high_workers=3, max_verify_retries=1, max_depth=1, max_subtasks=6)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/orchestrator.rs:165-198 (handle_at recursive core, LocalBoxFuture/boxed_local, Simple/Medium/High routing)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/orchestrator.rs:207-275 (pipeline + run_attempts closed best-of-N/verify/retry loop, promote_worker, synthesize)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/orchestrator.rs:282-352 (decompose_and_recurse: atomic fallback, max_subtasks truncate, sequential handle_at per subtask, final join verify)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/orchestrator.rs:364-502 (parsers: parse_complexity, verdict_passed, parse_subtasks/strip_list_marker, parse_best_worker, build_* + synthesize helpers)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/orchestrator.rs:506-822 (MockRunner + tests asserting (tier,stage) call counts for every path)` · `D:/agent_prac/ncx-train/rust/crates/ncx-cli/src/runner.rs:48-152 (LiveRunner: model_for tier mapping, run_in with_tools flag → ToolRegistry::empty for reason, run_worker copy_tree isolation, promote_worker sync-back+cleanup)` · `D:/agent_prac/ncx-train/rust/crates/ncx-cli/src/main.rs:239-248 (run_orchestrated wiring: LiveRunner + Orchestrator::new + OrchestratorConfig::default + handle)`

### 6. 项目记忆 · 自进化

**一句话**：一个极小的、以 markdown 为后端的“自我进化”存储（.ncx/memory/LEARNINGS.md），让 agent 能够积累经过验证的项目笔记，并把最相关的若干条作为线索（而非事实）召回进系统提示词；它使用一个轻量的词法-语义排序器，外加启发式与 LLM 支持的近重复合并。

**工作原理**：`MemoryStore`（memory.rs）封装了单一路径 `.ncx/memory/LEARNINGS.md`，并持久化 `MemoryEntry { ts: u64, tags: Vec<String>, text: String }` 记录。每条记录序列化为一个可解析的 HTML 注释头 `<!-- ts:<n> tags:<a,b> -->`，后接笔记正文；`parse_entries`/`write_all` 对此进行往返读写，使文件保持人类可读、可手工编辑。`remember(text, tags, now)` 会修剪文本、对其归一化（通过 `normalize` 折叠空白并转小写），如果任何已有记录归一化后完全相同则跳过写入（精确文本去重返回 `Ok(false)`）；否则追加该记录，按 ts 升序排序，并丢弃超出 `MAX_ENTRIES = 200` 之外最旧的记录。`recall(query, max_entries, max_chars)` 用 `semantic_score` 为每条记录打分，返回一个受上限约束的项目符号块，前面冠以 `RECALL_HEADER`，它明确告诉模型把这些笔记当作需要验证的线索、而非事实。打分是一个混合词法-语义之和：查询关键词（转小写、长度 >= 3、由 `keywords` 去重，再通过 `semantic_aliases` 中一张硬编码的领域同义词映射经 `expanded_keywords` 扩展），若命中标签则 +8，若出现在记录文本/标签的检索串中则 +4；相邻词构成的 `phrases`（二元组）在子串命中时加 +6；两个词集合的 Jaccard 重叠最多加 +20（`jaccard * 20`）。近期性被打包进低 6 位数字（`overlap * 1_000_000 + ts.min(999_999)`），使得 ts 越高在打平时越优先，然后逐条输出直到触及 `max_entries` 或 `max_chars`。`consolidate(threshold)` 是会话开始时运行的一次廉价启发式去重：它按从新到旧排序，以 `jaccard(word_set) >= threshold` 做单链接聚类，保留每个簇中最新的代表项，重新限制上限，且是幂等的。`summarize_consolidate(summarizer, threshold)` 做同样的贪心聚类，但对任何多条记录的簇，会请求注入的 `Summarizer` trait（CLI 中基于快速模型的 `LiveSummarizer`）把这些笔记折叠成一条简洁笔记，保留该簇最新的 ts 以及其标签的并集；如果 LLM 返回 `None`/空值，则回退为保留最新的记录，与启发式做法一致。

**设计理由（为什么）**：核心立场（memory.rs 模块文档）是：记忆并非“提高了智商”，而是把积累且经过验证的经验作为线索召回——因此 `RECALL_HEADER` 的措辞刻意持不信任态度，而 `remember` 工具描述坚持只存储经过 CONFIRMED（确认）的事实，从而让存储保持可信。选择带注释头格式的纯 markdown 而非数据库/嵌入索引，是为了让存储人类可读、可手工编辑、可被 git diff、且无依赖——这与项目“小巧、不引入重量级依赖”的理念一致；其代价是检索是词法的，而非向量语义的。为了在没有嵌入模型的情况下找回一些语义触达能力，作者额外加上了一张手工策划的同义词映射（`semantic_aliases`，例如 desktop/native/window -> gui,tauri）以及短语/Jaccard 信号——这是一个务实的折中，并由测试 `recall_uses_semantic_aliases_and_tags` 加以锁定（查询 "native installer release package" 会浮现出一条关于 Tauri/bundle 的笔记）。近期性是通过位打包折叠进整数分值的，而非作为第二排序键，从而给出一个稳定、确定性的单一比较器。`now` 由调用方传入、而非在存储内部读取时钟，使写入具有确定性、可被单元测试。`Summarizer` trait 是 `#[async_trait(?Send)]` 且通过依赖注入提供，使核心 crate 保持与提供方无关、并对单线程友好（整个 agent 是 `!Send`/基于 `Rc` 的），真正的 LLM 调用位于 CLI 的 `LiveSummarizer`（快速模型）中，而测试使用一个 `FixedMerger` mock。之所以存在两条合并路径，是因为启发式既免费又能安全地在每次启动时运行，而 LLM 合并质量更好但要付出一次调用的代价——所以 `consolidate` 在会话开始时无条件运行（runner.rs / main.rs 使用 0.85），而 `summarize_consolidate` 是一个显式选用的子命令。硬性的 200 条上限加上按字符预算的召回（例如 runner 中的 6 条/3000 字符）之所以存在，是因为召回的笔记会被前置到系统提示词中，无限增长会撑爆上下文窗口。

**关键机制**：
- **Markdown comment-header store format (markdown 注释头存储格式)** — 每条记录是 `<!-- ts:<epoch> tags:<csv> -->` + 正文，由 `write_all` 写入、由 `parse_entries` 读取；文件为 `.ncx/memory/LEARNINGS.md`。选择它是为了人类可读/可被 git diff，而非数据库或向量索引（零依赖，但只能做词法检索）。
- **Exact-text dedup on remember (remember 上的精确文本去重)** — `remember` 将 `normalize(text)`（折叠空白、转小写）与已有记录比较；归一化后相同则返回 Ok(false)。测试 `dedup_skips_identical` 确认 '  SAME   fact ' 会与 'same fact' 去重。
- **MAX_ENTRIES cap (200), oldest-dropped (MAX_ENTRIES 上限 200，丢弃最旧项)** — 追加之后，记录按 ts 升序排序，`drain(0..drop)` 移除最前端（最旧）的项。`cap_drops_oldest` 验证 len==200 且所有保留项的 ts>=5。可防止提示词无限增长。
- **Hybrid lexical-semantic recall score (混合词法-语义召回评分)** — `semantic_score`：标签命中 +8，每个（经别名扩展的）关键词文本子串命中 +4，二元组短语子串命中 +6，再加上 Jaccard(查询,记录的词集合)*20。`expanded_keywords` 通过 `semantic_aliases` 硬编码同义词映射进行扩展。
- **Recency bit-packing tie-break (近期性位打包打平)** — `s = overlap * 1_000_000 + (e.ts.min(999_999))`，然后降序排序——一个整数比较器，其中 overlap 占主导地位、近期性用于打平，无需第二排序键。
- **Leads-not-facts framing (“线索而非事实”定位)** — RECALL_HEADER 字面指示 'treat as leads, verify before acting'（当作线索，行动前先验证）；`remember` 工具描述（tools.rs）将写入限定为仅 CONFIRMED（确认）的笔记。召回块由 `compose_system_prompt` 追加到系统提示词中。
- **Heuristic consolidate (Jaccard single-link) (启发式合并，Jaccard 单链接)** — 从新到旧的贪心聚类；如果某条记录与任一已保留代表项的 `jaccard(word_set) >= threshold` 则视为重复。保留每个簇中最新的项，重新限制上限，幂等（重跑移除 0 条）。在每次会话开始时以 0.85 运行。
- **LLM summarize_consolidate with fallback (带回退的 LLM summarize_consolidate)** — 同样的聚类；对大小 >1 的簇，调用注入的 `Summarizer::merge` 将其折叠成一条笔记（最新 ts + 标签并集）；在 None/空值时回退为保留最新项。`LiveSummarizer` 使用快速模型，提示词为 'merge into ONE concise note, <=2 sentences'（合并为一条简洁笔记，不超过 2 句）。
- **Injected Summarizer trait (注入的 Summarizer trait)** — `#[async_trait(?Send)] trait Summarizer { async fn merge(&self,&[String]) -> Option<String> }` 使 ncx-core 保持与提供方无关；CLI 提供 `LiveSummarizer`（DeepSeekProvider，fast_model），测试使用 `FixedMerger`。

**控制 / 数据流**：
1. WRITE（写入）：模型调用 `remember` 工具（tools.rs 中的 RememberTool），传入一条已确认的笔记 + 可选标签；该工具读取 SystemTime 作为 `now` 并调用 `MemoryStore::remember`。
2. remember 修剪/归一化文本，遇到空文本或归一化后的精确重复则返回 Ok(false)，否则压入一个 MemoryEntry，按 ts 排序，限制到 MAX_ENTRIES（丢弃最旧项），并由 `write_all` 重新序列化整个 markdown 文件。
3. SESSION START（会话开始）：CLI/GUI 构造 `MemoryStore::new(workspace/.ncx/memory)` 并调用 `consolidate(0.85)`（幂等的启发式近重复合并）以廉价地整理存储。
4. RECALL（召回）：调用方调用 `recall(query, max_entries, max_chars)`——runner 使用 (6,3000)，CLI 使用 (8,4000)，GUI 在会话开始尚无任务时使用 ("",8,4000) 以纯近期性召回。
5. recall 计算 expanded_keywords（含同义词别名）、查询词集合，以及二元组短语，然后对每条记录用 `semantic_score` 打分（标签 +8 / 文本 +4 / 短语 +6 / jaccard*20）。
6. 分值与近期性做位打包、降序排序，并在 RECALL_HEADER 下以 `- <note>` 项目符号形式输出，直到触及 max_entries 或 max_chars（若没有任何条目得分/存储则为空字符串）。
7. INJECT（注入）：`compose_system_prompt` 把召回块（与项目说明、技能索引并列）追加到系统提示词，使这些笔记作为该回合的线索一同带入。
8. OPTIONAL DEEP CONSOLIDATION（可选的深度合并）：`summarize-memory` 子命令构建一个 `LiveSummarizer`（快速模型）并调用 `summarize_consolidate(&summarizer, 0.85)`，对近重复聚类，并把每个多笔记的簇用 LLM 合并成一条（最新 ts + 标签并集），合并失败时回退为最新项。

**面试话术点 / 候选人应能说出**：
- 定位很重要：这明确是“积累且经过验证的经验、作为线索召回”，而非把知识库当作绝对真理——这一点在写入时（remember 工具：仅 CONFIRMED）和读取时（RECALL_HEADER：'treat as leads, verify before acting'）都被强制执行。
- 刻意做到无依赖：用一个可手工编辑、带注释头的单一 markdown 文件，而非 SQLite/嵌入——契合“小巧”的理念，且可被 git diff，代价是只能做词法检索。
- 他们如何在没有嵌入的情况下伪造语义召回：一张手工策划的 `semantic_aliases` 同义词映射 + 二元组短语 + Jaccard 词集合重叠，合并进一个加性的 `semantic_score`。
- 单一整数评分比较器：`overlap * 1_000_000 + ts.min(999_999)` 把相关性与近期性打包，使一次降序排序就能得到相关性优先、近期性打平的确定性排序。
- 通过注入 `now` 实现可测试性：存储本身从不读时钟，所以去重/上限/召回测试是完全确定性的；调用方（RememberTool）提供 SystemTime。
- 两级合并作为成本/质量权衡：免费的启发式 Jaccard 去重在每次会话开始时运行（幂等），而 LLM 合并是显式、付费的选用项，能产出更漂亮的融合笔记。
- 通过 `?Send` 的 `Summarizer` trait 实现与提供方无关的核心——ncx-core 从不导入模型客户端；CLI 的 `LiveSummarizer` 接入快速模型，测试使用 mock。这与项目的 `!Send`/Rc 单线程设计一致。
- 有界的上下文安全：硬性 200 条上限 + 按字符预算的召回块，因为召回每回合都会被前置到系统提示词中。

**取舍与坑（适合做故障题/追问）**：
- remember 上的去重仅为精确文本（归一化之后）；措辞不同的近重复会蒙混过关，直到一次 `consolidate`/`summarize_consolidate` 处理——去重与合并刻意采用两种不同的相似度门槛。
- `semantic_aliases` 是一张很小的、硬编码的、领域专属映射（gui/tauri/build/rust 等）；召回语义无法泛化到本项目词汇之外，必须手工扩展。
- 近期性打包将 ts 贡献封顶在 999_999（`ts.min(999_999)`），而 overlap 上 >=1 的任何分值差异（×1_000_000）会完全压倒 ts——因此近期性只会在相关性完全打平时起到打破作用，绝不会凌驾于相关性之上。
- `word_set`/`keywords` 会丢弃短于 3 个字符的 token，所以诸如 'go'、'ci'、'os' 这类简短但有意义的 token 永远不会对 Jaccard 或关键词匹配做出贡献。
- 每次成功的 remember/consolidate 都会整文件重写（`write_all` 序列化所有记录）——在 200 条上限规模下尚可，但每次写入是 O(n)，并非追加式，尽管注释里写着“近似追加式”。
- `recall` 评分用 `hay.contains(w)` 对检索串做关键词子串匹配，所以它可能命中更大单词的内部（子串、而非词边界）——这是可能产生虚假 +4 命中的来源。
- `consolidate` 返回被移除的数量、且仅在移除数>0 时才写入，使其廉价且幂等，但它会默默保留近重复簇中最新的项，可能丢弃一条措辞更好的旧笔记而保留一条更草率的近期笔记（LLM 合并正是为缓解这一点而存在）。

**代码引用**：`D:/agent_prac/ncx-train/rust/crates/ncx-core/src/memory.rs:25 (Summarizer trait, ?Send)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/memory.rs:31 (MAX_ENTRIES=200)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/memory.rs:32 (RECALL_HEADER 'treat as leads')` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/memory.rs:60 (remember: dedup + cap)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/memory.rs:96 (recall: score + bit-pack + char/entry cap)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/memory.rs:109 (overlap*1_000_000 + ts recency packing)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/memory.rs:141 (consolidate: heuristic Jaccard near-dup, idempotent)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/memory.rs:179 (summarize_consolidate: LLM merge + newest-ts/union-tags + fallback)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/memory.rs:295 (semantic_aliases synonym map)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/memory.rs:311 (semantic_score: tag+8/text+4/phrase+6/jaccard*20)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/memory.rs:372 (jaccard)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/memory.rs:385 (parse_entries comment-header parser)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:1128 (RememberTool — CONFIRMED-only writes, SystemTime now)` · `D:/agent_prac/ncx-train/rust/crates/ncx-cli/src/runner.rs:100 (recall(task,6,3000) injected into prompt)` · `D:/agent_prac/ncx-train/rust/crates/ncx-cli/src/runner.rs:206 (LiveSummarizer — fast model, merge prompt)` · `D:/agent_prac/ncx-train/rust/crates/ncx-cli/src/main.rs:114 (summarize_consolidate at 0.85 subcommand; consolidate(0.85) at startup)` · `D:/agent_prac/ncx-train/rust/gui/src-tauri/src/bridge.rs:176 (GUI recall("",8,4000) — recency-only at session start)`

### 7. Skills · MCP · 视觉

**一句话**：两级渐进式披露的 Agent Skills（`SKILL.md` 索引始终常驻，完整正文通过 `skill` 工具按需获取）、被包装为一等 ncx 工具并共享同一个 `Rc<Mutex<McpClient>>` 的 MCP 服务器工具（带启发式的只读/审批门控），以及按轮次进行的视觉路由——把携带图像的轮次转发到一个可选的专用视觉 `Provider`。

**工作原理**：SKILLS（技能）：一个 `Skill`（`skills.rs`）是一个目录，里面有一份带 `---` 围栏的 YAML frontmatter（`name:`、`description:`）的 `SKILL.md`。`discover_skills` -> `discover_skills_with_home` 先用 `builtin_skills()`（通过 `include_str!` 在编译期内嵌，目前仅有 `commit-message`）为一个 `BTreeMap<String,Skill>` 播种，然后按顺序叠加两个文件系统根——先是 `~/.ncx/skills/*`，再是 `<workspace>/.ncx/skills/*`——这样后加入的同名技能会遮蔽先加入的（workspace 胜过 home，home 胜过 builtin）。`parse_frontmatter`/`frontmatter_lines`/`strip_frontmatter` 只手工解析所需的两个键（不引入 YAML 库）；`name` 缺省时回退到目录名。一级披露：`skills_index_block` 仅在固定标题下向系统提示中写入 `- name: description` 行（在 `main.rs` 中组装为 `skills_index`，即第 3 个额外块）。二级披露：`SkillTool`（`tools.rs`，标记 `read_only`）仅在 `ctx.skills` 非空时才注册；被调用时它以大小写不敏感的方式查找该技能并返回 `Skill::load_body()`——内置技能返回内嵌字符串，否则用 `std::fs::read_to_string` + `strip_frontmatter`——并以该技能的目录作为前缀，以便模型能 `read_file` 读取随包资源（三级披露）。MCP：`register_mcp_server`（`mcp_tool.rs`）通过 stdio 执行 `McpClient::connect`、`list_tools`，然后把这个唯一的客户端包进一个 `Rc<Mutex<McpClient>>`，并为每个 `McpToolDef` 注册一个 `McpTool`，它们全都共享那个句柄。`McpTool` 实现了 `Tool`（`?Send`），转发 name/description/input_schema，并通过 `is_read_only_name`（前缀/精确匹配的启发式：read_/get_/list_/fetch_/search_/find_）一次性计算出 `read_only`。在 `execute` 时，非只读工具先运行 `Approver::new(&ctx.approval_policy).classify(name, true)`；`AutoDeny` 返回一个错误字符串，`Ask` 以 `escalated:true` 调用 `ctx.approver.request(...)`（被拒绝则返回错误，缺少 approver 则放行），`AutoApprove` 则继续执行；随后 `client.lock().await` 对 `call_tool` 这一 RPC 进行串行化。VISION（视觉）：每次 `run_turn_inner` 都设置 `use_vision_this_turn = vision_provider.is_some() && has_image_block(user_input)`，其中 `has_image_block` 检查用户内容数组里是否有任何 `type == "image_url"` 的块。`active_provider()` 仅在二者都为真的那一轮返回视觉 provider，否则返回主 provider；`call_model` 在 `active_provider()` 返回的那个 provider 上调用 `.chat`。视觉 provider 是一个可选的 `Box<dyn Provider>`，在 `main.rs` 中由 `build_vision_provider` 构建（一个指向 `vl_model`/`vl_base_url`/`vl_api_key` 的 `DeepSeekProvider`，未设置时回退到主凭据；若 `vl_model` 为空则为 `None`）。

**设计理由（为什么）**：渐进式披露的存在是为了让始终常驻的提示保持低成本：只为每个技能注入 name+description（这些技能可能随附庞大的正文和辅助文件），从而避免为当前任务用不到的指令付出 token 代价；文档注释明确把这描述为与 `tool_search` 相同的两级目录形态。通过 `include_str!` 提供内置技能，保证了一套零文件系统依赖的基线技能集（随单一二进制一起发布），而文件系统遮蔽则让用户无需重新编译即可覆盖内置技能——以 `BTreeMap` 插入顺序进行的叠加是表达这种优先级最廉价的方式，同时还能产出按名称排序的确定性输出。手写的 frontmatter 解析器是一个刻意的依赖最小化选择：只有两个标量键有意义，所以引入一个 YAML crate 就过度了。对于 MCP，一台服务器的所有工具共享一个 `Rc<Mutex<McpClient>>` 是被架构强制的：该客户端拥有一个服务器进程和一条 stdio JSON-RPC 管道，因此并发调用必须被串行化——这个 Mutex 让交错的请求/响应分帧不可能被破坏，而用 `Rc`（而非 `Arc`）是正确的，因为整个 agent 运行在一个当前线程、`!Send` 的运行时上（注意 `#[async_trait(?Send)]`）。复用 `ctx.approver`/`Approver`/`Decision` 是刻意为之，让有副作用的 MCP 工具拥有与 `ShellTool` 完全相同的升级模型，而不是另搞一套并行路径；这个基于名称前缀的只读启发式是一个公认的近似（测试注释甚至点出了 `echo` 是误分类的边界情况），之所以采用是因为 MCP 协议并不暴露任何机器可读的副作用标志。视觉路由是按轮次且需显式开启的：与其搞一条独立的多模态流水线、或强迫一个模型同时既是程序员又具备视觉能力，不如让携带图像的轮次透明地转去一个专用 provider，而文本轮次继续留在主编码模型上；当没有配置视觉 provider 时，图像轮次就直接留在主 provider 上（优雅地变成无操作）。检测以 OpenAI 风格的 `image_url` 块为依据，这样同一个 `content` 数组对两个 provider 都适用。

**关键机制**：
- **Two-tier progressive disclosure（两级渐进式披露）** — `skills_index_block` 只注入 `- name: description` 行（一级）；`SkillTool.execute` 按需返回完整的 `SKILL.md` 正文 + 目录（二级），从而启用对随包资源的 `read_file`（三级）。`SkillTool` 仅在 `ctx.skills` 非空时才注册（`tools.rs:213-216`）。
- **Builtin + shadowing precedence（内置 + 遮蔽优先级）** — 通过 `include_str!` 的 `builtin_skills()` 为一个 `BTreeMap` 播种；`~/.ncx/skills` 再到 `<workspace>/.ncx/skills` 在其上叠加（workspace > home > builtin）。`Skill.is_builtin() == embedded.is_some()`；`load_body()` 读取内嵌字符串或文件系统内容（`skills.rs:69-120`）。
- **Shared serialised MCP client（共享且串行化的 MCP 客户端）** — `register_mcp_server` 把一个 `McpClient` 包进 `Rc<Mutex<McpClient>>`；该服务器的每个 `McpTool` 都克隆这个 `Rc`，`execute` 在 `call_tool` 之前执行 `client.lock().await`，从而在当前线程运行时上对 stdio JSON-RPC 进行串行化（`mcp_tool.rs:106,128-131`）。
- **Heuristic read-only + approval reuse（启发式只读 + 复用审批）** — `is_read_only_name` 匹配 read_/get_/list_/fetch_/search_/find_ 前缀（以及精确词）。非只读工具运行 `Approver::classify(name,true)`：`AutoDeny`->错误，`Ask`->`ctx.approver.request(escalated:true)`，`AutoApprove`->继续——与 `ShellTool` 完全一致（`mcp_tool.rs:38-46,66-104`）。
- **Per-turn vision routing（按轮次的视觉路由）** — `run_turn_inner` 设置 `use_vision_this_turn = vision_provider.is_some() && has_image_block(input)`；`active_provider()` 仅在那一轮换入视觉 `Provider`。`has_image_block` 扫描内容数组寻找 `type=="image_url"`（`agent_loop.rs:173-183,247,573-582`）。

**控制 / 数据流**：
1. 启动（`main.rs`）：`discover_skills(workspace)` 合并内置技能 + `~/.ncx` + `<workspace>/.ncx`；`skills_index_block` -> `skills_index` 折叠进系统提示；`ToolContext.with_skills(skills)`，于是 `SkillTool` 在非空时被注册。
2. 启动 MCP：对每个配置的服务器，`register_mcp_server` 通过 stdio 连接、`list_tools`、把客户端包进 `Rc<Mutex<...>>`，并为每个 def 向 `ToolRegistry` 注册一个 `McpTool`。
3. 启动视觉：`build_vision_provider` 返回 `Some(DeepSeekProvider on vl_model)` 或 `None`；`AgentLoop.with_vision_provider` 将其存储。
4. `run_turn_inner`：计算 `use_vision_this_turn = vision_provider.is_some() && has_image_block(user_input)`。
5. 模型调用：当且仅当 `use_vision_this_turn` 为真时 `active_provider()` 返回视觉 provider，否则返回主 provider；`call_model` 在其上调用 `.chat`。
6. 模型读取一级技能索引，判断某个任务匹配，并以确切的名称调用 `skill` 工具。
7. `SkillTool.execute`：在 `ctx.skills` 中大小写不敏感地查找；命中则通过 `load_body()`（内嵌或文件系统读取 + `strip_frontmatter`）返回 'Skill <name> (...): <body>'；未命中则返回一个列出可用技能的错误。
8. 模型可选地从返回的技能目录中 `read_file` 读取随包资源文件（文件系统技能）——三级披露。
9. 若模型发出一个 MCP 工具调用：`McpTool.execute` 检查 `read_only`；非只读则运行 `Approver.classify` -> 可能调用 `ctx.approver.request`；获批后 `client.lock().await` 再 `call_tool`，返回结果或一个 'Error:' 字符串。

**面试话术点 / 候选人应能说出**：
- 渐进式披露是核心看点：只有 name+description 始终常驻；重量级的 `SKILL.md` 正文由 `skill` 工具懒加载。明确仿照了与 `tool_search` 相同的两级目录，以保持提示 token 的低成本。
- 三个披露层级：(1) 系统提示中的索引，(2) 通过 `SkillTool` 获取的完整正文，(3) 模型用 `read_file` 从返回的技能目录中读取的随包资源文件。
- 内置技能用 `include_str!` 编译进来（零文件系统依赖，随单一二进制发布），但同名的文件系统技能会遮蔽它们——通过有序的 `BTreeMap` 插入实现 workspace > home > builtin；同时还产出按名称排序的确定性输出。
- frontmatter 仅手工解析 name+description（不用 YAML crate）——刻意的依赖最小化；`name` 回退到目录名，格式错误/无法读取的技能被静默跳过。
- 一个 `Rc<Mutex<McpClient>>` 被某服务器的所有工具共享：这是被强制的，因为该客户端拥有单一的服务器进程 + stdio JSON-RPC 管道，所以并发调用必须串行化。用 `Rc` 而非 `Arc` 是因为运行时是当前线程 / `!Send`（`async_trait(?Send)`）。
- MCP 审批复用了完全相同的 `ShellTool` 路径（`Approver`/`Decision`/`ctx.approver`）——`AutoDeny` / `Ask`（`escalated:true`）/ `AutoApprove`——而非另搞一套并行机制。
- MCP 的 `read_only` 是一个名称前缀启发式（read_/get_/list_/fetch_/search_/find_），一个明确的近似，因为 MCP 不公布任何副作用标志；测试甚至记录了 `echo` 这个边界情况。
- 视觉路由是按轮次且需显式开启的：图像轮次（内容数组中任意 `image_url` 块）被转去一个专用视觉 `Provider`（一个跑在 `vl_model` 上的 `DeepSeekProvider`）；文本轮次留在主编码模型上，而缺失视觉 provider 时会优雅降级到主 provider。

**取舍与坑（适合做故障题/追问）**：
- `read_only` 是一个无协议支撑的名称前缀启发式：一个名为 `search_and_replace` 之类的写工具会被自动批准，而测试注释本身就点出 `echo` 实际并不匹配（它没有读取前缀）——所以仅凭命名就决定了一个有副作用的 MCP 工具是否被门控。
- 当 `Decision::Ask` 但没有配置 approver 时，`McpTool` 会放行并照样调用工具（与 `ShellTool` 的自动批准行为一致）——在无头上下文中，非只读的 MCP 调用可能未经提示就运行。
- frontmatter 解析要求开头的 `---` 必须是文件的第一行且有一个匹配的闭合围栏；否则整个文件被当作正文，`name` 回退到目录名——这是静默的而非报错，且一个缺少 name 的内置技能会被跳过（隐藏了打包 bug）。
- 视觉检测只识别顶层 `content` 数组中 OpenAI 风格的 `image_url` 块；纯字符串提示或形状不同的图像块都不会被路由。检测是按轮次重新计算的，所以紧跟在图像轮次之后的一个工具/结果轮次不会继续留在视觉模型上。
- 视觉 provider 在 `build_vision_provider` 中被硬编码为 `DeepSeekProvider`；把整轮（而不仅是图像）路由给它，意味着视觉模型也要处理那一轮的任何工具调用。
- 某服务器的所有 MCP 工具都通过同一个 Mutex 串行化，因此一个缓慢的 MCP 调用会阻塞来自同一服务器的同级工具（在单线程运行时上可以接受，但没有按工具的并发）。

**代码引用**：`D:/agent_prac/ncx-train/rust/crates/ncx-core/src/skills.rs:52 (Skill::load_body)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/skills.rs:69 (builtin_skills via include_str!)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/skills.rs:101 (discover_skills_with_home + BTreeMap shadowing)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/skills.rs:160 (skills_index_block, level-1)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/skills.rs:177 (parse_frontmatter)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:213 (SkillTool registered only when skills present)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/tools.rs:1184 (SkillTool, level-2 body load)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/mcp_tool.rs:24 (McpTool struct, Rc<Mutex<McpClient>>)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/mcp_tool.rs:38 (is_read_only_name heuristic)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/mcp_tool.rs:66 (execute: approval + client.lock().await)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/mcp_tool.rs:118 (register_mcp_server)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/agent_loop.rs:173 (active_provider vision swap)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/agent_loop.rs:247 (use_vision_this_turn computed per turn)` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/agent_loop.rs:573 (has_image_block)` · `D:/agent_prac/ncx-train/rust/crates/ncx-cli/src/main.rs:148 (discover + index into system prompt)` · `D:/agent_prac/ncx-train/rust/crates/ncx-cli/src/main.rs:1094 (build_vision_provider)`

### 8. ncx-forge · 骨架训练框架

**一句话**：一个零 GPU 的"脚手架优化器"：由强模型组成的 teacher 评审小组通过反思式变异来改写 agent 中可演化的 harness（system prompt + 工具描述 = 所谓的"genome"，通过 `NCX_GENOME` 注入），以隐藏校验的 bench 通过率作为 fitness，并用一个 sentinel 自检 + train/val/test 划分 + 噪声感知的接受判定 + Pareto 小种群搜索来防范主要的失败模式（静默 no-op、过拟合、采样噪声），同时把分级后的轨迹导出，作为通往真正权重训练（SFT/agentic-GRPO）的桥梁。

**工作原理**：可演化的单元是"genome"：仅包含 agent 的基础 `system_prompt` 与每个工具的 `description` 文本——绝不包含工具行为。在 Rust 一侧（`genome.rs`），`Genome::from_env()` 在启动时读取 `NCX_GENOME=<path.toml>`；若未设置/为空/缺失/格式错误/空白，则返回 `Genome::default()`（一个与硬编码脚手架逐字节完全一致的 no-op），而 `base_system_prompt(default)`/`describe(name, default)` 仅在存在覆盖项的位置应用覆盖。在 trainer 一侧（位于 `train/` 的纯 Python，仅通过 subprocess + `NCX_GENOME` 驱动 `ncx.exe`，绝不导入遗留的 `nanocodex/*.py`），基线 genome 通过运行 `ncx --dump-genome`（`genome.py:extract_current`）获得，从而它总是反映真实的工具清单与真实的承载关键作用的描述，而不是解析 Rust 源码得来。`evaluator.py:evaluate` 在临时 workspace 中针对 bench 任务运行 agent，用隐藏的 `check.py` 打分，并且——关键地——在 `grade()` 把 `_check.py` 复制进来之前，从 agent 自身的 `.nanocodex/session.jsonl` 中收割一份经过脱敏的 FAILURE TRAJECTORY（最后一条 assistant 消息 + 工具调用名称），剔除任何包含 grader 标记的行，使 `check.py` 绝不泄漏给 teacher。`forge.py:train` 是单冠军的爬山法：每一轮先在一次全新抽样下重新评估在位者（noise-aware），把排名前 3 的失败轨迹喂给 `teacher.build_teacher_prompt`，让每个可用的 teacher 后端（`teacher.py` 中的 `codex`/`claude`/`api`，每个都经探针门控，且只输出一个由 `parse_candidate` 解析的 TOML 覆盖块）各提出一个变异，在 TRAIN 上评估候选，仅当候选在 TRAIN 上以 `accept_margin` 优于在位者，并且在留出的 VAL 划分上没有回退时才晋升；TEST 在最后只评分一次。`forge.py:evolve`（M2）以一个小型 Pareto 种群（通过 `pareto.py` 的支配关系 + NSGA-II 拥挤度裁剪，目标为通过率↑ vs token 成本↓）取代单冠军。在任何优化运行之前，`self_check` 通过在 `system_prompt` 中植入一个 sentinel 暗号词，并断言它在带 genome 时出现、在基线时不出现，以此对执行进行门控。`export.py` 写出完整的分级轨迹（`ncx-forge-trajectory/v1`），而 `rollout.py`/`finetune.py` 定义了通往权重训练的 agentic-GRPO 桥梁。

**设计理由（为什么）**：作为框架的约束前提是"没有 GPU，模型才是真正的杠杆"：你无法让基础模型变得更聪明，所以框架优化第二个杠杆——harness——使其更可靠地发挥现有能力（DESIGN §1）。这迫使其成为一个纯文本替换面：genome 只能改变描述/prompt，绝不改变行为，因为 (a) sandbox 仍然管控执行，所以一个被喂了不可信失败记录的 teacher 无法注入新能力（这是一个结构性安全边界，DESIGN §9），并且 (b) 这让 no-op 保证可被证明。`genome.rs` 中失败即 no-op 的默认行为承载关键作用：一次对抗式审查发现，若没有它，每个加载失败的候选都会被静默忽略，所有候选会得到相同的分数，整个运行会"全绿"通过却其实在 no-op——因此才有了那个 sentinel 自检门，它在无法证明注入确实生效之前拒绝训练。sentinel 之所以采用确定性方式，恰恰是因为最初的想法（一个"拒绝所有任务"的自毁式 genome）被证明不可靠：强模型会无视拒绝指令仍然把任务做完（合规与否是噪声），所以行为式探针无法区分"注入失效"与"模型不合规"。evaluator 中的轨迹收割之所以存在，是因为 `bench/run.py` 丢弃了 agent 的输出并 rmtree 掉 workspace，使 teacher 只能"从文件名去猜"——P2 是一项硬性前置条件。trainer/trainee 分离（Python trainer 绝不导入 Rust trainee 的遗留 Python 孪生体）使得唯一的 Rust 改动仅限于那一个 `NCX_GENOME` 注入点，把冲突面降到最小。最被公认的最大风险是过拟合/噪声，这决定了以下设计：任务级（而非运行级）的 train/val/test 划分，使某个任务的信号无法跨阶段泄漏；从 teacher 处屏蔽 grader 输出，以防在备注尾部进行 reward-hacking；`repeats` + 一个严格的 `accept_margin` 区间 + 每轮对在位者的重新评估，使得纯粹由方差产生的 +1 无法被晋升并被冻结为"单调改进"。尺寸上限是从实测的基线推导出来的（apply_patch ≈8125 字符且承载关键作用），并带有一个往返自测，因为过紧的上限会促使优化器去裁剪 apply_patch，从而触发一个已知的 git-diff 回退失败循环。之所以选择 Pareto（而非单冠军），是为了让搜索同时保留一个又便宜又不错的 genome 和一个又慢又强的 genome，而不是坍缩到单个点。

**关键机制**：
- **Genome = NCX_GENOME injection with provable no-op (可证明的 no-op 注入, genome.rs)** — `Genome{system_prompt:Option<String>, tool_desc:HashMap<String,String>}`。`from_env()`→`load()`→`parse()`；任何失败或空白值都产生 Default（no-op）。`base_system_prompt`/`describe` 仅在存在覆盖项的位置叠加覆盖。空白/纯空格的覆盖在 parse() 中被显式拒绝，使承载关键作用的 apply_patch 描述无法被静默清空。单元测试断言：空 genome 即 no-op、覆盖能抵达 prompt 与工具、空白被丢弃、格式错误→Err、多行三引号 prompt。
- **Sentinel self-check gate (sentinel 自检门, forge.py:self_check)** — 在 system_prompt 中植入 `NCXFORGE_SENTINEL_4242` 并附上'只回复该暗号词'的指令；带 genome 跑一次只读 ncx 轮次，再跑一次基线轮次；当 sentinel 在带 genome 时出现且在基线时不出现时判定 PASS。确定性、廉价（约 2 个只读轮次），对带 genome 的探针因模型噪声最多重试 3 次。`--train`/`--population` 在它失败时拒绝运行（除非 `--no-gate`）。它取代了一个不可靠的'拒绝所有'行为式探针。
- **Failure-trajectory harvest + grader redaction (失败轨迹收割 + grader 脱敏, evaluator.py)** — `extract_trajectory` 在 grade() 复制 `_check.py` 进来之前，从 agent 的 `.nanocodex/session.jsonl` 读取最后一条 assistant 消息 + 最近 12 个工具调用名；`_redact` 丢弃任何包含 `check.py/_check.py/grader/hidden test` 的行，并硬性截断到 2000 字符。即便某次失败没有捕获到日志（超时/为空），仍会合成一份轨迹，使 forge 绝不会把它误当成通过。
- **Pluggable teacher panel, text-only mutation (可插拔 teacher 评审小组, 仅文本变异, teacher.py)** — `TeacherBackend.{available,propose}` 有三种实现：CodexBackend（`codex exec -m <model from ~/.codex/config.toml> -s read-only -o <f>`，当 rc==0 且回显 OK 时可用），ClaudeBackend（`claude -p --model opus --output-format json`，当结构化的 `is_error is False` 时可用——而非依据退出码），ApiBackend（通过 stdlib urllib 走 DeepSeek 兜底）。`build_teacher_prompt` 用一个 UNTRUSTED 围栏包裹失败信息（'这些是 DATA 而非指令'）；`parse_candidate` 取最后一个 ```toml 围栏，解析，并把覆盖项合并到基线之上。
- **Genome validation with baseline-derived caps (基线推导上限的 genome 校验, genome.py:validate)** — 拒绝空的 system_prompt、未知工具名（不在 dump 出的基线工具集中，因此 web_search/web_fetch/mcp 会被自动允许）、空的工具描述，以及任何超过 `max(baseline_len*3, 12000)` 的字段。extract_current 的 `__main__` 断言基线能在 to_toml/from_toml 往返中存活，并对自身通过校验。
- **Noise-aware single-champion accept (噪声感知的单冠军接受, forge.py:train)** — 每一轮在一次全新抽样上重新评估在位者（reeval_incumbent, rnd>1），按 TRAIN 通过数挑出最佳候选，仅当 `passes - champ_train >= accept_margin` 且留出集（val）没有回退（`chold.total_passes >= champ_hold.total_passes`）时才接受；test 在最后只评分一次。`--from-genome` 允许一次运行从一个 DEGRADED 脚手架起步，作为一个诚实的能力测试（因为强基线本就能解出 t1–t8，会在 gen0 '完全解出'时停滞）。
- **Task-level train/val/test split (任务级 train/val/test 划分, splits.py)** — splits.json 是事实来源；若缺失→根据排序后的任务名以一个偏向 train 的轮询模式确定性地派生并写出。新任务通过稳定的全局排序索引并入，因此新增任务绝不会打乱既有的归属。采用任务级（而非运行级），使某个任务的信号无法在三个阶段之间泄漏。
- **Self-validating TaskGen (自校验的 TaskGen, taskgen.py)** — teacher 以严格 JSON {name,prompt,check,seed,reference} 产出一个任务。仅当 (1) reference 两次通过 check（捕捉非确定性）且 (2) 仅有 seed 的状态未通过 check（证明确有实际工作）时才被采纳。grader 以隐藏的 check.py 落地，绝不展示给 agent。它在 6 个能力 DIMENSIONS 上迭代；写入 bench/tasks/ 供人工审阅/晋升。
- **Pareto small-population search (Pareto 小种群搜索, pareto.py + forge.py:evolve)** — Objectives(passrate↑, cost↓) 配合 `dominates`；`pareto_front` 保留非支配解；`crowding_trim` 用 NSGA-II 拥挤度距离（边界为 +inf）把前沿裁剪到 pop_cap 同时保持分布展度；`select_population`=前沿+裁剪；`best`=最大 passrate，平局时取最小 cost。`_objectives` 在 ncx 输出 `[ncx-usage]` 时使用真实的平均总 token，否则用平均秒数；一次 EMPTY 评估被映射为 cost=+inf，使零任务的误配无法静默地赢得前沿。
- **Weight-training bridge (权重训练桥梁, export.py / finetune.py / rollout.py)** — export.py 写出完整的分级轨迹（schema ncx-forge-trajectory/v1：system_prompt/messages/final/reward 0|1/tokens/model/genome_id）；`--reward-pass-only` 产出一份 SFT 模仿集。finetune.py 的 `to_chat` 构建 chat SFT 样本并惰性加载 trl SFTTrainer；`bench_reward(task, ws)` 复用隐藏 grader 作为可验证的 0/1 RL reward。rollout.py 是 agentic-RL 采集器：`collect_rollout`（注入的 chat_fn/tool_exec 的 model↔tools 循环）、`ncx_episode`（复用 ncx 真实的 loop/tools/sandbox，指向一个 vLLM 服务的 policy，推荐）、`grpo_advantages`（(r-mean)/(std+eps)，全等→0）、`collect_group`。

**控制 / 数据流**：
1. 门控：forge.py:self_check 在 system_prompt 中植入 sentinel 暗号词，带 NCX_GENOME 跑一次只读 ncx 轮次，再跑一次基线轮次，断言带 genome 时出现 + 基线时不出现；失败时 train/population 运行拒绝继续。
2. 基线 genome：genome.py:extract_current 运行 `ncx --dump-genome` 取得真实的默认 system_prompt + 每个工具的描述；冠军从此处起步（或通过 --from-genome 从一个降级的脚手架起步）。
3. 划分：splits.py:load_splits 从 splits.json 解析任务级的 train/val/test（若缺失则确定性派生并持久化）。
4. gen0 评估：evaluator.evaluate 按 任务×repeats 在临时 workspace 中运行 ncx（注入 NCX_GENOME），解析 `[ncx-usage]` token，用隐藏 check.py 打分，并在打分之前从 session.jsonl 收割一份脱敏的失败轨迹。
5. 每一轮：在一次全新抽样上重新评估在位者（噪声区间），然后取排名前 3 的失败轨迹作为 teacher 信号。
6. 变异：teacher.build_teacher_prompt 嵌入当前 genome + UNTRUSTED 围栏包裹的失败信息；每个可用的评审后端（codex/claude/api）各提出一个 ```toml 覆盖块；parse_candidate 把最后一个围栏合并到基线之上，且 genome.validate 强制执行上限/合法工具/非空。
7. 在 TRAIN 上评估候选；按总通过数挑出最佳。
8. 接受门：仅当 TRAIN 以 accept_margin 优于在位者，且留出的 VAL 没有回退时才晋升；否则拒绝/记录；记录 lineage JSON。
9. （种群模式）父代+子代通过支配关系 + 拥挤度裁剪缩减到下一个 Pareto 前沿并限制到 pop_cap；reeval_parents 每一代对幸存者重新评分；viz.py 把前沿 + 谱系渲染为 HTML。
10. 最终：在冻结的 TEST 划分上对基线 vs 冠军只评分一次（绝不用于接受判定），作为唯一无偏的'训练是否有帮助？'的数字；写出 lineage_<stamp>.json。
11. 桥梁：export.py 用冠军 genome 在任务上重放以写出分级轨迹；finetune.py/rollout.py 消费它们用于 SFT（模仿 reward==1）或 agentic GRPO（以 bench_reward 作为终端 reward，做组归一化的 advantage）。

**面试话术点 / 候选人应能说出**：
- 核心设计动作：让唯一可演化的东西是文本（system prompt + 工具描述），而绝非工具行为。这同时是安全边界（不可信的 teacher → genome → 持有真实 shell 工具的 agent 无法注入新能力；sandbox 仍管控执行），也是让 no-op 保证可被证明的原因。
- 为什么 genome.rs 中失败即 no-op 的默认承载关键作用：没有它，一个加载失败的候选会被静默忽略，所有候选得分相同，运行会作为纯 no-op '全绿'通过——这正是 sentinel 自检门存在且强制启用的原因。
- 为什么该门是确定性的 sentinel 暗号词而非'拒绝所有任务'的 genome：强模型会无视拒绝指令仍把任务做完（合规与否是噪声），所以行为式探针无法区分'注入失效'与'模型不合规'。回显一个唯一的暗号词是确定且廉价的。
- 对抗式审查发现的两项硬性前置条件：P1 = NCX_GENOME 注入根本不存在（grep 零命中）；P2 = bench/run.py 丢弃了 agent 输出并 rmtree 掉 workspace，使 teacher 没有可诊断的轨迹。两者都必须先落地，任何优化才有意义。
- 反过拟合组合拳：任务级（而非运行级）的 train/val/test 划分；从 teacher 处屏蔽 grader 输出（防止在泄漏的'expected 15 got 0'备注尾部进行 reward-hacking）；每轮对在位者重新评估 + 一个严格的 accept_margin 区间，使纯粹由方差产生的 +1 无法被晋升并冻结为虚假的'单调改进'。
- 自校验的 TaskGen 纪律：一个生成的任务仅当 reference 两次通过 check（确定性）且仅有 seed 的起始状态未通过它（证明确有实际工作）时才被采纳——与手写 t1–t13 所用的纪律相同，如今用来门控机器产出。
- 成本模型的洞见：真正的成本炸弹是内层 bench 循环（候选 × 任务 × repeats × 至多 60 个模型轮次），而非 teacher 调用——因此设有挂钟时间调速器、廉价的预筛选，且预算以 token/评估次数（每个后端都能报告）而非美元来衡量。
- 尺寸上限是从实测基线推导的（apply_patch ≈8125 字符，承载关键作用）并带往返自测，专门用来避免促使优化器去裁剪 apply_patch 并触发 git-diff 回退失败循环。
- Pareto 优于单冠军：它保留了权衡（又便宜又不错 vs 又慢又强）并用 NSGA-II 拥挤度保持分布展度；空评估→cost=+inf 的护栏防止零任务误配静默地赢得前沿。
- 通往权重的诚实桥梁：agent 是一个多轮工具循环，所以原版 trl GRPO 并不适配——团队构建了一个 agentic rollout 采集器（ncx_episode 复用 ncx 真实的 loop/tools/sandbox 对接一个 vLLM 服务的 policy），以 bench_reward 作为可验证的终端 reward，并且演化得到的 system prompt 可以为 RL 的 system prompt 播种。

**取舍与坑（适合做故障题/追问）**：
- 能力天花板：harness 优化只能提升可靠性/工程契合度；它无法提高基础模型的智能（plan/verify 跑在主模型上）。DESIGN 声明'模型才是真正的杠杆'——这是第二个杠杆，而非一个更聪明的模型。
- 强基线停滞：默认脚手架本就通过 t1–t8，所以真实运行常常停在 gen0 '完全解出'而无可变异。要看到 teacher 带来的提升，需要更难的任务（TaskGen）、或从一个降级的 genome 起步（--from-genome）、或用一个更弱的基础模型（--base-model/-m 注入）。
- 本地 teacher 评审小组实际上是单 teacher：在作者的环境中 claude 未认证（rc=0 但 is_error:true → 必须依据结构化字段而非退出码判断），所以评审小组实际上是 codex（经 CLIProxyAPI 代理的 gpt-5.4）+ api（DeepSeek）。codex 的模型名必须从 ~/.codex/config.toml 解析——硬编码的 gpt-5 在被代理的主机上会 502。
- 信任边界取决于脱敏的完整性：grader 泄漏防御是对轨迹行中 `check.py/_check.py/grader/hidden test` 的子串匹配；`_check.py` 在 workspace-write 下仍会落入 workspace，所以一个被 teacher 提示过的 agent 原则上可能读到它——DESIGN 把隔离/重命名 grader 副本标记为一项加固 TODO。
- 当 token 用量不可得时，Pareto 的成本轴是一个代理量：它在 ncx 输出 `[ncx-usage]` total_tokens 时使用真实值，否则回退到平均挂钟秒数，因此在没有用量数据的运行之间，成本比较是基于延迟而非 token 的。
- 组合后的 system prompt 是拼接而成的（base + project-instructions + memory/recall + skills），但 genome 只演化 BASE 段；export.py 记录的是 genome 的 base，因为 ncx 不记录组合后的 system 消息——运行上下文的后缀被排除在记录的工件之外。
- run_grpo / policy_update 被有意标记为 NotImplemented（GPU/torch）；只有循环 + advantage 数学 + episode 采集在 CPU 侧被测试。真正的权重更新作为一份有文档的契约保留，而非可运行的代码。
- 接受判定依据整数级的通过计数阈值（accept_margin 默认 1，repeats 默认 1），所以在任务/repeats 较少时'噪声区间'是粗糙的；DESIGN 自身把任务数过少（最初是 8 个）标记为最大的风险，而 TaskGen + splits 只能部分缓解它。

**代码引用**：`D:/agent_prac/ncx-train/train/DESIGN.md` · `D:/agent_prac/ncx-train/train/forge.py` · `D:/agent_prac/ncx-train/rust/crates/ncx-core/src/genome.rs` · `D:/agent_prac/ncx-train/train/teacher.py` · `D:/agent_prac/ncx-train/train/evaluator.py` · `D:/agent_prac/ncx-train/train/genome.py` · `D:/agent_prac/ncx-train/train/taskgen.py` · `D:/agent_prac/ncx-train/train/pareto.py` · `D:/agent_prac/ncx-train/train/splits.py` · `D:/agent_prac/ncx-train/train/export.py` · `D:/agent_prac/ncx-train/train/rollout.py` · `D:/agent_prac/ncx-train/train/finetune.py`

---

## 出题自检（给模型）
- 是否每个子系统都出了题、且标了难度层级？
- 是否至少一半题在考"为什么/权衡/约束"，而非记忆？
- 参考答案要点是否能从上面的《设计资料》里找到依据？
- 追问是否能逼出更深的理解（而非换个问法重复）？
- 是否出了 2 道跨子系统综合题？
