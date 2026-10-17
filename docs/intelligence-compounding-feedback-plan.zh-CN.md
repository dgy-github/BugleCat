# 智能复利回流 & 无人工介入完成率 — 开发计划

> 状态（2026-10-17）：P1–P6 代码已落地；首轮扫描发现的正确性缺陷已按 HANDOFF N1–N5 修正并补测试。**尚未经真实使用验证**，两个开关保持 `shadow`，进入 2 周基线期（N6）后才可评估切换 `on`。实现与本文的差异见第 6 节。
> 来源：对照曾鸣《智能》第二章"智能复利 / 60 分奇点"对 BugleCat 的差距分析。
> 结论一句话：harness 已经把过程与结果数据串联为"行动 → 结果（`TaskOutcome` + `OutcomeFollowup`）→ 反馈 → 学习（记忆有界调权 + 路由单调升级 + 失败恢复候选沉淀）"的闭环，并持续度量"AI 对结果负责"的无人工介入完成率（ACR）与 60 分奇点距离。

两个目标：

1. **G1 主线指标**：定义并持续度量"无人工介入完成率"（ACR, Autonomous Completion Rate），量化离 60 分奇点还有多远。
2. **G2 在线回流**：任务结果 → 记忆打分、路由打分，形成"行动 → 结果 → 反馈 → 学习"的闭环。

G1 是 G2 的前提：没有可信的结果标签，回流就是噪声放大器。所以先做 G1。

---

## 1. 现状：当前 harness 的流程与可用信号

### 1.1 一次任务的生命周期（Rust 主线）

```
CLI cli_app.rs / GUI bridge.rs / orchestrator_runner.rs
        │ run_turn / run_goal_round
        ▼
AgentLoop::run_turn_with_authority            (ncx-core/src/agent_loop.rs:553)
  ├─ next_turn_id++，tools.begin_task
  ├─ retrieval_provider.record_user_instruction   ← 用户指令可撤销冲突记忆
  ├─ state_machine: Planned → Authorized → Executing   (agent_state.rs, 落 context_store)
  ├─ turn::run(...)                            (agent_loop/turn.rs)
  │    ├─ 组装 prompt：hooks + turn_context + memory_recall_notes(LEARNINGS.md)
  │    ├─ 模型调用 ↔ 工具调度（middleware / 审批 / 沙箱 / recovery）
  │    │      └─ 审批：StateRecorder::record_approval → AwaitingConfirmation → Authorized|Blocked
  │    └─ 验证（verification::Mode，生产默认 off）
  ├─ apply_stop_hook
  ├─ stop_reason → 终态映射：
  │    cancelled→Cancelled；timeout/task_budget/compaction_failed/blocked/verification_failed→NeedsHuman；
  │    error→Failed；含 compile/validation→Failed；其它→Completed
  └─ cost_service.record(usage)                ← 唯一的"结果后处理"挂点，目前只记成本
        ▼
TurnResult { final_text, iterations, stop_reason, tools_used, usage }
```

`run_turn_with_authority` 是 CLI、GUI、orchestrator worker 的**唯一公共出口**，并且只有一条返回路径。这是落 outcome 记录的天然位置。

### 1.2 已有、可直接复用的信号

| 信号 | 位置 | 用途 |
|---|---|---|
| 终态 Completed / NeedsHuman / Failed / Cancelled | `agent_loop.rs:620-640` | 任务是否自主结束 |
| 审批请求与结果（approved/denied，带 tool_call_id） | `agent_state.rs:297 record_approval` → `context_store` `StateTransition` 事件 | 人工介入次数 |
| `ask_user_question` 调用 | `TurnResult.tools_used` | 人工介入次数 |
| 验证状态（verified / invalidated / need_evidence / contradiction） | `LoopEvent::Verification`、`EventType::RuntimeVerification` | 结果可信度 |
| workspace_revision（turn 开始时 git 修订） | `StateRecorder::begin_turn` | 判断事后人工改动 |
| 成本 / usage | `CostTelemetryService` | 成本维度 |
| orchestrator 复杂度、verify_passed、verify_rounds、best_worker | `orchestrator/types.rs:141 OrchestratorOutcome` | 路由效果 |
| checkpoint 创建 | `EventType::Checkpoint` | 回滚检测（restore 需补事件） |
| 事件存储（SQLite、按 event_id 幂等、已脱敏） | `context_store.rs` | 存储底座 |

### 1.3 断点（为什么现在没有复利）

1. **没有任务级结果记录**：`TurnResult` 用完即弃，只有 cost 被汇总。没有"这个任务最后被接受了吗"的持久记录。
2. **记忆召回不可归因**：`memory_recall_notes`（`turn.rs:976`）把 `MemoryStore::recall` 返回的**纯字符串**拼进 prompt，不记录本轮注入了哪些 note。结果没法归因到 note 上。
3. **两套记忆、都不看结果**：`MemoryStore`（LEARNINGS.md，排序看词法 + 时间）和 `MemoryIndex`（SQLite，`recall` 只按 `created_at` 倒序），都没有 uses / success 字段。
4. **路由不看历史**：`Orchestrator::classify`（`orchestrator.rs:193`）每次都是一次性模型判断；`OrchestratorOutcome` 不落盘；A/B worktree 的"采纳哪一侧"也不沉淀。
5. **事后修正不可见**：用户下一条消息是"不对 / 还报错"、`/restore` 回滚、手动改动 agent 刚改过的文件，这些最强的负反馈信号目前都没有被捕获。
6. **worker 会被重复计数**：`orchestrator_runner.rs:168` 的 worker 也走 `run_turn`（authority = `DirectHuman`），如果直接在出口记录，一个用户任务会被计成 N+2 个。

---

## 2. G1：无人工介入完成率（ACR）

### 2.1 任务单元

- **一个任务 = 一次 `DirectHuman` turn**，加上它触发的所有 Goal 自动轮次。`task_id = {session_id}:{turn_id}`。
- 新增 `TaskRole { Primary, OrchestratorWorker, OrchestratorNode, GoalRound }`。只有 `Primary` 计入 ACR 分母；worker、node 的结果挂在父任务下作为路由数据。
- 分母排除：无工具调用的纯问答（`tools_used` 为空，且 deliverable 未检测到）单独统计为 `chat` 类，不进主线 ACR。避免闲聊稀释指标。

### 2.2 介入信号（按可观测性分级）

| 级别 | 信号 | 来源 | 何时可得 |
|---|---|---|---|
| I1 显式阻断 | 终态 NeedsHuman / Failed / Cancelled | 状态机 | turn 结束时 |
| I2 显式询问 | 审批弹窗（无论 approved/denied）、`ask_user_question` | StateRecorder、tools_used | turn 结束时 |
| I3 回滚 | 下一个任务开始前执行了 `/restore`、GUI 恢复 checkpoint | 需新增 restore 事件 | 下一个任务开始时 |
| I4 纠正 | 下一条 DirectHuman 消息被判为"纠正 / 返工" | 确定性关键词表（中/英/日，复用 adaptive effort 的多语言表思路）+ 是否涉及同一文件 | 下一个任务开始时，标记为 `inferred` |
| I5 事后手改 | 下一任务开始时，agent 本轮改过的文件被人工改动（git diff 对比 turn 结束快照） | workspace_revision + files_touched | 下一个任务开始时 |
| L 人工标注 | `/outcome good|bad [note]`（CLI）、GUI 点赞点踩 | 新增 | 任意时刻，优先级最高 |

### 2.3 指标定义

- **ACR-strict**（主线指标）= 无 I1–I5 的 Primary 任务数 / Primary 任务总数（排除 chat）。
- **ACR-approval-tolerant** = 允许 I2 中的"审批通过"，其余同 strict。审批多少由策略配置决定、不完全是 agent 能力，所以两个口径都报。
- **辅助指标**：介入次数 / 任务、返工率（I3 + I4 + I5）、首次验证通过率、每个被接受任务的成本。
- **切片**：按任务类别（确定性分类：bugfix / feature / refactor / test / build / docs / explain / other）× 模型 × 审批策略 × orchestrator 复杂度。
- **统计纪律**：报 Wilson 95% 置信区间；单切片 n < 20 只显示样本数、不给结论；I4 是推断值，报告中与 strict 分列。

### 2.4 "60 分奇点"如何量化

书里的"60 分"是结构性拐点（从人替 AI 兜底到 AI 对结果负责），不是一个技术分数。工程上把它落成可检验的门槛：

> **某类任务跨过奇点** ⇔ 滚动 30 天内 n ≥ 30，且 ACR-strict 的 Wilson 下界 ≥ 60%，且返工率 ≤ 15%。

60% 和 15% 是本项目选定的工程阈值，不是书里的数字，评审时可调。报告输出每个类别"当前下界 − 60%"的距离和趋势，这就是"离奇点还有多远"。

### 2.5 离线口径（与在线互补）

`bench/`、`evals/self_repair` 已经能跑非交互任务。把"首轮通过且无反馈轮"定义为离线 ACR，纳入 `evals/metrics.py`。离线口径用于回归门禁（eval-gate），在线口径用于反映真实分布。两者在报告中并列，不混算。

---

## 3. G2：任务结果在线回流

### 3.1 数据模型

新模块 `ncx-core/src/outcome.rs`：

```rust
pub struct TaskOutcome {
    pub task_id: String,              // session_id:turn_id
    pub parent_task_id: Option<String>,
    pub role: TaskRole,
    pub category: TaskCategory,       // 确定性分类
    pub started_at: u64, pub ended_at: u64,
    pub terminal: AgentState, pub stop_reason: String,
    pub iterations: usize, pub tool_calls: usize,
    pub approvals_requested: u32, pub approvals_denied: u32,
    pub user_questions: u32,
    pub verification: Option<String>,
    pub model: String, pub route: Option<RouteKey>,   // complexity + tier 配置
    pub recalled_note_ids: Vec<String>,
    pub files_touched: Vec<String>,
    pub workspace_revision: Option<String>,
    pub usage: BTreeMap<String, i64>,
    pub query_hash: String,           // 不存原文
}

pub struct OutcomeFollowup {           // 追加记录，不修改原 outcome
    pub task_id: String,
    pub signal: FollowupSignal,       // Accepted | Correction{inferred} | Rollback | ManualEdit | Label{good|bad}
    pub at: u64,
}
```

存储：在 `context_store` 增加 `EventType::TaskOutcome` / `EventType::OutcomeFollowup`，复用它的幂等写入、脱敏和跨会话查询，不新开存储。

### 3.2 记忆回流

1. **可归因召回**：`MemoryStore::recall` 新增 `recall_entries` 返回 `(note_id, text)`，`note_id = content_hash(normalized text)`，与 `MemoryIndex` 的 id 规则对齐。`memory_recall_notes` 把 id 写进 `TurnState`，最终进入 `TaskOutcome.recalled_note_ids`。
2. **反馈表** `memory_feedback(note_id, workspace, injected, accepted, rejected, last_used)`，放在 `MemoryIndex` 的 SQLite 里。
3. **打分**：Beta 后验 `p = (accepted + 1) / (injected + 2)`。排序时在现有词法分上叠加**有界**加成（±20% 以内），不让结果分压过相关性。
4. **降级而非删除**：`injected ≥ 8 且 p < 0.25` → 新状态 `NoteStatus::Demoted`，不再召回但保留行，`ncx --memory-merge` 时提示人工处理。复用"Superseded 保留行"的既有约定。
5. **归因的局限要写清**：注入不等于被使用，这是弱信号。所以只做有界调权，不自动删除，也不自动改写 note 文本。

### 3.3 路由回流

1. orchestrator 在 `handle` 结束时把 `OrchestratorOutcome` 摘要（complexity、tier 配置、verify_passed、verify_rounds、成本）写入父任务的 `route` 字段，worker 的 outcome 用 `parent_task_id` 关联。
2. `RoutingStats`：按 `(category, complexity, route)` 聚合 ACR 后验与成本中位数。
3. 策略开关 `[feedback] routing = "off" | "shadow" | "on"`，默认 `shadow`（沿用项目里 host-rules 默认 off 的稳健风格）：
   - `shadow`：只在 outcome 里记"按统计会选什么路由"，不改变行为。用于先评估收益。
   - `on`：仅做**升级**类调整。例如某类别在 Simple 路线上 ACR 下界 < 40% 且 n ≥ 20，就把该类升到 Medium 管线。不做降级（降级省钱但会带来静默质量风险），降级留给人工决策。
4. A/B worktree 的"采纳哪一侧"写成 `Label` 类 followup，作为路由偏好的高质量样本。

### 3.4 经验候选（可选，P4）

把"同一任务内失败 → 恢复成功"的配对（tool_recovery、验证失败后通过）抽成**候选** note，状态为 `Candidate`，必须带 `Evidence::VerifiedToolResult`。由 `--memory-merge` 或 GUI 人工确认后才转 Active。不允许模型摘要直接入库，现有 `PromotionRefused::UnverifiedSummary` 规则保持不变。

---

## 4. 分阶段计划

| 阶段 | 内容 | 主要改动 | 验收标准 |
|---|---|---|---|
| **P0 规格**（0.5 天） | 本文评审；确定阈值、类别表、纠正关键词表 | docs | 评审通过 |
| **P1 Outcome 记录**（2–3 天） | `outcome.rs`；`TaskRole`；在 `run_turn_with_authority` 出口生成 `TaskOutcome`；从 `state_machine.history()` 统计审批；worker 标 `OrchestratorWorker` | `agent_loop.rs`、`agent_state.rs`、`context_store.rs`、`orchestrator_runner.rs` | 单测覆盖各终态映射、审批计数、worker 不进分母；CLI 与 GUI 都能落记录（共享出口） |
| **P2 Followup 信号**（2–3 天） | restore 事件；下一 DirectHuman turn 开始时判定 I3/I4/I5 并追加 followup；`/outcome good|bad`；GUI 点赞点踩 | `checkpoint.rs`、`runtime_support.rs`、`turn.rs`、`bridge.rs`、Svelte | 纠正关键词表有中英用例；手改检测用临时 git 仓库测试 |
| **P3 指标报告**（1–2 天） | `ncx --outcomes report [--since 30d] [--by category,model,policy] [--json]`；Wilson CI；奇点距离；离线 ACR 进 `evals/metrics.py` | `ncx-cli`、`evals/` | 报告可复现；n 不足时不下结论；README 增加指标说明 |
| **— 基线期**（2 周） | 只记录、不回流，攒真实分布基线 | — | 至少 2 个类别 n ≥ 30 |
| **P4 记忆回流**（3 天） | 可归因召回；`memory_feedback`；有界调权；`Demoted` 状态；先 shadow 再开启 | `memory.rs`、`memory_index.rs`、`turn.rs` | shadow 期对比"调权前后排序差异"；开启后 ACR 不低于基线（CI 重叠即算不退化） |
| **P5 路由回流**（3 天） | 路由摘要落盘；`RoutingStats`；`[feedback] routing` 开关；A/B 采纳作为标签 | `orchestrator.rs`、`ncx-config` | shadow 报告显示"若开启会改变的任务比例与预期 ACR 变化"；`on` 只做升级 |
| **P6 经验候选**（可选） | 失败 → 恢复配对生成 Candidate note，人工确认后晋升 | `memory_index.rs`、`memory_merge.rs` | 未确认的候选永不召回 |

---

## 5. 风险与约束

- **古德哈特**：agent 不能看到 ACR，也不能为了"少弹审批"去绕开审批。审批由 sandbox 策略决定、不受模型影响，指标按审批策略切片报告。
- **推断标签有噪声**：I4 关键词判定会误报。报告中单列 inferred，人工标注优先级最高，阈值评估以 strict 和人工标注为准。
- **样本量**：个人或小团队使用量小，强制最小 n 和置信区间，宁可报"数据不足"。
- **隐私**：outcome 只存 query hash、类别和计数，不存 prompt 原文，与 context_store 现有脱敏保持一致；全部本地，不上传。
- **回流的回滚**：所有回流都有 `off / shadow / on` 开关；记忆调权有上限；Demoted 可恢复；路由只升级不降级。任何时候关掉开关，行为都回到当前版本。
- **与 forge 的关系**：forge（`train/`）是离线进化提示词和工具描述；本计划是在线调整召回与路由。后续可以用 outcome 中失败最多的类别驱动 `train/taskgen.py` 出题，让离线训练对准真实短板（不在本计划范围内）。

---

## 6. 实现与计划的差异（2026-10-17 修正后）

- **记忆计分时机**：不在召回时累加 `injected`，改为任务定论时把 injected 与 accepted/rejected 一起 +1；每个任务只计一次分，之后若有方向相反的信号（如人工标签），只翻转这一次计数。worker、Goal 轮次和未定论任务因此不会进入分母。
- **note_id**：反馈表以 LEARNINGS 的 `content_hash(normalize(text))` 为准。降级是 `memory_feedback.demoted` 上的持久标志，通过文本哈希同步到 `memory_index.status`，`rebuild_from_markdown` 之后会重新应用。降级只在 `on` 模式、且收到负反馈时发生；可用 `--memory-review restore <id>` 恢复。
- **shadow 语义**：注入内容与路由决策都和 `off` 完全一致，只在 `TaskOutcome.memory_rank_changed` 与 `RouteKey.shadow_recommended_route` 中记录"若开启会改变什么"，由报告的 Shadow 段汇总。
- **pending**：已完成但还没有 followup 的任务不进入任何比率；I1 失败的任务直接算已定论。
- **回滚归因**：只归因到 checkpoint 创建之后开始的最近一个 Primary 任务；CLI 下额外限定为当前会话。
- **候选经验**：只配对"同一工具、同一目标"的失败→成功，并且要求任务 Completed、验证结果为 passed 或 skipped；必须经 `--memory-review confirm` 人工确认后才写入 LEARNINGS.md。`--memory-merge` 不再自动晋升任何候选。
- **路由统计**：按分类器给出的初始复杂度（`RouteKey.initial_complexity`）分桶；非编排的直接轮次不参与统计；只取最近 30 天。
- **编排任务 id**：`{session}:orch-{turn}`。worker 和 node 的 outcome 记为 `{parent}:worker-N-k` / `{parent}:node-k`，写入工作区事件库。
