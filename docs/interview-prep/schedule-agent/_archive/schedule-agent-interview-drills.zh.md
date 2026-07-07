# 排产 Agent · 精炼刷题卡（口播自检版）

> 对部署版排产 `agentbot/schedule-agent-langgraph` 刷题、录语音。少而尖：1 道总纲 + 必刷 3 + 补充 2 + 选刷 1。
> 刷法：① 只看【题目】录一段 → ② 对【答题骨架】自检漏了哪条 → ③ 背【一句话】 → ④ 想【换业务/易错点】。
> 配套流程图：`schedule-agent-deploy-deepdive.zh-CN.html`。
>
> **诚实纪律（重要）**：部署版是 **确定性 LangGraph workflow + 规则意图识别 + 四段治理 + RAG**，**不含 CP-SAT solver**。凡讲到"最优化 / solver / 组合优化"，用「如果重新设计我会…」句式（对齐 `agent-case-compliance-vs-scheduling.zh.md` 的理想版），别把它说成已做。

---

## Q0 · 总纲（场景迁移）
**题目：** 同一套 agent harness 换一个业务场景，哪些环不变、哪些环要重新「承重」？拿你的排产 agent 举例。

- **考点：** 驾驭工程的可迁移性 + 「场景决定承重」的判断力。
- **答题骨架：**
  1. **不变的骨架**：prompt 钉角色边界 / context 供可信数据 / 工具面收窄副作用 / ground-truth 校验 + 人审兜出口 / **LLM 原始输出不直接落地**。
  2. **变的是承重**：任务是 *LLM-native*（语义比对是主活）还是 *LLM-as-interface*（主活外包给确定性系统）→ 决定 LLM 是主判断者还是翻译官。
  3. **风险形态**：最怕假阴性（合规漏判）还是假阳性（排产幻觉方案 / 编码乱改）→ 决定保守方向。
  4. **排产落点**：写操作直接改生产库、副作用大 → **Guardrails 环（四段治理）承重**；LLM 被降级为"意图理解 + 方案解释"，路由/治理/执行全是确定性的。
- **一句话：** 同一套方法论——合规长成 RAG+judge、排产长成 **确定性 workflow + 四段治理**、编码长成沙箱+审批+压缩；**骨架同，承重随业务。**

---

## Q1 · 写操作四段治理 【必刷】
**题目：** 排产 agent 怎么保证 LLM 不会乱改生产库？

- **考点：** Guardrails 环 · 写治理 · 人在回路 · 安全建在代码不建在措辞。
- **答题骨架：**
  1. **四段链** `governance → preview → execute → verify`，写操作**永不直接落库**。
  2. **governance**：角色层级 `业务员0<工艺员1<主任2<厂长3`（层级在代码 `ROLE_HIERARCHY`，per-intent 的 minimum_role/risk 在 `intents.json`）；blocked 三因：未知意图 / 权限不足(`user_level<required`) / 高危缺机台号。信号字段 `governance_passed` 布尔一票。
  3. **preview**：纯 dry-run 只读，产 `preview_result{impact}` 给人看；`after_preview` 用严格 `confirmed is True` 才放行——**人在回路**，未确认只出预览不落库。
  4. **execute**：真实 SQLite 落库；`machine_failure` 批量改带 `LIMIT 50` 防误伤；本节点**不写审计**。
  5. **verify**：按 intent 回读 DB 生成 `checks[]`；不一致**不回滚**，降 `needs_review` 标人工复核 + 写审计（审计只在 governance-blocked 和 verify 两处写）。
- **一句话：** 提案→预览→执行→回读四段闭环 + 角色权限 + 人在回路——**把数据库事务的思路搬到 agent 写操作**。
- **换业务：** 金额/退款类 → 审批分级（初审快筛 / 资深复核）；只读分析 agent → 治理环退居次要、检索环承重。

---

## Q2 · 确定性 workflow / 为什么不让 LLM 全权 【必刷】
**题目：** 为什么用 LangGraph 确定性 workflow，而不是让一个 agent 自由 loop 去排产？

- **考点：** Workflow 环 · LLM-as-interface 第一性原理。
- **答题骨架：**
  1. **整体是确定性 StateGraph**：`after_route` 三路分流（need_clarify / readonly / write），条件边硬编码；**LLM 只在 node 内需推理时才决策**（rag 语义、respond 生成）。
  2. **意图识别零 LLM 成本**：规则匹配 + 关键词打分（19 写意图），省钱且**可复现**。
  3. **为什么不自由 loop**：排产要**可复现、可排障、可治理**；同一请求两次结果不同就没法排障；确定性流程才能在**每个写操作边界插治理闸**。
  4. **诚实边界**：核心排程的"最优化"若要上 solver（CP-SAT），是「如果重新设计我会…」方向；**当前部署版是规则 + DB 写 + 治理，不含 solver**。
- **一句话：** 确定性 workflow 包住 LLM——LLM 只当翻译官（理解/解释），路由和治理是确定性的，这样才**可复现、可排障、可治理**。
- **换业务：** 任何"确定性路由 + 状态机"占主体、只在局部需推理的业务都适用（客服分诊、审批流）。

---

## Q3 · 评测体系 + 数据飞轮 【必刷】
**题目：** 你怎么知道改一版 prompt / 换个模型没让 agent 退化？评测集哪来的？

- **考点：** Evals 环 · 三层防护 + 数据飞轮。
- **答题骨架：**
  1. **三层防护**：`audit`（防 skill 退化，5 检查：dark / conflict / self_recog<0.5 / coverage / orphan）→ `check_expected`（~16 类**确定性**断言）→ `judge`（LLM 3 维 1–5，可降级为规则）。
  2. **门禁硬**：`rule_pass_rate ≥ 1.0` 才 exit 0；CI 里 `audit` 先过才跑评测（`needs: audit`）。
  3. **防幻觉断言**：`max_retrieval_steps`（防 agentic RAG 无限检索）、`response_must_not_contain`（订单不存在时禁止编字段）、`answer_grounded / evidence_source`。
  4. **数据飞轮**：线上 `audit_log` → 挖候选 → 人工审 → 晋级 `online_hard_cases.json` → 定时回归——评测集**从真实失败长出来**。
- **一句话：** 三层防护（防退化审计 + 确定性断言 + 可降级 judge）+ 硬门禁 + 数据飞轮——**评测是长出来的，不是拍脑袋造的**。
- **易错点：** 实际 **44 用例 / 11 类**（不是 8 类）；`conflict` 是 warning 不阻断；`bench_chunking`/`lab_dpo` 是离线 lab，不进 CI 门禁。

---

## Q4 · RAG 工程化 【补充】
**题目：** 规则检索这块 RAG 你怎么做的？为什么不是直接 embedding 一把梭？

- **考点：** Context 环 · RAG 工程。
- **答题骨架：**
  1. **双模检索**：Qdrant + fastembed（bge-small-zh）语义 + **文件关键词 fallback**（向量库挂了不崩）。
  2. **重排 + 实验**：reranker 重排；graphRAG lab（kg_graph）；chunking bench（BM25 对比切块策略，离线 lab）。
  3. **query_plan 先判该不该检索**：`mode(structured_sql vs rag)` / `requires_rag` / `primary_tool`——不是无脑 RAG，结构化查询直查 SQLite。
  4. `max_retrieval_steps` 断言防 agentic RAG 无限检索。
- **一句话：** RAG 胜负手不在 embedding 模型，在"**该不该检索**"的 query_plan + 双模 + fallback + 重排，且检索步数有闸。
- **换业务：** 合规 → 按条款切分 + 效力层级元数据；客服 → FAQ 检索 + 订单直查分流。

---

## Q5 · 确定性校验 vs LLM judge 【补充 · 跨场景元原则】
**题目：** critic / 校验这一环，什么时候用确定性校验、什么时候才上 LLM judge？

- **考点：** Guardrails/Evals 的通用判据（高频、体现判断力）。
- **答题骨架：**
  1. **有数学/可程序化判据 → 永远优先确定性**：排产的 `verify` 回读 + `checks`、eval 的 `check_expected` 断言、权限的 `can_write`。
  2. **只有语义判据 → 才上 LLM judge**：如合规「该条款是否支持该结论」，且 judge 必须用人工校准集对齐（agreement ≥ 0.9）。
  3. 排产的 `verify` 是**确定性回读**，不挂大模型评审；judge 只在 eval 里补语义 3 维，且**可降级为规则**。
- **一句话：** 有确定性判据就别用 LLM judge；judge 只留给纯语义场景，且必须校准。

---

## Q6 · 防幻觉 / 意图识别 【选刷】
**题目：** 用户查一个不存在的订单，agent 会不会编出机台号 / 排产日期？怎么防？

- **考点：** 防幻觉 + grounding。
- **答题骨架：**
  1. **意图识别零 LLM**（规则 + 关键词打分），确定性、可复现、省成本。
  2. **grounding**：`answer_grounded` + `evidence_source=sqlite`；`response_must_not_contain` 断言（不存在时禁止出现"机台：/排产日期：/规则 R-"）。
  3. **query_plan 直查**：`structured_sql` 直查（`max_retrieval_steps=0`），订单不存在 → 明确答"不存在"，不进 RAG 乱编。
- **一句话：** 订单不存在就如实说不存在——grounding 断言 + 直查 + 禁止编造字段，**把"不知道"作为合法输出**。
- **换业务：** 合规 → `insufficient_context` 转人审；客服 → 查无此单转人工。

---

### 刷题优先级
- **先刷 Q1 / Q2 / Q3**（四段治理、确定性 workflow、评测飞轮）——排产最差异化、最可能被深挖的点。
- **Q0 做开场框架**，任何 agent 系统设计题都能用它起手。
- **Q5 是跨场景元原则**，合规/排产/编码/网关都能套。
- **口径统一**：排产"数据不出域→敏感本地" = 网关"脱敏+block" = 合规"通道隔离"，同一信条不同承重。
