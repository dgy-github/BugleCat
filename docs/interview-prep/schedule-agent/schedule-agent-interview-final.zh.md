# 排产 Agent · 面试口播终版（叙事骨架 + L3 血肉）

> 合并两层：**叙事骨架**(来自 ChatGPT 复盘，L1/L2，口播顺)+ **L3 血肉**(真实函数名/常量/数字/易错点，被钻时用)。
> 用法：先背【叙事】开口，被追问翻【L3 血肉】，卡壳看【被追问】。每段末尾标了真实机制。
> 定位诚实：**已上线的制造业排产 1.0 最小可行闭环**；半途接手后用 AI 改造，深层原系统设计非我从零做——被钻到原系统内部用"如果重新设计我会…"兜。

## 🔢 数字速记（ChatGPT 版最缺的，先记这几个）
- 数据：**49,981 条**真实生产订单，纺织/化纤行业，SQLite。
- 意图：**19 类写意图**（configs/intents.json），四级角色权限。
- 评测：**golden 44 用例 / 11 类**，CI 门禁 `rule_pass_rate ≥ 1.0` 才 exit0。
- DB 关键字段：`ctsalebid`(销售订单编号=业务主键) / `jt`(机台) / `gg`(规格,如 545dtex/72f) / `pcrq`(排产日期) / `ddsl`(数量,吨)。
- 术语更正：链路 ID 是 **`trace_id`** 不是 chain_id。
- **RAG 实测**(challenge/bench 跑出,非口胡)：Hybrid+RRF **Recall@3=0.83 / Precision@3=0.40**；切块 **markdown 一规则一块 MRR=0.956**(优于 fixed_200=0.935 / fixed_500=0.917 / recursive_300=0.907)。
- **可观测实测**：端到端 **P50=941ms / P95=4702ms / avg=1480ms**；节点级 **LLM P95=4588ms vs Route P95=15ms**(慢节点=LLM)；成功率 90%；50 请求总成本 **¥0.0774**(≈¥0.0015/请求)。

---

## ① 定位（叙事强，保留）
**叙事**：已上线的制造业排产 **1.0 最小可行闭环**——不是完整覆盖全部工厂规则的全量系统，但已验证 Agent 在复杂制造业里做"自然语言理解→规则匹配→流程编排→DryRun→结果解释"的可行性。覆盖订单状态/交期/规格/机台能力/负载/就近/切换/故障/插单等典型高频场景。
**L3 血肉**：数据 49981 条真实订单；19 类写意图；已 systemd 部署 + 公网隧道。
**被追问"为什么叫最小可行"**：规则覆盖不完整（全量工厂规则没做完）、调度优化能力弱（当前是规则+查询不是 solver）——但写操作全链路治理已闭环、已上线验证。

## ② 为什么用 Agent / 规则在前模型在后（叙事强）
**叙事**：排产是多角色、多步骤、多约束的调度系统，需求动态（急单/临时调整/参数不全/表达不标准）。**规则和算法做确定性判断，Agent 做流程编排+人机交互，模型做语义理解+结果解释**——不是"大模型替代算法"。
**L3 血肉**：架构是 `业务规则/数据查询/执行链路在前，模型在后`。核心判断（订单状态/机台负载/规格匹配/权限/能否停排/能否故障转移）走规则+SQL+确定性逻辑；模型只做后置增强（信息补充/格式优化/规则说明/澄清/解释）。
**被追问"哪些给模型哪些不给"**：**判断/分类一律不给模型**。①安全相关分类（权限/是否可写/风险）不给；②**意图识别本身也不给模型**——是规则+关键词打分（`route.py` 的 `_calculate_intent_score` 遍历 `configs/intents.json`：trigger_phrases 命中 +12、各意图关键词 +16/18/40、冲突信号 −30/−36，**零 LLM**）；`confidence` 由 `_score_to_confidence` 把**规则分数**阶梯映射（score≥30→0.95 / ≥24→0.90…），**不是模型概率**。消歧看 **top1/top2 分差过小** 或 **写操作 confidence < 阈值** → `need_clarify`，低置信不猜。**给模型的只有后置**：语义理解补充/格式优化/规则说明/澄清话术/结果解释——模型在"理解与表达"层，不在"判断与分类"层。

## ③ 执行链路：四段治理（叙事有，L3 是缺口）
**叙事**：写操作不是让 Agent 直接执行，而是 `意图识别→参数抽取→权限管控→DryRun→用户确认/审批→Execute→Verify 回读→结果解释`。
**L3 血肉**（真实节点/机制）：
- 真实拓扑：`rewrite→route→query→rag→answer_verify→governance→preview→execute→verify→respond`。
- 三条件边：`after_route`(歧义/`confidence<0.5`/缺实体→need_clarify；`is_write`→write；else readonly)；`after_governance`(看布尔 `governance_passed`)；`after_preview`(**严格 `confirmed is True`** 才 execute，否则 await_confirm)。
- **preview 是纯 dry-run 只读**，产 `preview_result{action,summary,impact}`；preview 节点**不设 confirmed**，由人工/入口注入——这是 human-in-the-loop 落点。
- **verify 回读生成 `checks[]`；不一致不回滚，降 `needs_review` 标人工复核**。
- 写链信号字段串：`is_write → governance_passed → confirmed → verify_result.status`。
**被追问"verify 不一致为什么不自动回滚"**：宁可标复核转人工，也不静默回滚——制造业写操作可逆性和影响面复杂，自动回滚可能造成二次扰动，让人判更稳。

## ④ 安全边界 / 权限 / DryRun（叙事有，补 L3 常量）
**叙事**：Dry Run 不是普通预览，是写操作前的安全防守层，和权限管控、审批、回读一起保证写操作可控可追踪可回滚。查询灵活、写操作保守。
**L3 血肉**：
- 角色层级在代码：`ROLE_HIERARCHY = [业务员0, 工艺员1, 主任2, 厂长3]`；`user_level < required_level → blocked`（required 来自 intents.json 的 minimum_role）。
- blocked 三因：**未知意图 / 权限不足 / 高危缺实体**（机台故障缺 machine_id → block）。
- 故障转移批量改带 **`LIMIT 50`** 防误伤。
- 网关审批流：`/agent/approval/create → confirm/reject`。
**被追问"高危操作怎么防"**：物理隔离 + 分级——不可逆写强制走审批，缺关键实体直接 block，不靠 prompt 劝模型别乱来。

## ⑤ 审计（叙事只到"可追踪"，L3 是大缺口）
**叙事**：写操作全程留痕，可回链、可追责。
**L3 血肉**：
- `audit_log` 表 **append-only**，字段：`trace_id / intent / action / actor_role / status / success / need_approval / before_state_json / after_state_json / verify_checks_json / metadata_json`。
- **只在两处写**：governance-blocked（拦截时）和 verify（执行后）；**execute 本身不写审计**。
- `before` = preview_result（改前预估），`after` = tool_result（改后实况）——可比对"批准的 vs 执行的"。
- 查询：`/agent/audit/{trace_id}`；管理台 `recent_audit_rows()`。
**被追问"success 怎么判"**：verify 全通过才 `success=true`；回读不一致降 false + `status=needs_review`。

## ⑥ RAG / 检索（ChatGPT 版几乎空白，重点补）
**叙事**：RAG 不是一条链打所有问题，而是**按场景路由**——不同问题走不同 Retriever。
**L3 血肉**（5 种 Retriever）：
| Retriever | 场景 | 数据源 |
|---|---|---|
| SQLRetriever | 订单号/机台号/规格历史 | SQLite（精确快） |
| VectorRetriever | 规则语义查询 | Qdrant + bge-small-zh |
| HybridRetriever | 规格/颜色强约束 | 向量+关键词 |
| CaseRetriever | 历史排产案例 | SQLite（可解释） |
| CompositeRetriever | "为什么排不下"诊断 | SQL + RAG |
- **query_plan 先判该不该检索**：`mode=structured_sql`(订单/机台直查，`requires_rag=false`) vs `rag`；**写操作跳过 RAG**避免无意义检索。
- 防幻觉断言：`answer_grounded` + `evidence_source=sqlite` + `response_must_not_contain`(订单不存在禁编机台/日期) + `max_retrieval_steps`(防无限检索)。
- Rerank：做过实验，**生产暂不默认开**（小规则库易被标题权重带偏，1000+条再引入）——诚实取舍点。
- **实测数字**（我跑 `bench_chunking` / `challenge_03` 得到，可复现）：Hybrid+RRF **Recall@3=0.83**；切块 **markdown MRR=0.956** 最优（一规则一块，语义边界干净）。**诚实短板**：所有切块策略 Negative Precision=0%——2 个"本不该命中"的 query 全被 BM25 误召回，因为 **BM25 不会说"无匹配"**，这正是要叠向量/阈值/`response_must_not_contain` 的理由。
**被追问"为什么不纯向量"**：订单号/机台/规格是强约束字段，纯向量把"语义相似"误当"业务正确"；所以精确走 SQL、规则走 Hybrid、诊断走 Composite。
**被追问"给个 RAG 数字"**：Recall@3=0.83、markdown 切块 MRR=0.956；复现命令 `python -X utf8 eval/bench_chunking.py`（BM25 基线，不依赖向量服务）。

### 🔎 RAG 刷题（排产版·30–45min 刷完，含实测数字）
> 深挖版(Agentic/Graph/RRF/父子块/间接注入)在 `rag-optimization-drills.zh.md`，可选、别挤必刷。

- **【必刷】你的 RAG 怎么设计的？**→ 不是一条链打所有问题，**按场景路由 5 种 Retriever**(SQL/Vector/Hybrid/Case/Composite)；`query_plan` **先判该不该检索**(结构化直查 SQLite / 语义走 rag)，**写操作跳过 RAG**。数字：Hybrid+RRF Recall@3=**0.83**。
- **【必刷】为什么不纯向量一把梭？**→ 订单号/机台/规格是**强约束字段**，纯向量把"语义相似"误当"业务正确"；所以精确走 SQL、规则走 Hybrid、诊断走 Composite。
- **【必刷】怎么防 RAG 幻觉 / 订单不存在还编出机台日期？**→ grounding **做在模型外**：`answer_grounded` + `evidence_source=sqlite` + `response_must_not_contain`(不存在禁编字段) + `max_retrieval_steps`(防无限检索)。
- **【数字题】切块策略怎么选？**→ bench 对比 4 种，**markdown 一规则一块 MRR=0.956 最优**(语义边界干净，优于 fixed/recursive)；**诚实短板**：全策略 Neg-Precision=0%，BM25 不会说"无匹配"——这正是要叠向量/阈值的理由。
- **【备弹】Rerank 为什么生产没默认开？**→ 小规则库易被标题权重带偏；规则库扩到 1000+ 条再引入 Rerank/Compress。

## ⑦ 可观测 / Tracing（叙事到 chain_id，L3 全缺）
**叙事**：每次请求一个 `trace_id`，从输入→意图→抽参→路由→查询→RAG→模型→工具→执行→回读→输出做节点级日志；出问题按 trace_id 还原链路定位是哪个节点。
**L3 血肉**：
- 三层数据模型：`Span`(name/trace_id/start·end/status[OK·ERROR·TIMEOUT]/attributes/error) → `Trace`(spans[]/is_success=all OK) → MetricsCollector/AnomalyDetector。
- **P95 不用 avg**：avg 掩盖长尾，P95 反映尾延迟=体感命门；**节点级 P95** 才能定位慢节点。
- 异常检测三类：延迟(节点历史 P95×2)、错误率(最近 window 错误率≥阈值)、成本(单请求超预算)。
- **诚实坑**：延迟基线用了含异常样本的全量数据→自指污染，生产要滑动基线；扁平 span 无 OTel 父子树。
- **实测数字**（我跑 `challenge_08` 得到）：端到端 **P50=941ms / P95=4702ms / avg=1480ms**；**节点级 LLM P95=4588ms vs Route P95=15ms**——一眼定位慢节点是 LLM 不是 route；成功率 90%、50 请求总成本 ¥0.0774。
**被追问"给个数字/怎么定位慢节点"**：端到端 P95=4702ms，拆到节点级 LLM P95=4588ms 就知道锅在 LLM；复现 `python -X utf8 interview_challenges/solutions/solution_08.py`。

## ⑧ 评估体系（叙事维度全，补 L3 机制）
**叙事**：不能只看一个准确率，要**自动化指标 + 人工指标**双建。自动指标定位链路问题，人工指标判断业务价值。人工指标拆细（对交期满意还是对机台推荐满意）。
**L3 血肉**：
- 三层防护：`audit`(skill 健康，**5 检查**：dark/conflict/self_recog/coverage/orphan) → `check_expected`(**~16 类确定性断言**：intent_code/path/governance_passed/max_retrieval_steps/response_must_not_contain) → `judge`(LLM **3 维 1-5**，可降级规则)。
- CI 三 job：audit → rule-eval(无 LLM,每次) ∥ full-eval(含 judge,定时)；门禁 `rule_pass_rate≥1.0`。
- **数据飞轮**：线上 `audit_log` → `mine_eval_candidates` → 人审 → `online_hard_cases.json` → `run_scheduled_eval`。
**被追问"judge 打不准怎么办"**：留人工校准子集验 judge-human 一致率，<0.9 先修 judge；能转确定性断言的不用 judge。

## ⑨ 记忆 / 上下文（叙事有雏形，补 L3）
**叙事**：维护多轮任务状态（订单号/机台号/目标机台/规格/DryRun 结果/确认状态），用任务级摘要 + 结构化状态压缩上下文。
**L3 血肉**：指代消解靠"最近 N 轮原文"不靠 RAG（时序邻近 ≠ 语义相似）；历史压缩=滑窗 + 摘要；结构化事实走 DB 现查不进向量库。
**被追问"'把它排到101'怎么知道'它'是谁"**：靠会话记忆里最近 N 轮的原文做指代消解，不靠向量召回（会把语义像但时间无关的历史块拉进来误导）。

## ⑩ 驾驭工程(元) + 长期治理（叙事很强，保留）
**叙事**：Agent 工程核心是**驾驭大模型的概率性输出**，放进可执行/可观测/可评估/可测试的系统。控制手段：结构化输出、关键参数过规则校验、写操作五段(权限/DryRun/确认/Execute/Verify)、低置信澄清不猜。长期治理才是真难点：规则沉淀/边界 case 收集/人工标注/评估集扩展/线上回流/多角色协作，周期长。
**L3 血肉**：这条对应四信条——LLM 原始输出不直接落地 / 安全建在代码不建在措辞 / 有确定性判据就别用 LLM judge / 数据不出厂(本地 Qwen)。

---

## ⑪ 并发处理（真做了，不是缺口）
**叙事**：同一订单可能多人同时改，我用**读写分离 + 订单级写锁**兜——读多不锁，只锁同订单的写。
**L3 血肉**：①路由前查询阶段加锁；②意图澄清后判读写——**读操作直接放过(不锁)**；③**写操作**先查该订单当前有没有 `trace_id` 日志在写；④有在写→返回"**有其他人正在操作该订单，请稍后重试**"。本质=用 audit/trace 日志做**订单级悲观写锁**：读多不锁、只锁同订单写、不同订单不互斥。系统压力另叠令牌桶限流 + 队列削峰 + 本地模型 Semaphore 上限。
**被追问"这个锁的坑"**：①check"有没有在写"与"标记我在写"要**原子**，否则 TOCTOU（两请求同时检查都发现没锁）——单实例 asyncio 临界区内安全，跨实例要 DB 唯一约束 / Redis SETNX；②写崩溃/超时要 **TTL/超时释放**否则订单锁死；③多实例把锁移到**共享存储**；④叠幂等键防重试双写。

## ⑫ 意图扩展 / 为什么 LangGraph / 1.0 新特性（高频硬追问）
**意图类型超 10 个怎么扩展**：意图是 **config-driven**（`configs/intents.json` 加一类=加配置不改核心代码），扩展靠 `audit` 的 **5 检查**防退化（dark/conflict/self_recog/coverage/orphan）——加了不冲突、有覆盖、不产生孤儿。优化方向：规则低置信时才调**大模型前置兜底** + embedding 语义匹配 + 对未命中样本**聚类挖新意图**。
**为什么用 LangGraph（不只是节点编排）**：不是"只能画工作流"——用的是 **① 图状态(TypedDict State 贯穿节点) ② 条件边(after_route/after_governance/after_preview 做确定性分支) ③ interrupt/HITL(confirmed 落点) ④ cycles(need_clarify 回环) ⑤ 可加 checkpointer 做 durable/time-travel**。确定性 workflow 骨架正好压制 LLM 的不确定。
**LangGraph 1.0 新特性（被问最新版）**：durable execution + 多后端 checkpointer（InMemory/Postgres/SQLite/**DynamoDB+S3**）+ Store(跨会话长期记忆) + **node caching**(CachePolicy ttl 缓存检索) + **deferred nodes**(等多分支到齐再跑，做 best-of-N 聚合) + pre/post model hooks。

## ⑬ 下一步优化：Bad-case 回流飞轮 + 优化速查
**Bad-case 回流飞轮（下一步主线）**：①从线上可观测日志提取 **Bad Request** 抽成待评估用例 → ②**分类**：响应请求异常 vs 业务理解异常 → ③业务理解异常由**业务人员人工标注** → 确定下一轮需求 → ④深化成**新 golden Case**。**关键**：响应异常有确定性信号(超时/500/schema失败/工具error/needs_review)→**自动走工程线修**、不麻烦业务；**只把业务理解异常送人标**（省标注成本，确定性优先）；业务理解异常再分"**模型理解错**(prompt/意图/抽取→工程修) vs **规则缺失过时**(需求变更→下一轮需求)"。映射真实代码：audit_log `reason_codes`→`mine_eval_candidates`→`eval_candidate_case`→人审→`online_hard_cases.json`→`run_scheduled_eval`(min_pass_rate=1.0)。
**优化速查（被问"怎么优化/下一步"，答法=现状能用 + 有 X 局限 + 方向 Y + 当前不做因为 Z）**：
- **调度优化能力**（1.0 最大缺口）：现规则+查询无 solver → 接排程 solver/OR-Tools 做最优化，LLM 退成 NL→约束翻译官。
- **RAG 准确性**：Recall@3=0.83 → 规则库 1000+ 开 Rerank + 父子块 + 诊断类 Graph RAG 多跳 + 垂域 embedding 微调。
- **可观测**：扁平 span + 自指基线 → OTel 父子树 + 滑动基线 + SLO error budget burn rate 告警。
- **评估**：golden+judge → 线上人审改判率反哺离线维度权重 + held-out 防过拟合 + 端到端多轮回放。
- **记忆**：滑窗+摘要 → 三层记忆(DB/事件源/向量) + 长期偏好 + 租户隔离。

---

## 🧪 8 个技术硬挑战（证明"真做过"，各配一句 + 数字）
| 模块 | 挑战 | 一句话 |
|---|---|---|
| RAG | PDF 分块 | layout 分区 + 跨页表格合并 + 语义切块 |
| RAG | 脏 Excel 解析 | 启发式表头检测 + 合并单元格 + 列名映射 |
| RAG | Hybrid+Rerank | BM25 + 向量 + **RRF 融合**（rank 融合免量纲不可比） |
| LM | 熔断+多模型降级 | 熔断三态机 + 指数退避 + 降级链 |
| LM | 输出格式保障 | JSON 鲁棒解析 + schema 校验 + 重试 |
| 记忆 | 多轮指代消解 | 实体提取 + 指代类型分类 + 消解策略 |
| 记忆 | 历史压缩 | Token 估算 + 滑窗 + 摘要压缩 |
| 可观测 | Tracing | Span/Trace 模型 + P50/P95 + 慢节点/异常检测 |
> 面试策略：每个能拿出①能跑代码 ②设计权衡 ③指标数字（Recall=0.83 / P95=4702ms）。

## 🎯 与 nanocodex 的定位（选主讲用）
- **排产**（业务/治理复杂度高，已上线，接手改造）→ 偏企业 AI 落地/业务系统/安全治理岗位主讲。
- **nanocodex**（工程/系统复杂度高，从零自研）→ 偏基建/框架/平台岗位主讲。
- 串讲主叙事：**同一套驾驭工程，一个长成"确定性 workflow + 四段治理 + 评测飞轮"(排产)，一个长成"沙箱 + best-of-N + 离线演化"(nanocodex)；骨架同，承重随业务。**

---

## 一句话总纲（背这句）
> 已上线的制造业排产 1.0 最小可行闭环：不覆盖全量工厂规则，但验证了 Agent 在复杂制造业场景里做自然语言理解、规则匹配、流程编排、权限管控、DryRun、安全防控、结果反馈、可观测和评估治理的可行性；核心是**规则在前模型在后、写操作四段治理、评测飞轮长期治理**；难点不是换模型，是规则沉淀/边界数据/人工标注/线上回流/多角色协作的体系化建设。
