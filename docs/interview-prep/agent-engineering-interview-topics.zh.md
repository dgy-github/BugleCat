# Agent 工程面试题库（通用·工程定义标准版）

> 用途：这份题库覆盖 AI/LLM Agent 工程里真正高频、且有行业/学术标准定义支撑的问题——不是黑话堆砌，每类先给一个经真实来源核对过的工程定义，再给高频问题。

> **怎么用**：每道题按 `先讲这类问题的标准工程定义（显得懂行业共识） -> 再用 nanocodex 里的真实实现具体化（显得有工程落地） -> 被追问再下钻` 的顺序作答。有 `对应 nanocodex` 标注的题，去读 [nanocodex-portrait.zh.md](nanocodex-portrait.zh.md) 对应子系统卡拿到代码级细节；`架构范式` 和 `多 Agent 通讯` 两类分别是你已有的「Harness 工程管理」「主子 agent 如何通讯」两张卡的通用母题（见下方提示）。

> 每类结构：工程定义（含真实来源）+ 高频问题（tier / 考察点 / 参考答案要点 / 追问 / 对应 nanocodex）。代码/协议/论文标识符保留英文，叙述用中文。

## 目录

1. Agent 架构范式与控制流
2. 多 Agent 协作与通讯协议
3. 工具使用工程化 / Function Calling
4. 上下文工程与记忆系统
5. 安全护栏与执行沙箱
6. 可靠性、成本与延迟工程
7. 评估、可观测性与持续改进
8. 用法建议

---

## 1. Agent 架构范式与控制流

> **提示**：这一类是「Harness 工程管理」的通用母题——你已有的 [01_harness.md](portrait_parts/01_harness.md) 是它在 nanocodex 里的深度、代码级实现；先讲这里的标准范式定位，再用那张卡的细节落地。

**工程定义**：Agent 的『控制流』本质上是在回答同一个问题的不同侧面——每一步该谁来决定『下一步做什么』、决定依据是什么、什么时候停。ReAct(Yao et al., arXiv:2210.03629, ICLR 2023)给出了最基础的单步控制流原语:把 Thought(推理)、Action(工具调用)、Observation(环境反馈)交替串成一个循环,让模型在每一步都能被外部世界的真实观察修正,而不是像纯 Chain-of-Thought 那样完全依赖内部知识连续推理;论文在 HotpotQA/FEVER 上验证了这能抑制幻觉累积,并在 ALFWorld/WebShop 两个交互式决策基准上分别以 34%/10% 的绝对成功率优势超过模仿学习与强化学习基线。Plan-and-Execute 把这个单一循环拆成两个角色:一个 Planner 一次性/阶段性产出有序的步骤列表,一个 Executor(通常本身是一个 ReAct 风格的子循环)逐步执行——其学术源头是 Plan-and-Solve Prompting(Wang et al., ACL 2023, arXiv:2305.04091),该论文把 zero-shot CoT 的触发语从『Let's think step by step』换成『先制定计划再按计划执行』以消除 missing-step 错误;LangChain 在 2023 年将其工程化为 Plan-and-Execute Agents,并在官方博客中说明该设计同时受 BabyAGI 与 Plan-and-Solve 论文启发,代价是调用次数显著增加,换来的是规划与执行的解耦(可用不同能力/成本的模型分别承担两个角色)。Reflexion(Shinn et al., NeurIPS 2023, arXiv:2303.11366)解决的是跨试次(trial)的控制流:它不更新模型权重,而是让 agent 对失败信号生成自然语言自我反思,写入一个持续积累的 episodic memory 缓冲区并在下次尝试时注入上下文——这是一种『verbal reinforcement learning』;论文的消融实验显示,收益主要来自『自我反思』这一结构化归纳步骤本身,而不只是保留更多原始历史,在 HumanEval 上达到 91% pass@1(同期 GPT-4 直接推理为 80%),在 AlfWorld/HotPotQA 上相比基线分别提升约 22%/20%。在架构层面,Anthropic 的《Building effective agents》(Erik Schluntz、Barry Zhang,2024 年 12 月)把上述控制流统一归入『agentic systems』大类下的两个子类:workflow 指 LLM 与工具按预先写死的代码路径被编排(prompt chaining、routing、parallelization、orchestrator-workers、evaluator-optimizer 五种模式均属此类),agent 指 LLM 在运行时动态决定自己的执行路径与工具使用、每一步都从环境获取 ground truth 来评估进展,建议优先用最简单的 workflow,只有任务步骤数无法预先枚举时才升级为开放式 agent 循环。在『单 agent vs 多 agent』的选型上,Anthropic 的《How we built our multi-agent research system》(2025 年 6 月)给出的实测是:以 Claude Opus 4 为 lead agent、Claude Sonnet 4 为 subagent 的多 agent 系统在其内部研究评测(internal research eval)上比单一 Opus 4 agent 的任务表现高出 90.2%——这是准确率/完成质量的提升而非耗时的缩短;文章进一步用 BrowseComp 评测做归因,token 用量、工具调用次数与模型选择三者共同解释约 95% 的表现方差,其中 token 用量单独就能解释约 80%,说明多 agent 的收益很大程度上来自更大的并行探索预算而非架构本身的额外智能;但代价同样明确——agent 通常消耗约 4 倍于普通 chat 的 token,multi-agent 系统约消耗 15 倍,且在紧耦合、依赖共享上下文连续性的任务(文章明确举例 coding)上,多 agent 因子 agent 间上下文隔离反而表现更差。

**来源**：
- [[2210.03629] ReAct: Synergizing Reasoning and Acting in Language Models](https://arxiv.org/abs/2210.03629)
- [[2305.04091] Plan-and-Solve Prompting: Improving Zero-Shot Chain-of-Thought Reasoning by Large Language Models](https://arxiv.org/abs/2305.04091)
- [Plan-and-Execute Agents - LangChain Blog](https://www.langchain.com/blog/plan-and-execute-agents)
- [[2303.11366] Reflexion: Language Agents with Verbal Reinforcement Learning](https://arxiv.org/abs/2303.11366)
- [Building effective agents \ Anthropic](https://www.anthropic.com/research/building-effective-agents)
- [How we built our multi-agent research system \ Anthropic](https://www.anthropic.com/engineering/multi-agent-research-system)

### [L1 理解] ReAct 范式的核心控制流是什么?它相比纯 Chain-of-Thought 解决了什么问题?

- **考察点**：考察候选人是否理解几乎所有现代 agent 工具调用循环的理论起点,而不是把'能调用工具'和'ReAct'画等号。
- **参考答案要点**：
  - Thought→Action→Observation 交替循环:模型先推理、再产生一个 Action(如调用外部工具/检索),把真实的 Observation 喂回下一步推理,直到产生 finish 动作终止(Yao et al. 2022, arXiv:2210.03629, ICLR 2023)
  - 纯 CoT 的推理链完全依赖模型内部知识连续展开,容易产生幻觉且无法自我纠正;ReAct 把每一步的真实环境反馈接入推理链,用外部 ground truth 打断幻觉累积
  - 论文在 HotpotQA/FEVER 上 ReAct 与 CoT 表现相当但更少幻觉,且在 ALFWorld/WebShop 两个交互式决策任务上分别以 34%/10% 的绝对成功率超过模仿学习和强化学习基线
  - 关键工程含义:控制流不是'一次性想完再做',而是'边想边做、每步都被环境校正'
- **追问**：
  - ReAct 在长任务里容易出现什么退化模式(如重复调用同一工具却不终止)?你会怎么在工程上抑制?
  - 如果去掉显式 Thought、只保留 Action→Observation,会丢失什么能力?
- **对应 nanocodex**：Harness 工程管理 — AgentLoop::run_turn 的回合循环(call-model 产出下一步意图 → run-tools 得到 Observation → 进入下一回合)是 ReAct 循环在生产 harness 里的具体化,并用双预算 max_model_calls=60/max_tool_calls=120 给这个本可无限循环的控制流加终止保证。卡:01_harness.md

### [L1 理解] Anthropic 在《Building effective agents》里怎么区分 workflow 和 agent?这个区分对架构选型有什么指导意义?

- **考察点**：考察候选人是否理解'workflow'和'agent'不是营销词而是有清晰工程边界的两类系统,避免把任何带循环的代码都笼统称为'agent'。
- **参考答案要点**：
  - agentic systems 是总称;workflow 指 LLM 与工具按预定义、硬编码的代码路径被编排(prompt chaining、routing、parallelization、orchestrator-workers、evaluator-optimizer 五种模式);agent 指 LLM 在运行时动态决定自己的执行路径与工具使用,循环从环境获取 ground truth(工具结果/代码执行)评估进展,直到任务完成或需要人工介入
  - 核心建议是'从最简单方案开始':很多时候单次 LLM 调用或固定 workflow 就足够,只有任务步骤数无法预先枚举、需要模型自主决策路径时才值得升级到开放式 agent 循环
  - 权衡:workflow 更可预测、可测、成本可控;agent 自主性更强但意味着更高成本、更难预测的行为和错误累积风险,需要建立可信的执行环境
  - 该文章(Erik Schluntz、Barry Zhang,2024 年 12 月)也警告不要把框架当作起点,框架会遮蔽底层真实的 prompt 和 response,建议先用底层 API 理解清楚控制流再决定是否引入框架
- **追问**：
  - orchestrator-workers 这个 workflow 模式和'多 agent 系统'有什么本质区别,还是只是叫法不同?
  - 如果一个系统的'agent 循环'其实每次都严格按固定步骤执行,它还能被称为 agent 吗?
- **对应 nanocodex**：分层编排器(orchestrator flash/pro) — orchestrator.rs 的 classify/plan/decompose/workers/verify 节点图本身是一个'预定义代码路径编排 LLM 调用'的 workflow,但它编排的每个 worker 节点内部又是一个完整的开放式 AgentLoop——是 workflow 套 agent 的具体分层实现。卡:05_orchestrator_comm.md

### [L2 权衡] 对比 Plan-and-Execute 和 ReAct 两种控制流架构,说明各自的优劣和适用场景。

- **考察点**：考察候选人能否区分'隐式逐步展开的规划'和'显式全局计划'两种范式,而不是把所有带循环的 agent 都当成同一种东西。
- **参考答案要点**：
  - ReAct('Action agent'):单个模型在一个循环里交替做推理和行动,规划是隐式的、逐步展开的,没有独立的全局计划
  - Plan-and-Execute:显式拆成 Planner(一次性/阶段性产出有序步骤列表)与 Executor(逐步执行,通常本身是一个 ReAct/Action 风格的子 agent)两个角色;学术源头是 Plan-and-Solve Prompting(Wang et al., ACL 2023, arXiv:2305.04091);LangChain 2023 年将其工程化为 Plan-and-Execute Agents,并声明设计明确受 BabyAGI 和 Plan-and-Solve 论文启发
  - 优势权衡:Plan-and-Execute 把'规划'和'执行'解耦,规划可用更强模型、执行可用更小更快模型,且全局计划更易审计/中途修改;代价是调用次数显著增多(LangChain 原博客明确指出这是主要缺点)
  - 适用场景:任务步骤在开始时大致可预见、需要可解释/可审核的全局计划时选 Plan-and-Execute;任务需要频繁根据环境反馈调整策略、步骤数无法预知时 ReAct 的逐步交替更合适;两者也可叠加(先 Plan-and-Execute 定框架,每个子步骤内部再跑 ReAct)
- **追问**：
  - 如果 Planner 一开始制定的计划中途被环境证伪(某一步的前提不成立),Plan-and-Execute 架构要如何重新规划?
  - LangChain 后来用 LangGraph 图结构重做了 Plan-and-Execute,相比最初的链式实现解决了什么问题?
- **对应 nanocodex**：分层编排器(orchestrator flash/pro) — orchestrator.rs 对 Complexity::Medium/High 任务先跑 plan 阶段产出子任务列表,再 fan-out 给多个 worker(Executor)执行、由 verify 节点裁决,是 Plan-and-Execute 范式在生产编排器里的具体实现,而每个 worker 内部的 AgentRunner 会话仍是逐步交替的 ReAct 式循环。卡:05_orchestrator_comm.md

### [L2 权衡] Reflexion 是怎么让 agent'从失败中学习'的?它和单纯把历史对话/日志拼接进 context 有什么本质区别?

- **考察点**：考察候选人是否理解 Reflexion 的核心机制是'结构化自我反思'而非'更长的记忆窗口',这是区分'真正学习型 agent'与'只是塞更多上下文'的关键点。
- **参考答案要点**：
  - Reflexion(Shinn et al., NeurIPS 2023, arXiv:2303.11366)不更新模型权重,而是让 agent 对失败信号(如单元测试报错、环境反馈)生成一段自然语言自我反思,写入一个持续积累的 episodic memory 缓冲区,在下一次尝试时作为额外上下文注入——本质是用语言而非梯度做强化学习('verbal reinforcement learning')
  - 关键消融实验:论文证明收益主要来自'自我反思'这一步本身,而不只是'保留了更多历史记录';去掉反思生成、只让模型看原始失败轨迹重试时效果没有提升,说明单纯拼接日志/history 和 Reflexion 的结构化反思+归纳是两回事
  - 效果数据:HumanEval pass@1 达到 91%(同期 GPT-4 直接推理为 80%);在 AlfWorld、HotPotQA 上相比基线分别提升约 22%、20%
  - 工程含义:要复刻 Reflexion,不能只是'把上一轮输出塞进 prompt',而要有一个显式步骤让模型对失败做归因/总结,并且这个总结要被结构化沉淀(而非原始日志)供后续复用
- **追问**：
  - Reflexion 的 episodic memory 缓冲区如果无限增长,会带来什么问题?你会怎么设计淘汰/压缩策略?
  - Reflexion 依赖外部可验证的反馈信号(如单元测试),如果任务没有这种明确的对错判据,这个机制还成立吗?
- **对应 nanocodex**：项目记忆 自进化 — memory.rs 的 remember/recall(混合词法语义打分的写入与召回)与 consolidate(Jaccard 阈值去重折叠旧条目)是'把经验结构化沉淀+后续复用'这一思想在长期项目记忆而非单次任务内的对应实现,但 consolidate 做的是相似条目合并,并非 Reflexion 论文里针对失败原因的语言归因总结,两者不能等同。卡:06_memory.md

### [L3 深挖/故障] Anthropic 的 orchestrator-worker 研究系统实测比单 agent 效果提升明显,但也提到 token 消耗更高。你会依据什么标准判断一个任务该用多 agent 还是单 agent?

- **考察点**：考察成本/收益权衡意识,防止候选人一味'堆 agent'而不考虑代价与适用边界——是面试官区分'纸上谈兵'与'真正上过生产'候选人的常见问法。
- **参考答案要点**：
  - Anthropic 内部研究评测(internal research eval)显示,以 Claude Opus 4 为 lead agent、Claude Sonnet 4 为 subagent 的多 agent 系统,比单一 Opus 4 agent 的任务表现(评测得分)高出 90.2%——这是准确率/完成质量的提升,不是耗时的缩短(《How we built our multi-agent research system》, Anthropic, 2025)
  - 文章同时用 BrowseComp 评测做归因分析:token 用量、工具调用次数、模型选择三个因子共同解释约 95% 的表现方差,其中仅 token 用量一项就能解释约 80%——多 agent 的收益很大程度上来自更大的并行探索预算,而非架构本身带来额外智能
  - 成本代价:文章给出的经验数字是 agent(单 agent 工具循环)通常消耗约 4 倍于一次普通 chat 的 token,multi-agent 系统约消耗 15 倍——多 agent 不是免费的性能提升
  - 明确的反例:文章指出多 agent 编排在紧耦合、依赖共享上下文连续性的任务(原文举例 coding)上效果反而更差,因为子 agent 间上下文隔离带来协调开销和信息丢失;早期版本还出现了为简单查询派生过多 subagent、无限抓取网页等失控行为,靠 prompt engineering 才收敛
  - 决策依据:任务能否拆成可并行、弱耦合的独立子任务 + 结果价值能否覆盖 4x~15x 的 token 成本 + 是否需要跨子任务的强状态一致性(强一致性/紧耦合场景优先单 agent)
- **追问**：
  - 如果一个任务看起来可以并行分解,但子任务之间其实存在隐藏的隐式依赖,盲目上多 agent 会引入什么新故障模式?
  - 'token usage 解释约 80% 方差'这个结论,除了判断'要不要多 agent',还能怎么用来做资源分配决策?
- **对应 nanocodex**：分层编排器(orchestrator flash/pro) — orchestrator.rs 用 best-of-N worker fanout(join_all 并行)在 Complexity::High 场景放大探索路径以换取质量,同时用 tool-stripped reason() 节点和固定 worker Tier::Fast 控制成本,是同一条'质量 vs token 成本'权衡曲线在真实代码里的取舍点。卡:05_orchestrator_comm.md

### [L4 开放设计] 如果要为一个开放式 agent 循环设计'控制流的工程化护栏'(不依赖模型自己决定何时停),你会围绕哪些机制展开?

- **考察点**：综合考察候选人能否跳出'选哪种推理范式(ReAct/Plan-and-Execute/Reflexion)'这一层,进入'循环本身会不会跑飞、卡死、永久挂起'这一更底层的 harness 工程问题——是本主题'控制流是怎么被工程化管理的'这一母题的压轴题。
- **参考答案要点**：
  - 终止保证:双维度预算(如上限模型调用次数、上限工具调用次数)而非单一维度,因为模型可能'多想少做'或'多做少想',单一预算容易被绕过
  - 取消与中断:协作式取消而非强杀进程,在关键节流点做低延迟轮询,保证用户中断能在可预期时间内生效,同时不破坏已产生的对话历史完整性
  - 历史一致性:模型 API 对'工具调用后必须有对应工具结果'这类历史合法性有硬性要求,当预算耗尽或被取消导致工具调用没跑完时,必须有回填(backfill)机制补齐占位结果,否则下一次请求会被 API 直接拒绝
  - 成本/延迟优化而不牺牲正确性:对只读、无副作用的工具做并发批量执行,对有副作用的工具保持串行,这是控制流工程里'安全的并行化边界'判断
  - 上下文/工具可见性动态裁剪:回合内根据当前查询动态选择一个小而相关的工具子集暴露给模型,而不是把全部工具 schema 都塞进 prompt,既降低成本又降低模型选错工具的概率,这本身也是控制流的一部分——决定模型在每一步能看到、能做什么
  - 这套护栏与'选 ReAct/Plan-and-Execute/Reflexion 哪种推理范式'是正交的两层问题:上层选范式解决'怎么想、怎么规划',下层护栏解决'循环会不会跑飞、会不会因为一次异常而永久挂起'
- **追问**：
  - 如果双预算里工具调用次数先耗尽但模型还想继续推理,你会让循环立刻终止,还是允许'只想不做'再跑几轮?为什么?
  - 动态工具可见性裁剪如果裁剪错了(该在的工具没露出来),要怎么在 trace/日志里快速定位到是裁剪逻辑的问题而不是模型的问题?
- **对应 nanocodex**：Harness 工程管理 — AgentLoop::run_turn 的回合循环(call-model/run-tools 交替)、双预算(max_model_calls=60/max_tool_calls=120)保证终止、两层协作式取消(100ms select!)、只读并发批量(parallel_run)、每回合动态 tool-schema 选择(DEFAULT_VISIBLE_TOOL_LIMIT=9)、API 历史合法性 backfill(backfill_unanswered_tool_calls)——这些正是这道设计题里'护栏机制'的真实落地实现。卡:01_harness.md

---

## 2. 多 Agent 协作与通讯协议

> **提示**：这一类是「主子 agent 如何通讯」的通用母题——你已有的 [05_orchestrator_comm.md](portrait_parts/05_orchestrator_comm.md) 是它在 nanocodex 里的深度、代码级实现；先讲这里的标准范式定位，再用那张卡的细节落地。

**工程定义**：多 agent 协作系统在工程上通常被归纳为三类拓扑:hierarchical/supervisor-worker(中心 supervisor 节点做路由决策、worker 之间不直接对话——例如 LangGraph 的 create_supervisor 用 handoff tool 把控制权转移与状态更新打包成一个 Command 对象转交给目标 agent)、blackboard(多个 knowledge source 互不寻址,而是通过读写一块共享数据结构——blackboard——间接协作,由独立的 control 组件决定下一个被激活的 knowledge source;该模式最早由 Erman、Hayes-Roth、Lesser、Reddy 在 Hearsay-II 语音理解系统中提出,发表于 ACM Computing Surveys 1980)、以及 peer-to-peer/network(agent 间 many-to-many 直接通信、无固定层级)。要判断某个系统是否具备真正意义上的 agent-to-agent「消息传递」,关键看它是否具备(a)独立可寻址的 agent 身份 (b)去中心化的路由决策 (c)不必经过某个中心可变状态即可直接投递的通道——AutoGen 提出的 conversable agent 抽象(通过 send/receive/generate_reply 做消息传递,复杂场景下用 GroupChatManager 做动态 speaker selection + broadcast)是较接近该定义的实现;而多数生产系统,包括 supervisor-worker 与 blackboard 模式,本质上是「共享/编排状态 + 中心节点决策」被包装出了「agent 对话」的外观,并不满足上述三条中的全部。在协议分层上,MCP(Model Context Protocol,Anthropic 于 2024 年 11 月发布,现由 Linux Foundation 托管)解决的是 agent 到工具/数据源的标准化接入问题:定义 host/client/server 三层架构,一个 client 与一个 server 建立有状态的 JSON-RPC 会话,通过 tools/resources/prompts 三种原语的 list/get/call 方法暴露能力,被调用方在语义上被假定为受控、无状态的「工具」;而 A2A(Agent2Agent Protocol,Google 于 2025 年 4 月发布,同样已转交 Linux Foundation 治理)解决的是 agent 到 agent 的互操作问题:agent 通过 Agent Card(JSON)广播自身能力,client agent 发现并调用 remote agent 去完成一个拥有独立生命周期的 task 对象,基于 HTTP、Server-Sent Events 与 JSON-RPC 2.0 传输。行业共识将二者视为互补而非竞争的两层:MCP 回答『一个 agent 如何调用工具』,A2A 回答『一个 agent 如何发现并委托给另一个具备自主规划能力的 agent』。

**来源**：
- [Specification - Model Context Protocol](https://modelcontextprotocol.io/specification/2025-11-25)
- [Introducing the Model Context Protocol \ Anthropic](https://www.anthropic.com/news/model-context-protocol)
- [Announcing the Agent2Agent Protocol (A2A) - Google Developers Blog](https://developers.googleblog.com/en/a2a-a-new-era-of-agent-interoperability/)
- [How we built our multi-agent research system \ Anthropic](https://www.anthropic.com/engineering/multi-agent-research-system)
- [AutoGen: Enabling Next-Gen LLM Applications via Multi-Agent Conversation Framework (arXiv:2308.08155)](https://arxiv.org/abs/2308.08155)
- [GitHub - langchain-ai/langgraph-supervisor-py](https://github.com/langchain-ai/langgraph-supervisor-py)
- [Erman, Hayes-Roth, Lesser, Reddy — The Hearsay-II Speech-Understanding System: Integrating Knowledge to Resolve Uncertainty, ACM Computing Surveys 12(2), 1980](https://dl.acm.org/doi/10.1145/356810.356816)

### [L1 理解] 请把 multi-agent 系统的常见拓扑分成 hierarchical(supervisor-worker)、blackboard、peer-to-peer 三类,分别说明它们的信息流向和谁来做路由决策。

- **考察点**：考察候选人是否具备清晰的架构分类心智模型,而不是笼统地说'多个 agent 互相调用';这是回答任何'主子 agent 怎么通讯'问题前必须先澄清的坐标系。
- **参考答案要点**：
  - hierarchical/supervisor-worker:中心 supervisor 做路由决策,worker 之间不直接通信,如 LangGraph 的 create_supervisor 用 handoff tool 返回 Command 对象把控制权+状态更新一起转交给目标 agent
  - blackboard:没有中心控制流,knowledge source 通过读写共享数据结构间接协作,起源于 1970s 的 Hearsay-II 语音理解系统(Erman et al., ACM Computing Surveys 1980),适合 ill-structured、可增量拼接的问题
  - peer-to-peer/network:agent 间 many-to-many 直接通信,无固定层级,LangGraph 里对应 network 模式
  - 三者的核心区别在于'谁做决策'与'信息通过什么中介传递'——直接寻址消息 vs 共享可变状态
- **追问**：
  - 现实系统里这三种模式经常混合使用,能举一个具体例子吗?
  - 为什么生产系统更常采用 supervisor 模式而不是纯 peer-to-peer?
- **对应 nanocodex**：分层编排器(orchestrator flash/pro) + 主子agent通讯 — orchestrator.rs 的 classify/plan/decompose/workers/verify 节点图整体就是 hierarchical/supervisor-worker 拓扑的具体实现。卡:05_orchestrator_comm.md

### [L2 权衡] 'agent-to-agent 通讯'到底是真正的消息传递(message passing),还是共享状态+中心编排在伪装成'对话'?请结合具体框架的实现机制回答。

- **考察点**：这是'主子 agent 如何通讯'这类问题的母题:考察候选人能否戳穿'多 agent 对话'的表象,理解底层真实的数据流机制,而不是把'调用了另一个 agent'等同于'两个 agent 在通讯'。
- **参考答案要点**：
  - 判定标准:是否有独立可寻址的 agent 身份、是否有去中心化的路由决策、信息是否不必经过某个中心可变状态就能直接投递
  - AutoGen 的 conversable agent 抽象(send/receive/generate_reply)加 GroupChatManager 的动态 speaker selection + broadcast 是较接近'真消息传递'的实现,但消息仍经由一个中心 manager 广播,并非完全去中心化
  - LangGraph supervisor 模式的 handoff 本质是'状态更新+控制流转移'(Command 对象),子 agent 之间看不到彼此私有会话历史,不构成独立寻址的消息通道
  - blackboard 模式彻底不是消息传递:agent 互不寻址,只读写共享数据结构,由 control 组件决定谁被激活
  - 很多所谓'多 agent 对话'系统,底层是父进程把子 agent 输出重新序列化后当 prompt 灌入下一个子 agent——子 agent 没有独立寻址/路由能力,本质是编排,不是通讯
  - 结论:多数生产系统是'共享/编排状态 + 中心节点决策'被包装成了'通讯'的外观,真正的去中心化 message passing 较少见
- **追问**：
  - A2A 协议是否让'真正的' agent 间消息传递成为可能?它和 AutoGen 的 message passing 有什么本质区别?
  - 如果要设计一个可验证的测试来证明某系统是'真' p2p 而非编排,你会怎么设计?
- **对应 nanocodex**：分层编排器(orchestrator flash/pro) + 主子agent通讯 — orchestrator.rs 的 AgentRunner trait 让子节点是全新无状态会话，没有消息总线：下行靠 prompt 文本按值序列化，代码改动的真实 IPC 是文件系统隔离+promote_worker，回路靠 PASS/FAIL/BEST:<n> 裁决文本，是'共享状态+编排'而非真消息传递的具体反例。卡:05_orchestrator_comm.md

### [L1 理解] MCP 和 A2A 分别解决什么问题?两者的定位区别是什么,能不能只用其中一个协议搭建完整的多 agent 系统?

- **考察点**：考察候选人是否理解协议分层定位,而不是把'工具协议'和'agent 间协议'混为一谈——这是围绕多 agent 通讯讨论时最常见的概念混淆点。
- **参考答案要点**：
  - MCP(Model Context Protocol,Anthropic 2024 年 11 月发布,现由 Linux Foundation 托管)解决 agent 到工具/数据源的标准化接入:host/client/server 三层架构,client 与 server 一对一建立 JSON-RPC 会话,通过 tools/resources/prompts 原语的 list/get/call 暴露能力
  - A2A(Agent2Agent,Google 2025 年 4 月发布,现由 Linux Foundation 托管)解决 agent 到 agent 的互操作:agent 用 Agent Card(JSON)广播能力,client agent 发现并调用 remote agent 完成一个有生命周期的 task 对象,基于 HTTP+SSE+JSON-RPC 2.0 传输
  - 两者互补而非竞争:一个 agent 可以同时是 MCP host(调用工具)和 A2A 的 client/remote agent(与其他 agent 协作)
  - 关键区别:MCP 假设被调用方是受控、无状态、能力单一的'工具';A2A 假设被调用方是有自主规划能力、task 可跨多轮存在的'agent'
- **追问**：
  - 如果一个'MCP server'内部本身又跑了一整套 LLM agent 循环,这是否破坏了 MCP 的语义假设?
  - A2A 的 Agent Card 和 MCP 的 tool schema 在设计目的上有什么本质不同?
- **对应 nanocodex**：Skills · MCP · 视觉 — mcp_tool.rs + ncx-mcp 的 stdio JSON-RPC bridge 实现了 MCP client 侧角色，可以据此对照 A2A 做定位区分。卡:07_skills.md

### [L2 权衡] Anthropic 的 orchestrator-worker 研究系统实测比单 agent 效果提升明显,但也提到 token 消耗约为单 agent 的 15 倍。你会依据什么标准判断一个任务该用多 agent 还是单 agent?

- **考察点**：考察成本/收益权衡意识,防止候选人一味'堆 agent'而不考虑代价——这是面试官区分'纸上谈兵'与'真正上过生产'候选人的常见问法。
- **参考答案要点**：
  - Anthropic 实测:Opus lead + Sonnet subagents 的多 agent 系统比单 agent Opus 在其内部研究评测上高出 90.2%,但 token 消耗约为单 agent chat 的 15 倍
  - 'token usage 本身可以解释 80% 的性能方差'——说明多 agent 的收益很大程度上来自并行探索预算,而非架构本身带来的额外智能
  - 适用边界:多 agent 更适合能拆成'并行独立研究线程'的任务;对紧耦合、依赖上下文连续性的任务(Anthropic 原文举例 coding)效果反而更差,因为子 agent 上下文隔离会导致协调开销/信息丢失
  - 工程代价不止 token:还包括 checkpointing、重试逻辑,以及'prompt engineering 成为主要调试杠杆'(早期系统会为简单 query 派生 50 个 subagent、无限爬网等失控行为)
  - 决策依据:任务是否可并行分解 + 结果价值能否覆盖 token 成本 + 是否需要跨子任务的强状态一致性
- **追问**：
  - 如果任务恰好是 coding 这种强依赖场景,你会如何改造 orchestrator-worker 模式来降低协调损耗?
  - 'token usage 解释 80% 方差'这个发现,对你设计 subagent 数量/并行度的策略有什么启发?
- **对应 nanocodex**：分层编排器(orchestrator flash/pro) + 主子agent通讯 — orchestrator.rs 用 best-of-N worker fanout(join_all 并行)放大探索路径的同时，用 tool-stripped reason() 节点、固定 worker Tier::Fast 控制成本，是同一权衡在代码里的具体取舍。卡:05_orchestrator_comm.md

### [L3 深挖/故障] 假设你的 supervisor-worker 系统里,多个 worker 并行地对同一份代码仓库做修改,最后阶段频繁出现文件冲突或互相覆盖。可能的根因有哪些?你会按什么顺序排查?

- **考察点**：考察对'并发写入共享状态'这一多 agent 系统经典故障模式的诊断能力,考的是排查思路而非背答案——是这个主题下最容易暴露'纸上理解'和'实战理解'差距的问题。
- **参考答案要点**：
  - 根因假设 1:worker 共享同一份 workspace/文件系统而未隔离,并行写操作(如 apply_patch 新增同名文件)天然产生竞态覆盖
  - 根因假设 2:合并/promote 阶段是'整体复制覆盖'而非真正的 diff/merge,导致非 winner 的 worker 改动被静默丢弃
  - 根因假设 3:裁决(verifier)对畸形输出的兜底策略(如找不到明确胜者时默认选第一个)可能选中了不该被信任的结果
  - 排查顺序:先确认 worker 是否运行在物理隔离目录;再检查 promote/merge 阶段是编辑合并还是整体覆盖;最后检查裁决解析的健壮性与默认兜底是否合理
  - 工程原则:任何时刻真实 workspace 只应有'恰好一个写者'——要么串行化写入,要么隔离后再择一提升,不能让多个并行 agent 直接对同一份可变状态做非原子写入
- **追问**：
  - 如果子任务之间存在依赖(B 依赖 A 的产出),隔离+串行提升方案要如何调整?
  - 除了文件系统,共享数据库/内存状态的多 agent 写入冲突要如何设计类似的隔离机制?
- **对应 nanocodex**：分层编排器(orchestrator flash/pro) + 主子agent通讯 — orchestrator.rs/runner.rs/isolate.rs 的具体实现：run_worker 给每个 worker 分配唯一 scratch 目录做 copy_tree 隔离，verifier 用 BEST:<n> 选 winner 后 promote_worker 覆盖式提升回真 workspace，正是为修复'并行 apply_patch 同名文件撞车'这一真实 bug 而设计。卡:05_orchestrator_comm.md

### [L4 开放设计] 如果让你设计一个多 agent 系统的通讯协议,支持一个'主规划 agent'调度若干'执行 agent'对同一个代码仓库做协作式修改,且子任务之间存在依赖关系,你会如何设计通讯通道、隔离机制和裁决机制?

- **考察点**：综合考察候选人能否把前面几个概念(hierarchical 编排、共享状态 vs 消息传递、隔离与合并)组合成一个可落地的系统设计,是本主题的开放设计压轴题。
- **参考答案要点**：
  - 通讯通道:不必维护长会话/消息总线,可用'无状态子会话 + prompt 按值序列化'——每次调用把当前 plan、身份、上一轮反馈重新序列化进子 agent 的 prompt,避免维护复杂的跨会话状态同步
  - 隔离机制:每个并行执行者应工作在物理隔离的副本(临时目录/分支)而非直接改共享真实状态,应对'多个写者竞争同一份可变资源'这一类通用问题
  - 裁决与提升:设计独立的 verifier/裁决角色输出结构化或半结构化裁决(如 PASS/FAIL、BEST:<n>),由父节点的确定性代码(而非另一个 agent)执行覆盖式提升;裁决解析必须有兜底策略避免死锁
  - 依赖处理:有依赖关系的子任务必须串行执行,且每个子任务的产出必须在下一个子任务开始前真正提升到共享状态,后续子任务才能在其之上叠加;无依赖的子任务可并行 fanout
  - 权衡取舍:prompt 按值序列化的代价是每轮 token 开销更高、无法利用会话记忆增量;隔离复制的代价是磁盘/时间开销,且合并只能是'整体覆盖'而非跨 worker 的语义合并
  - 可以借鉴 A2A 的 Agent Card/task 生命周期思想作为'agent 间需要长期、可发现协作'时的备选方案,但紧耦合单仓库协作场景下更简单的编排+隔离模型往往更可靠
- **追问**：
  - 如果要支持'多个子任务同时修改同一文件的不同部分'这种更细粒度协作,方案要如何演进(引导到三方合并/语义 merge)?
  - 这套设计和 A2A 协议里 task 对象生命周期的设计理念有什么相通或本质差异?
- **对应 nanocodex**：分层编排器(orchestrator flash/pro) + 主子agent通讯 — orchestrator.rs 全套机制(AgentRunner trait 隔离子会话、build_worker_task 的 prompt 序列化、isolate.rs 的 copy_tree 隔离、promote_worker 覆盖提升、串行 decompose_and_recurse 处理依赖子任务)正是这道开放设计题的现实答案。卡:05_orchestrator_comm.md

---

## 3. 工具使用工程化 / Function Calling

**工程定义**：Function calling(工具调用/tool use)的工程标准定义可从 Anthropic 与 Model Context Protocol(MCP)的官方规范中提炼:每个工具被声明为一份显式契约——name、description、以 JSON Schema 表达的 input_schema(parameters),模型(非确定性一侧)只产出结构化的 tool_use 请求并选择何时调用、传何参数,实际执行永远发生在调用方的确定性代码里,执行结果以 tool_result 块(可带 is_error: true 及可读错误文本)回灌对话,形成'定义 schema → 模型选择/填参 → 应用执行 → 结果回灌'的闭环(Anthropic, 'Tool use with Claude')。为了给客户端提供风险/安全提示(而非用于工具发现或选择路由),MCP 在 Tool 之上定义了 ToolAnnotations(readOnlyHint / destructiveHint / idempotentHint / openWorldHint),但规范与官方博客明确指出这些字段只是'hint'、不保证真实描述工具行为,客户端不能把未受信 server 报的 annotation 当作安全决策依据,真正的安全保证必须落在确定性的 client 侧控制上(MCP Schema Reference 2025-06-18; MCP Blog 'Tool Annotations as Risk Vocabulary')。当工具数量从个位数扩展到几十/上百个(如 MCP 生态、企业 API 网关)时,把全部 schema 一次性喂给模型会稀释工具选择准确率——Gorilla 论文实测显示,用质量欠佳的检索器(BM25/GPT-Index)做工具候选收窄反而使准确率出现两位数百分点的下降,而 ToolLLM 则通过专门训练的 API retriever 在 16000+ 真实 API 规模上做动态候选收窄并对比 oracle retriever 与 zero-shot 设置;这说明'动态暴露/路由'不是可选优化,而是工具规模超出模型有效选择窗口后的必要工程手段,收窄检索器本身的质量会成为新的准确率瓶颈(Gorilla, arXiv:2305.15334;ToolLLM, arXiv:2307.16789)。在执行侧,OpenAI 的 Function calling 指南明确模型单轮可能产出零个、一个或多个(parallel)工具调用,调用方必须假设并发/重复调用会发生;OpenAI 的 Using GPT-5.5 指南进一步建议把'副作用(side effects)'与'重试安全(retry safety)'写进工具描述本身,业界也普遍认为有副作用的写工具应被设计为对重复调用安全,并将只读、无副作用的调用与有状态写操作在并发与顺序保证上区别对待,这与 Anthropic 建议的错误处理范式(用 is_error + 具体可读的错误文本让模型能够重试或改变策略,而非用泛化的 'Failed' 中断)共同构成了工具调用结果的错误处理与恢复、以及只读/写并发安全的工程基础(OpenAI, 'Function calling' | OpenAI API;OpenAI, 'Using GPT-5.5' | OpenAI API;Anthropic, 'Tool use with Claude')。

**来源**：
- [Schema Reference - Model Context Protocol (2025-06-18, ToolAnnotations)](https://modelcontextprotocol.io/specification/2025-06-18/schema)
- [Tool Annotations as Risk Vocabulary: What Hints Can and Can't Do | Model Context Protocol Blog](https://blog.modelcontextprotocol.io/posts/2026-03-16-tool-annotations/)
- [Tool use with Claude - Claude Platform Docs (Anthropic)](https://docs.anthropic.com/en/docs/build-with-claude/tool-use)
- [Writing effective tools for AI agents—using AI agents (Anthropic Engineering)](https://www.anthropic.com/engineering/writing-tools-for-agents)
- [Introducing advanced tool use on the Claude Developer Platform (Anthropic Engineering, tool_search/defer_loading)](https://www.anthropic.com/engineering/advanced-tool-use)
- [Gorilla: Large Language Model Connected with Massive APIs](https://arxiv.org/abs/2305.15334)
- [ToolLLM: Facilitating Large Language Models to Master 16000+ Real-world APIs](https://arxiv.org/abs/2307.16789)
- [Function calling | OpenAI API](https://developers.openai.com/api/docs/guides/function-calling)
- [Using GPT-5.5 | OpenAI API](https://developers.openai.com/api/docs/guides/latest-model)

### [L1 理解] 工具的 JSON Schema(input_schema/parameters)设计应该遵循什么原则?为什么给 agent 设计工具不能照搬给人类开发者写 REST API 的思路(比如把 list_users/list_events/create_event 拆成三个细粒度工具)?

- **考察点**：考察是否理解 function calling 的契约模型本质——schema 是模型与执行环境之间唯一的接口约定,以及为什么工具设计需要'为 agent 而非为开发者'重新思考。
- **参考答案要点**：
  - Anthropic 文档明确 tool use 是契约模型:JSON Schema 定义参数结构,模型据此填参,调用方执行并回传结果
  - Anthropic 工程博客建议 consolidate functionality——把频繁链式调用的多步操作合并成一个工具(如 schedule_event 取代 list_users+list_events+create_event),减少模型需要编排的调用轮数和出错点
  - 每个工具应有清晰、单一、不重叠的目的;工具集越大越需要 namespacing(前缀/后缀分组)防止相似功能的工具互相混淆
  - JSON Schema 只能表达结构合法性,表达不了'什么时候该填可选参数'等使用模式,因此 Anthropic 新增 input_examples 作为 schema 的补充
- **追问**：
  - 如果一个工具的参数是深层嵌套结构,你会怎么降低模型生成错误 JSON 的概率?
  - 你会用什么指标(而不是纯准确率)评估一次 schema 重构是否真的改善了 agent 表现?
- **对应 nanocodex**：工具系统 — tools.rs 的 Tool trait 声明 name/description/parameters(即 JSON Schema)/read_only/execute 字段契约,parameters 字段是 schema 设计原则的直接落地点

### [L2 权衡] 当可用工具从几个扩展到几十/上百个时,为什么直接把所有工具 schema 都塞进 context 反而会让模型的工具选择准确率下降?你会用哪些工程手段解决这个问题?

- **考察点**：这是该类别的核心权衡题,考察是否理解'工具过多稀释选择准确率'这一实证现象,以及是否知道业界几种应对路线(检索收窄 vs 分层暴露 vs namespacing)及各自代价。
- **参考答案要点**：
  - Gorilla 论文实测:用 BM25/GPT-Index 这类质量一般的检索器做工具候选收窄,反而使准确率下降两位数百分点,说明'收窄'本身不是免费的,劣质检索器比不检索更confuse模型
  - ToolLLM 在 16000+ API 规模上专门训练 API retriever 做动态候选收窄,并对比 oracle retriever 与 zero-shot,证明检索质量直接决定下游任务完成率
  - Anthropic 提出的应对手段:namespacing 分组、tool_search + defer_loading(按需加载工具定义而非一次性全部声明)、以及持续用 tool-call 错误率/token 消耗等指标做评估驱动的迭代
  - 本质上是候选集大小与检索精度之间的权衡:候选集越小越省 token、选择越准,但可能漏掉正确工具;候选集越大越不容易漏,但稀释选择
- **追问**：
  - 如果你的检索器召回率高但精确率低(召回了很多不相关工具),这比全量暴露更差还是更好?
  - 你会如何设计'核心工具常驻 + 长尾工具按需发现'的两层结构来兼顾稳定性和可扩展性?
- **对应 nanocodex**：工具系统 动态暴露 — tools.rs 的 schemas_for_query 每回合把工具裁剪到 DEFAULT_VISIBLE_TOOL_LIMIT=9,用 catalog_score 词法打分(100/50/20)在核心集∪tool_hints 之外竞争填位,tool_search 把命中写入 tool_hints 供下一轮暴露

### [L2 权衡] 工具调用失败(参数错误、下游 API 超时/503、权限被拒)时,你会如何设计错误从执行层回传给模型的机制,让 agent 能理解错误并自我恢复,而不是让整个 turn 崩溃或陷入无意义重试?

- **考察点**：考察对 tool_result 错误契约设计的理解,以及'把错误变成可恢复信号'与'防止无限重试'之间的工程权衡。
- **参考答案要点**：
  - Anthropic 的做法是在 tool_result 上设置 is_error: true 并附具体、可读的错误文本(如指出具体不存在的实体),而不是泛化的 'Failed',这样模型才能判断是重试、换参数还是放弃
  - 错误必须被表达成'模型可读的普通消息'而不是抛异常中断 agent loop,否则一次工具失败会让整个多步任务崩溃
  - 需要用外部预算(如最大工具调用次数/最大回合数)兜底,防止模型对同一错误无休止地重试同一工具
  - 应区分可恢复错误(参数格式错误、瞬时超时——值得重试或改参数)与不可恢复错误(权限拒绝、资源不存在——应该终止该路径或询问用户)
- **追问**：
  - 如果工具超时,你怎么区分'确实执行慢'和'已经卡死需要杀掉'?
  - 如果同一个工具调用带同样参数连续失败 3 次,你会在系统层面加什么熔断逻辑?
- **对应 nanocodex**：工具系统 — tools.rs 的 execute() 恒定返回 String 而非 Result,未知工具名、pre-hook 阻断等失败一律变成模型可读的 Error 字符串(如 'Error: unknown tool ...');另见 Harness 工程管理 — 双预算 max_model_calls/max_tool_calls 防止无限重试

### [L3 深挖/故障] 生产环境里发现 agent 并发执行了两个工具调用,其中一个只读调用读到了另一个并发写操作的中间状态,导致结果不一致。你怎么定位根因、怎么修?

- **考察点**：考察只读并发批处理的故障排查能力——是否知道'read_only 标记'的本质是信任声明而非运行时验证,以及并发调度器的正确边界条件应该是什么。
- **参考答案要点**：
  - 先确认并发调度器的分段逻辑本身是否正确:是否只在连续调用都被标记为 read_only 时才并发执行,写调用/未知调用是否正确打断了并发段并转入串行
  - 再确认结果是否按原始调用顺序缝回给模型,而不是按并发完成顺序,否则即使执行正确,模型看到的调用-结果对应关系也会错乱
  - 关键怀疑点是 read_only 标志本身的可信度——它通常是工具的静态声明或名字启发式(如按前缀猜测),不是运行时验证;一个谎报只读实则有副作用的工具会被调度器错误地并发调度而产生 race
  - MCP 规范里对应的 readOnlyHint 也被官方明确定义为'hint,不保证真实行为',印证了'不能把只读标记当作安全验证依据'这一通用原则
- **追问**：
  - 如果一个第三方 MCP 工具名字看起来只读(如 fetch_status)但底层触发了副作用,你的系统怎么防御?
  - 你会给'只读'加运行时验证机制(比如沙箱内先探测一次)吗,还是接受这是一个必须靠声明约定的信任边界?
- **对应 nanocodex**：Harness 工程管理 — parallel_run 仅当连续调用都 is_read_only 时才用 join_all 并发,写工具打断成串行段,并发结果按 batch.iter().zip(results) 按原始调用序缝回;read_only 是信任边界而非验证,工具谎报只读会被并发调度而 race

### [L3 深挖/故障] 任务链中第 3 个工具调用返回的结果字段被下游 API 静默改名(比如 user_id 变成 userId),导致后续依赖该字段的调用全部失败。这是 schema 设计缺陷还是错误处理缺陷?你会怎么修?

- **考察点**：考察是否能区分'设计期契约漂移'与'运行时错误处理'这两类不同性质的问题,以及是否有 fail-fast 而非让错误静默传导到模型推理层的工程习惯。
- **参考答案要点**：
  - 根因是工具的 output 契约漂移——下游 API 变更未同步更新工具侧的输出结构或文档,这属于 schema/契约维护问题而不是运行时错误处理问题
  - 理想做法是给工具的输出也做结构校验(不只是输入 input_schema),一旦发现字段缺失/改名应在工具执行层就近报错,而不是让格式错误的结果流入模型上下文让模型自己去'猜'并产生连锁误判
  - 这与 OpenAI strict mode/structured outputs 的思路一致:用更强的 schema 约束尽早暴露契约不一致,而不是依赖模型的鲁棒性去兜底不可靠的数据形状
  - 工程上应考虑给第三方工具(尤其 MCP server)的输出加版本号或做防御性字段兼容,减少'静默改名'类故障的爆炸半径
- **追问**：
  - 如果这个改名是第三方 MCP server 自行升级导致的、你无法控制其发版节奏,你会在自己系统里加哪一层防护?
  - 你会考虑在 tool_result 里附加 schema version 字段吗?这样做的成本和收益是什么?
- **对应 nanocodex**：(无对应卡:工具输出契约/schema 漂移校验不在当前 8 张 nanocodex 卡片覆盖范围内,属通用工程契约维护问题,非项目特定机制)

### [L4 开放设计] 请设计一个能安全支持数百个工具(含不受信任的第三方 MCP servers)的 agent 工具系统。'决定哪些工具暴露给模型看'和'决定某个工具能不能被执行/是否需要审批'这两个决策,你会不会用同一套逻辑处理?为什么?

- **考察点**：开放设计题,考察系统级思维——能否把'工具路由/选择准确率'这个 UX/性能问题和'工具执行安全'这个信任/安全问题识别为两条正交关注点并分层设计,避免用同一个信号(如 read_only 标记)同时驱动两件事而产生安全漏洞。
- **参考答案要点**：
  - 动态暴露/路由关心的是'如何缩小候选集提升模型选择准确率',本质是 UX 与性能问题,可以用词法/embedding 检索、namespacing、核心集常驻+按需发现等手段解决,判断错了代价是选错工具、多绕一轮
  - 执行安全关心的是'这个工具调用会不会产生不可逆的真实副作用',本质是信任与安全问题,需要独立的一层——先判定物理上是否允许(如目标路径是否在可写目录内),再判定越界时如何处置(自动批准/询问用户/自动拒绝)
  - 两者绝不能共用同一个信号做决策:如果把'是否只读'这个暴露/并发用的标志同时当作沙箱写权限的依据,一个谎报只读的第三方工具就能绕过审批直接执行破坏性操作
  - MCP 官方立场与此一致:ToolAnnotations 只用于 UX 提示与客户端展示,规范明确要求客户端不能把未受信 server 报告的 hint 当作安全决策依据,真正的安全保证必须落在客户端侧的确定性控制上
- **追问**：
  - 如果一个 MCP server 完全不受信任,你会不会采信它自报的 readOnlyHint 做任何自动化决策?
  - 审批粒度你会做成工具级(整个工具需要审批)还是参数级(比如允许读任意文件但只能写指定目录)?为什么?
- **对应 nanocodex**：沙箱 审批状态机 — policy.rs(纯函数判物理可写性 can_write)+ approval.rs(Approver::classify 判 AutoApprove/Ask/AutoDeny)与 工具系统 动态暴露 的 read_only 标志相互正交:read_only 只驱动并发批处理,不 gate 沙箱写,沙箱写完全由 SandboxPolicy 与 Approver 独立决定

---

## 4. 上下文工程与记忆系统

**工程定义**：上下文工程(context engineering)是围绕LLM推理时那个有限的、由system prompt、tools、few-shot examples、message history等token组成的context window,系统性地决定'该放入什么、该省略什么、该外部化到哪里'的工程学科。Anthropic在其工程博客《Effective context engineering for AI agents》(2025-09-29)中将其定义为'the set of strategies for curating and maintaining the optimal set of tokens (information) during LLM inference, including all the other information that may land there outside of the prompts',并指出其必要性源于'context rot':needle-in-a-haystack类基准显示,随着context window中token数增加,模型准确召回信息的能力会下降,这是一种精度梯度而非硬崖式失效,根源在于Transformer架构下每个token都要attend to每一个其他token所形成的O(n²) pairwise关系在长序列上被稀释,加上训练数据里长序列样本本身更少。为应对这一有限的'attention budget',该文给出三类互补的long-horizon手段:compaction(把接近上限的对话历史交给模型摘要后重新起一个context,如Claude Code保留架构决策/未解决bug/实现细节,同时丢弃冗余tool原始输出)、structured note-taking/agentic memory(agent主动把状态写到context之外的持久存储如NOTES.md或Claude的memory tool,按需读回)、以及sub-agent architectures(子agent在独立干净context里做数万token的深度探索,只把1000-2000 token的蒸馏摘要返回给主agent)。在记忆的时间维度上,MemGPT(Packer et al., arXiv:2310.08560,《MemGPT: Towards LLMs as Operating Systems》)提出的OS式分层记忆模型被广泛借鉴:main context(相当于内存)容纳system instruction、working context和FIFO消息队列,是模型直接可见的短期记忆;外部的recall storage(全部历史消息的可检索库)与archival storage(向量化长期知识库)构成长期记忆,二者通过LLM自主调用的函数(如archival_memory_search、core_memory_append)做类比OS virtual memory paging的换入换出。检索增强生成(RAG)是让长期记忆'非参数化'地外置于模型权重之外的经典范式,源自Lewis et al.(2020, arXiv:2005.11401, NeurIPS,《Retrieval-Augmented Generation for Knowledge-Intensive NLP Tasks》),核心是把预训练的parametric memory(模型权重里的知识)与可检索的non-parametric memory(外部文档索引)结合,靠检索器返回的段落约束生成,以提升事实性、可追溯性并支持知识更新而无需重新训练。在记忆的写入去重与遗忘策略上,Generative Agents(Park et al., 2023, arXiv:2304.03442, UIST,《Generative Agents: Interactive Simulacra of Human Behavior》)给出了被广泛引用的检索打分公式——score = 归一化后的recency(指数衰减)+ importance(LLM打1-10分)+ relevance(embedding余弦相似度)的加权和,并引入'reflection'机制,定期让LLM在近百条原始记忆上归纳出更高层抽象并写回记忆流;MemoryBank(Zhong et al., arXiv:2305.10250)则显式借鉴心理学中的Ebbinghaus forgetting curve,用随时间指数衰减的记忆强度分数做选择性遗忘与强化,避免记忆库无限增长带来的检索噪声。Prompt caching是服务于同一工程目标的推理层优化:据Anthropic官方文档(platform.claude.com/docs/en/build-with-claude/prompt-caching,原docs.anthropic.com同路径为其legacy别名),其机制是对prompt按tools→system→messages固定顺序做前缀哈希,cache write只发生在cache_control breakpoint处、写入'截止该断点的整条前缀'对应的一条entry;下一次请求在breakpoint处及其之前最多回溯20个block(20-block lookback window)寻找此前写入过的匹配entry,断点之前任意内容变化(时间戳、工具集变化等)都会使该断点及之后全部cache miss;5分钟TTL下cache write price是base input token的1.25倍,cache read只需0.1倍,因此要求把静态、跨请求不变的内容放在prompt最前且保持字节级稳定。

**来源**：
- [Effective context engineering for AI agents (Anthropic Engineering Blog, 2025-09-29)](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)
- [Prompt caching - Claude Platform Docs (Anthropic)](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)
- [MemGPT: Towards LLMs as Operating Systems (Packer et al., arXiv:2310.08560)](https://arxiv.org/abs/2310.08560)
- [Retrieval-Augmented Generation for Knowledge-Intensive NLP Tasks (Lewis et al., arXiv:2005.11401, NeurIPS 2020)](https://arxiv.org/abs/2005.11401)
- [Generative Agents: Interactive Simulacra of Human Behavior (Park et al., arXiv:2304.03442, UIST 2023)](https://arxiv.org/abs/2304.03442)
- [MemoryBank: Enhancing Large Language Models with Long-Term Memory (Zhong et al., arXiv:2305.10250)](https://arxiv.org/abs/2305.10250)

### [L1 理解] Context window 为什么是agent工程里的核心瓶颈?什么是'context rot',工程上该怎么系统性应对,而不是简单指望'更大的context window'?

- **考察点**：考察候选人是否理解context是有限、有衰减效应的资源而非无限缓存,是判断候选人有没有做过真实长会话agent的基础问题。
- **参考答案要点**：
  - Anthropic定义context engineering为'curating and maintaining the optimal set of tokens...during LLM inference'(《Effective context engineering for AI agents》,2025-09-29)
  - 'context rot':needle-in-a-haystack类基准显示随context中token数增加,模型准确召回信息的能力下降,是精度梯度而非硬崖式失效
  - 根因是Transformer里每个token都要attend所有其他token,形成O(n²) pairwise关系,长度增长使关系被稀释,训练数据里长序列样本本身也更少
  - 工程应对不是等更大context window,而是主动裁剪出'能最大化期望行为的最小高信号token集合'
- **追问**：
  - 如果模型支持1M甚至10M context,是否还需要context compaction?
  - 怎么量化衡量你的agent在生产里有没有出现context rot?
- **对应 nanocodex**：02_context_compression — session.rs 的 edited_body 两趟压缩算法(压缩旧 tool 结果 max_tool_result_chars=4k、超预算 max_chars=120k 丢最老前缀)正是应对 context rot 的具体工程实现

### [L2 权衡] Compaction(摘要重置)、structured note-taking(agentic memory)、sub-agent架构(context隔离)是应对long-horizon任务的三种context管理手段,它们分别适合什么场景?怎么取舍?

- **考察点**：长程coding/research agent工程里最常被问到的架构权衡题,考察候选人是否知道'一刀切用摘要压缩'并非最优解。
- **参考答案要点**：
  - Compaction:把接近context上限的历史交给模型摘要后重新起窗口,保留架构决策/未解决bug/实现细节,丢弃冗余tool原始输出;适合需要保持强连续对话流的高频来回任务
  - Structured note-taking:agent主动把状态写到context之外的持久文件(如NOTES.md、Claude的memory tool),按需读回;持久化开销最小,适合有清晰milestone的迭代任务
  - Sub-agent架构:子agent在独立干净context里做数万token的深度探索,只把1000-2000 token的蒸馏摘要返回给主agent,把细节探索的token消耗与主线决策的context占用解耦;适合可并行探索的复杂研究/搜索任务
  - 三者不互斥,可组合使用
- **追问**：
  - 子agent摘要过度压缩导致主agent决策错误时怎么排查?
  - note-taking写的内容算短期记忆还是长期记忆,由谁决定何时读回?
- **对应 nanocodex**：05_orchestrator_comm — nanocodex的orchestrator确实把每个worker放进全新无状态AgentRunner会话、配独立workspace拷贝(真实IPC靠promote_worker),这是与sub-agent context隔离结构上相似的一点;但驱动这个设计的是修复并行worker apply_patch写同一文件的撞车(isolate.rs:4-8)、并支持verifier的best-of-N裁决,而不是token预算式的摘要蒸馏——build_verify_task实际上是把每个worker的完整原始输出'--- worker i --- {r}'原样拼接给verifier,没有任何摘要/压缩步骤,与Anthropic所述'子agent只返回1000-2000 token蒸馏摘要给主agent'正好相反。只能当作部分/结构性类比,不要当成同一技术的实现

### [L2 权衡] 什么时候该用RAG(向量检索)、什么时候该把资料直接放进long context、什么时候该用agentic的'just-in-time'检索(现查现用)?三者的工程权衡是什么?

- **考察点**：RAG是这个类目里最高频的基础考点,真正区分候选人水平的是能否讲清它跟长上下文、agentic检索的边界,而不是背诵RAG流程。
- **参考答案要点**：
  - RAG(Lewis et al., 2020, arXiv:2005.11401)本质是把预训练的parametric memory(模型权重)与可检索的non-parametric memory(外部文档索引)结合,靠检索段落约束生成,提升事实性、可追溯性,知识更新无需重新训练
  - 预计算embedding+向量检索速度快、成本可控,但存在index过期、embedding语义与查询语义不对齐的风险
  - Agentic just-in-time检索(如用文件路径/查询等轻量标识符运行时动态加载)更贴近人类用文件系统/书签做外部索引的方式,能做渐进式披露(progressive disclosure),但比预取慢,依赖模型自主导航、不走死胡同的能力
  - 工程上常见混合策略:高置信度稳定资料预取进context,长尾/易变资料留给运行时探索
- **追问**：
  - chunk size怎么选,过大过小分别有什么后果?
  - 怎么评估RAG系统的召回率和精确率,分别对下游生成质量的影响是什么?
- **对应 nanocodex**：03_tools — tools.rs 的 tool_search 打分整个工具catalog(词法打分器,无需embedding)、把命中名写入共享 ctx.tool_hints 供下一轮暴露,是'agentic just-in-time检索'(运行时按需动态发现资源而非一次性全量预置)这一设计哲学在工具发现场景下的具体落地

### [L3 深挖/故障] Agent长期运行后,记忆库里出现重复、过时甚至互相矛盾的记忆片段,检索结果被这些噪声污染,拖累了决策质量——你怎么诊断根因、怎么修?

- **考察点**：记忆系统在生产环境里最常见的故障模式,考察候选人有没有真正跑过'越用越乱'的记忆系统,能否给出可落地的修复路径而不是空泛地说'加个清理任务'。
- **参考答案要点**：
  - 先定位写入路径:是否每次相似经历都不做判断地新建一条记录(没有去重),导致同一事实在库里出现N份且都被检索打分函数排到高位
  - 再看retrieval打分公式:如果只看relevance(embedding相似度)不看recency/importance,旧的、已被后续事实覆盖的记忆会持续被检索到并与新记忆冲突
  - Generative Agents(Park et al., arXiv:2304.03442)方案:retrieval score = 归一化后的recency+importance+relevance加权和,并引入独立的reflection步骤定期把近百条原始记忆归纳成高层结论回写
  - MemoryBank(Zhong et al., arXiv:2305.10250)方案:借鉴Ebbinghaus forgetting curve,给每条记忆维护随时间指数衰减的记忆强度分数,对强度过低的做选择性遗忘
  - 修复路径:引入定期consolidation对高相似度记忆做折叠去重;检索打分加入时间衰减项;对长期未被访问且重要性低的记忆做显式降权或删除
- **追问**：
  - 去重相似度阈值设太低/太高分别会造成什么后果?
  - 怎么区分'该被遗忘的噪音'和'低频但关键的记忆'(比如一年一次的重要偏好)?
- **对应 nanocodex**：06_memory — memory.rs 的 consolidate() 用 Jaccard 阈值去重折叠,正是解决这个具体故障模式(记忆冗余污染recall)的工程手段

### [L1 理解] Anthropic的prompt caching底层是怎么工作的?为什么prompt里'哪部分放前面、哪部分放后面'会直接决定缓存命中率和成本?

- **考察点**：考察候选人是否理解prompt caching不是'语义缓存'而是严格的前缀匹配机制,这直接决定了agent的system prompt/tool定义该怎么组织。
- **参考答案要点**：
  - 据Anthropic官方文档(platform.claude.com/docs/en/build-with-claude/prompt-caching):prompt按tools→system→messages固定顺序做前缀哈希;cache write只发生在cache_control breakpoint处,写入'截止该断点的整条前缀'对应的一条entry
  - 读取时在breakpoint处及其之前最多回溯20个block(20-block lookback window)寻找此前写入过的匹配entry,本质是'找之前写过的prefix'而非'识别哪部分内容没变'
  - 断点之前任意一个block变化(时间戳、工具集变化、图片增减)都会让该断点及其后的哈希失效,导致缓存未命中
  - 成本结构:5分钟TTL下cache write是base input token价格的1.25倍,cache read只要0.1倍,只有足够高的复用次数才能覆盖写入溢价
  - 工程含义:必须把静态、跨请求不变的内容(tool定义、system instruction)放在prompt最前面且保持字节级稳定,把易变内容放在最后
- **追问**：
  - automatic caching和explicit breakpoint该怎么选?
  - 如果一定要在system prompt里加时间戳,怎么设计能不破坏缓存?

### [L4 开放设计] 如果agent每回合都动态裁剪、只暴露最相关的一小部分工具schema(而不是固定工具集),这跟prompt caching依赖'稳定前缀'的机制是不是冲突?工程上怎么调和?

- **考察点**：把'工具动态暴露'和'prompt caching'两个真实机制放在一起做设计权衡的开放题,考察候选人能否预见两个独立优化目标之间的相互作用,而不是孤立看待每个机制。
- **参考答案要点**：
  - 根据prompt caching机制,tools字段排在prefix最前面且要求跨请求字节级一致才能命中缓存
  - 如果每回合根据query动态裁剪到TopN个工具,tools块内容逐回合变化,会导致从tools往后的所有内容(system、历史消息)在该breakpoint处全部miss,退化成每轮全量付费
  - 调和思路一:分层——把高频稳定的少数工具放在prompt最前参与缓存,按需检索到的长尾工具作为不改变前缀的追加上下文放在后面
  - 调和思路二:接受一定缓存命中率损失换取token效率与决策清晰度,按实际turn数/费用结构核算哪个更划算
  - 本质是'context精简'与'prompt稳定性'两个工程目标的直接冲突,没有免费的解
- **追问**：
  - 如果你的产品turn数很多(如长程coding agent),你会偏向哪种设计?
  - 怎么用数据而不是直觉去决定该不该做动态工具暴露?
- **对应 nanocodex**：03_tools — tools.rs 的 schemas_limited_for_query 每回合把工具裁剪到 DEFAULT_VISIBLE_TOOL_LIMIT=9 个,正是与 prompt caching 前缀稳定性直接冲突的具体机制

---

## 5. 安全护栏与执行沙箱

**工程定义**："安全护栏与执行沙箱"这一工程领域的核心，是在 Agent 能够自主调用工具、执行代码、访问外部数据的前提下，把"模型的意图判断"与"系统能否物理执行该操作"解耦成两个互相独立、可分别验证的机制。其理论基础是 least privilege 原则——NIST SP 800-53(Rev.5) 的官方 glossary 将其定义为 "an entity is granted the minimum system authorizations and resources needed to perform its function"，对应到 Agent 系统即每次工具调用只应获得完成该次任务所必需的最小权限，而非账号级/进程级的一次性授权。工程上通常拆成两条正交边界：(1) permission/审批模型——如 Claude Code 官方文档所述，用 allow/ask/deny 三态规则、并按固定的 deny > ask > allow 顺序求值，决定"要不要问人"；(2) execution sandbox——OS 级隔离（Anthropic 用 macOS Seatbelt / Linux bubblewrap 实现 filesystem isolation 与 network isolation），决定"就算批准了，进程物理上能不能碰到这个资源"。Anthropic 明确把二者定义为互补而非替代的 defense-in-depth 层：permission 覆盖全部工具的访问决策，sandbox 只在 OS 层兜底 Bash 子进程的文件/网络边界。但"即便 prompt injection 绕过模型决策、边界依然拦得住"这一保证并非在所有产品形态下都同等成立：Anthropic 后续的《How we contain Claude across products》一文明确区分——在采用完整 VM 隔离、不存在"sandbox 外逃生舱"进程的 Claude Cowork 里，这个保证是干净成立的；而在本地运行的 Claude Code 里，仍有一个位于 sandbox 之外、负责逐条命令决定是否强制沙箱化的特权进程，一段有说服力的注入提示或一次审批疲劳下的误点，仍可能诱使这个特权进程放行未被沙箱化的命令——也就是说，对 Claude Code 而言 sandbox 边界是"大幅收窄攻击面"，而不是无条件的绝对兜底。这套体系存在的根本诱因是 prompt injection——OWASP Top 10 for LLM Applications 2025 将其列为 LLM01，定义为 "user prompts alter the LLM's behavior or output in unintended ways"，并指出根源是模型把 instruction 与 data 放在同一 token 流里处理、缺乏可靠通道分离，因此 RAG、微调等手段只能 ground 模型、并不能 secure 模型，唯有 privilege restriction 与 human-in-the-loop 才是相对确定性的第二道防线。Anthropic 的工程博客也印证了纯人工审批模式会随审批次数增多而失效（据《How we built Claude Code auto mode》，即便已有 sandbox 与 --dangerously-skip-permissions 可选，用户对手动 permission prompt 仍约 93% 直接批准），因此实践进一步演化出"危险操作分级拦截"：按风险把工具调用分层（只读自动放行、可逆写入每步确认、外发/删除/资金类操作强制人审并留痕），LangChain/LangGraph 的 human-in-the-loop middleware（用 interrupt_on 按 tool/参数配置 allowed_decisions）与 Claude Code auto mode 的分级 classifier 都是这一思想的具体实现。在协议层，针对 Model Context Protocol 的安全分析进一步指出，工具描述本身也必须被当作不可信输入处理，因为 capability 声明是 self-asserted、缺乏验证机制的，这既违反 least privilege（允许运行时任意扩权），也违反 origin authenticity（用户无法区分 server 注入的指令与用户原始指令）。

**来源**：
- [least privilege - Glossary | CSRC (NIST SP 800-53 Rev. 5)](https://csrc.nist.gov/glossary/term/least_privilege)
- [OWASP Top 10 for LLM Applications 2025 (v2025 PDF)](https://owasp.org/www-project-top-10-for-large-language-model-applications/assets/PDF/OWASP-Top-10-for-LLMs-v2025.pdf)
- [Configure permissions - Claude Code Docs](https://code.claude.com/docs/en/permissions)
- [Making Claude Code more secure and autonomous with sandboxing \ Anthropic](https://www.anthropic.com/engineering/claude-code-sandboxing)
- [How we built Claude Code auto mode: a safer way to skip permissions \ Anthropic](https://www.anthropic.com/engineering/claude-code-auto-mode)
- [How we contain Claude across products \ Anthropic](https://www.anthropic.com/engineering/how-we-contain-claude)
- [Enterprise-Grade Security for the Model Context Protocol (MCP): Frameworks and Mitigation Strategies](https://arxiv.org/pdf/2504.08623)
- [Human-in-the-loop - Docs by LangChain (deepagents)](https://docs.langchain.com/oss/python/deepagents/human-in-the-loop)

### [L1 理解] 什么是 least privilege 原则？在一个 Agent 工具调用链路里，这个原则具体是通过哪些机制落地的（比如 permission rule 和 sandbox 的分工）？

- **考察点**：考察候选人是否理解安全护栏不是一句口号，而要落到具体、可验证的系统边界上；能否区分'决策层（要不要做）'和'执行层（能不能做）'。
- **参考答案要点**：
  - NIST SP 800-53 官方定义：entity 只获得完成当前任务所必需的最小授权（https://csrc.nist.gov/glossary/term/least_privilege）
  - 落地成两条正交边界：permission/审批规则（allow/ask/deny，决定要不要问人）+ OS 级 execution sandbox（filesystem/network isolation，决定进程物理上能不能碰到资源）
  - Claude Code 官方文档：permission 规则按 deny > ask > allow 固定优先级求值，规则粒度不改变优先级
  - 覆盖面不同：permission 覆盖所有 tool（Bash/Read/Edit/WebFetch/MCP），sandbox 只兜底 Bash 子进程的 fs/network 边界
- **追问**：
  - 如果一个 deny 规则和一个更具体的 allow 规则同时匹配同一次调用，最终结果是什么？为什么这样设计？
  - permission rule 的判断是谁在执行——模型自己决定要不要问，还是有独立的执行层？
- **对应 nanocodex**：沙箱 审批状态机 — ncx-sandbox 的 policy.rs 用 read-only/workspace-write/danger-full-access 三态模式做纯函数式的物理可写性判断，对应的是 OpenAI Codex CLI 的 sandbox_mode/approval_policy 设计（read-only/workspace-write/danger-full-access 与 untrusted/on-failure/on-request/never 都是 Codex CLI 的原生术语，源码 docstring 也自称是 Codex 的 Rust port）；Claude Code 用的是不同的 allow/ask/deny + allowRead/denyRead/allowWrite/denyWrite 语义，命名和机制都不相同，二者不应被并列为同一种设计

### [L2 权衡] Anthropic 的两篇工程博客分别提到过两个不同阶段的数字：《Making Claude Code more secure and autonomous with sandboxing》报告说，上线 OS-level sandbox 之后，permission prompt 数量减少了约 84%；而更晚近的《How we built Claude Code auto mode》里又提到，即便已经有 sandbox 和 --dangerously-skip-permissions 两个选项可用，用户对手动 permission prompt 的批准率依然高达约 93%。为什么纯粹依赖'每次操作都问用户'的 permission-prompt 模式会失效？sandbox 和 permission prompt 应该分别放在防御体系的哪一层，为什么说它们是互补而非替代关系？

- **考察点**：考察是否理解'人类审批'是有限资源（会疲劳、会习惯性放行），以及为什么要把安全边界从'用户注意力'转移到'系统机制'；同时考察候选人能否分清两个不同时间点、不同产品机制下的统计数字各自的因果指向，而不是把它们串成一条虚假的时间线（例如误以为'早期测得 93% 被无脑批准，所以后来才引入了 sandbox'）。
- **参考答案要点**：
  - 《Making Claude Code more secure and autonomous with sandboxing》：上线 OS-level sandbox（macOS Seatbelt / Linux bubblewrap）后，permission prompt 减少约 84%，动机是缓解一般性的 approval fatigue
  - 更晚近的《How we built Claude Code auto mode》：即便已经有 sandbox 和 --dangerously-skip-permissions 两个选项，用户对手动 permission prompt 依然批准约 93%——这是'sandbox 已经存在之后'对现状的观察，用来论证还需要第三层机制（基于模型的 classifier，即 auto mode），而不是'sandbox 建成之前'促成 sandbox 立项的动机数据；不能把两者接成'早期 93% 被无脑批准 → 因此后来引入了 sandbox'的假因果链
  - 官方文档明确二者互补：permission deny rule 阻止 Claude 尝试访问受限资源；sandbox 边界确保即便 prompt injection 绕过 Claude 的决策，Bash 子进程仍碰不到边界外资源——但这一'绕过仍拦得住'的保证在 Claude Code 里并非绝对，据《How we contain Claude across products》，sandbox 外还有一个逐条命令做放行决定的特权进程，可能被有说服力的注入提示或审批疲劳下的误点绕过
  - sandbox 覆盖面窄（只管 Bash 及其子进程），permission rule 覆盖全部工具类型，因此必须叠加而非二选一
- **追问**：
  - sandbox 覆盖不到的工具（比如 Edit、WebFetch）要靠什么机制兜底？
  - 如果去掉 OS 级 sandbox，只靠更严格的 system prompt 能不能达到同等安全性？为什么？
- **对应 nanocodex**：沙箱 审批状态机 — ncx-sandbox 的 policy.rs 判物理可写性、approval.rs 判要不要问人，加上另一个 crate ncx-tools 的 executor.rs（PolicyExecutor，只做进程容器/超时，不碰审批），三者职责分离正对应'审批疲劳'与'系统边界'要分层处理的设计动机

### [L3 深挖/故障] 假设 ncx-sandbox 的 SandboxPolicy.network_access 是一个全局布尔开关（不区分域名），如果一次间接 prompt injection（比如工具读到的网页/文件里藏了指令）诱导 agent 在 network_access=true 时执行一条 curl 命令，把 workspace 里的密钥外泄到攻击者控制的域名——policy.rs 和 approval.rs 这套机制能拦住吗？为什么？你会怎么加固？

- **考察点**：这是本类别里最能问出真实工程深度的问题：考察是否理解 isolation sandboxing（控制 agent 能在哪跑）和 behavioral sandboxing（控制 agent 做什么）之间的 gap——即便所有边界检查都通过，攻击也可能发生在'被允许的操作'内部，而且要能看穿'文档写的意图'和'代码实际接线'之间可能存在的落差。
- **参考答案要点**：
  - policy.rs 只回答'这个路径能不能写、网络开不开'这类物理层问题，是不关心语义/调用意图的纯函数；network_access 一旦为 true，会对所有域名一视同仁地放行
  - 实际调用链路里，approval.rs 的 Approver::classify(command, needs_escalation) 是否检查命令首 token（TRUSTED_COMMANDS 里是否含 curl，答案是不含）取决于当前 approval policy：只有 untrusted 策略会同时看 is_trusted(command) 与 needs_escalation；默认的 on-request 策略只看 needs_escalation 这一个布尔值，完全不管命令是什么。而 needs_escalation 由 ncx-core/src/tools.rs 的 ShellTool::needs_escalation 计算，在 workspace-write 模式下目前只判断 workdir 是否可写，并不读取 ctx.policy.network_access——也就是说在默认 on-request + workspace-write + network_access=true 的组合下，这条 curl 命令很可能被直接 AutoApprove，既不看命令内容也不因为要发网络请求而升级审批
  - 这正是 isolation sandboxing（控制运行边界）与 behavioral sandboxing（控制行为语义）之间的 gap，而且比'语法层判断粗糙'更严重：approval.rs 文档注释里写的'needs_escalation 覆盖 network access'的设计意图，在 shell 工具这条调用链上目前并未真正接线；即便 agent 拥有的凭证/权限都'合法'，prompt injection 仍可能诱导它在权限范围内做恶意的事
  - 加固方向：把 network_access 真正接入 needs_escalation 的判断逻辑（让'即将访问网络'本身触发升级审批，而不只判断 workdir 可写性）；引入域名级 allowlist（类比 Claude Code 的 WebFetch(domain:...) 或 MCP 场景下按 data classification zone 分组工具）；在 tool 输出进入上下文前做 prompt-injection 内容扫描
- **追问**：
  - 如果要在 nanocodex 里加一层 domain allowlist，你会加在 ncx-sandbox 的 policy.rs 还是 ncx-tools 的 executor.rs？为什么？
  - 如果 escalation 请求本身就是一次网络请求，谁来判断这个目标域名到底安不安全？
- **对应 nanocodex**：沙箱 审批状态机 — ncx-sandbox 的 policy.rs（network_access 全局开关，无域名粒度）+ approval.rs（Approver::classify 基于命令首 token 与 needs_escalation 的语法层分类），但实际调用点 ncx-core/src/tools.rs 的 ShellTool::needs_escalation 在 workspace-write 模式下目前只判断 workdir 是否可写、并未读取 network_access，approval.rs 文档注释里'覆盖 network access'的设计意图尚未在这条链路接线，二者共同构成了'物理边界够但行为语义/接线都不够'的真实 gap

### [L4 开放设计] 如果要给一个 Agent 系统设计'危险操作分级拦截'——低风险只读操作自动放行、可逆写入每步确认、外发/删除/资金类操作强制人审并留痕——你会把'判断这次操作属于哪一级风险'和'该不该问人'这两件事拆成几个模块？为什么不能写成一个大 if-else？

- **考察点**：考察系统设计能力：能否把一个模糊的产品需求（分级拦截）拆解成职责清晰、可独立测试、可替换策略的模块；是否理解为什么'纯判定'和'审批策略'必须解耦。
- **参考答案要点**：
  - 参考设计：把'这个操作物理上能不能做'（纯函数，不依赖 IO/会话状态）与'这个操作要不要经过审批'（依赖 policy 名称如 untrusted/on-failure/on-request/never 的状态机）拆成两个正交模块，各自独立可测；执行侧再单独做一个只管进程隔离、完全不掺和审批逻辑的第三模块
  - 这种 separation of concerns 让'改风险分级策略'不需要碰隔离逻辑，'改隔离边界'不需要碰审批逻辑，两者可独立演进、独立单测
  - 对应业界实践：LangGraph/LangChain 的 human-in-the-loop middleware 用 interrupt_on 按 tool（甚至按参数用 when 谓词）配置 allowed_decisions，本质也是把'风险分级'做成声明式配置而非代码里的 if-else
  - OWASP 建议把 privilege restriction（沙箱/最小权限）和 human-in-the-loop 作为两条独立防线叠加，而非合并成一层
- **追问**：
  - '这次操作需要 escalation'这个信号应该由谁计算——是纯判定层算出来，还是审批层自己判断？
  - 如果要支持'按每步 vs 按整个会话'两种确认粒度，这个状态机要怎么改？
- **对应 nanocodex**：沙箱 审批状态机 — 这个设计的真实实现横跨两个 crate：ncx-sandbox 的 policy.rs 是纯函数判物理可写性，approval.rs 的 Approver::classify 判 AutoApprove/Ask/AutoDeny；ncx-tools 的 executor.rs（PolicyExecutor）只做进程容器/超时，完全不碰审批。approval.rs 里的 step_decision 函数专门处理'per-step 确认'这一分级需求，把 AutoApprove 在特定条件下升级为 Ask

### [L2 权衡] OWASP 认为 prompt injection 目前没有'彻底修复'的方法，只能做 defense-in-depth：input validation、output filtering、privilege restriction、human-in-the-loop 这几层里，为什么说 privilege restriction（也就是 sandbox/least-privilege）是相对'确定性'的一层，而'在 system prompt 里让模型不要听信外部指令'这种防御被认为是不可靠的？

- **考察点**：考察是否理解 LLM 层面的防御（system prompt 约束、指令过滤）本质上是概率性、可被绕过的，而系统层面的边界（权限/沙箱）是确定性的、和模型是否'听话'无关；能否讲清楚为什么工程上要把安全边界建在模型能力之外。
- **参考答案要点**：
  - OWASP Top 10 for LLM Applications 2025 LLM01：prompt injection 源于模型把 instruction 和 data 放在同一 channel 里处理，模型无法可靠区分'这是系统指令'还是'这是需要处理的数据'；RAG、微调等手段只是 ground 模型、不能 secure 模型
  - system prompt 层的防御（'忽略外部内容里的指令'）本质上仍是喂给同一模型的另一段文本，是否遵守取决于模型自身对齐水平、上下文长度/位置等因素，是概率性的
  - privilege restriction（sandbox 强制的 fs/network 边界、permission rule 的 deny 规则）不依赖模型'愿不愿意听话'——即便模型被注入指令完全接管，只要没有权限，操作在系统层面就执行不了，这是相对确定性的边界（但也要注意：如果决定'要不要强制这层边界'的判断本身是由 sandbox 外的某个进程/流程做出的，这个判断点仍可能被说服，边界本身是确定的，但是否触发边界不总是确定的）
  - 工程实践上永远是'prompt 层面的提示 + 系统层面的强制边界'叠加，而非只依赖前者
- **追问**：
  - 如果 sandbox 本身的网络开关是一个全局布尔（不区分域名），这条'确定性边界'还够'确定'吗？
  - 有没有办法把 output filtering 也做成确定性的，而不是再叫一个模型来分类？

### [L3 深挖/故障] 如果一个 MCP server 返回的 tool description 里藏了一段隐藏指令（工具描述投毒），client 把这段描述原样塞进模型的 system context，这是什么性质的攻击？MCP 协议目前在规范层面能提供什么防御，责任最终落在谁身上？

- **考察点**：考察候选人对'工具生态'这个新攻击面的认识——不仅用户输入和网页内容可能被注入，连协议本身声明的元数据（工具描述、capability 声明）也可能是攻击载体；是否了解 MCP 生态目前的安全现状和局限。
- **参考答案要点**：
  - 安全分析（arxiv 2504.08623 等）与 NSA/CISA 的 MCP 安全指南指出：MCP server 的 capability 声明是 self-asserted、无验证机制的，恶意 server 可在初始化后任意扩权，直接违反 least privilege
  - tool description 本身必须被当作不可信输入处理：用户/模型无法区分'server 注入的指令'和'用户原始指令'，这违反了 origin authenticity 这一安全属性
  - 当前 MCP 规范优先保证 interoperability 而非安全，没有强制的安全机制，防御责任被下放给 host application（如按数据分类 zone 把公共工具与涉敏工具分开授权）
  - 本质上与'间接 prompt injection'是同一大类问题（不可信内容进入模型上下文），只是载体从网页/文档内容换成了协议层元数据
- **追问**：
  - 这和典型的间接 prompt injection（通过网页/文档内容注入）相比，防御手段有什么不同？
  - 如果要在 nanocodex 的 MCP bridge 里加一层防御，你会加在哪个环节？
- **对应 nanocodex**：Skills · MCP · 视觉 — mcp_tool.rs（ncx-core/src/mcp_tool.rs）+ ncx-mcp 的 stdio JSON-RPC bridge(Rc<Mutex<McpClient>>) 是 nanocodex 里唯一从外部 MCP server 拉取 tool description 并注入模型上下文的入口，也就是这里讨论的需要加防御的具体位置

---

## 6. 可靠性、成本与延迟工程

**工程定义**：在生产级 LLM Agent 系统中，"可靠性、成本与延迟工程"是围绕外部依赖不确定性（网络抖动、限流、模型输出的不确定性）与 token 计费模型展开的一组工程实践：(1) 对可重试的瞬态错误（5xx/429/超时）应使用 capped exponential backoff with jitter 而非固定退避——AWS 的研究表明朴素指数退避只降低重试频率、并不打散并发重试的时间聚集（仍会造成 thundering herd），加入随机抖动（Full Jitter / Equal Jitter / Decorrelated Jitter）才能显著降低竞争工作量与恢复时间；(2) 只有被判定为 idempotent（重复执行不改变最终状态、无副作用）的操作才允许被自动重试，Stripe 与 Google Cloud 的通行做法是引入 client-generated idempotency key，服务端缓存首次执行结果并在重放时直接返回，把网络层的 at-least-once 转换为业务语义上的 exactly-once；(3) 在模型选择上，FrugalGPT 提出的 LLM cascade 用 cheap 模型优先响应、按质量估计器（quality estimator）决定是否升级到更贵模型，可在几乎不损失准确率的前提下实现 50%~98% 的成本下降，这是 cheap/strong tiering 的学术源头；(4) 在 token 成本与延迟层面，Anthropic 的 prompt caching 对稳定前缀按 tools→system→messages 的层级设置 cache breakpoint（cache_control），命中的 cache-read token 成本降至约 10%，但任何上游层级的改动都会级联使其后所有层级失效，因此上下文编辑（context editing/compaction）与缓存命中之间存在直接博弈；(5) 在并行度上，Anthropic 的 tool-use API 允许模型在一次 assistant turn 中返回多个 tool_use blocks，但协议本身不规定执行顺序，是否并发执行、如何避免写写冲突需调用方按工具的副作用类别（read-only vs mutating）自行判定；(6) 为保证 agent loop 在有限步内终止，OpenAI Agents SDK 用 max_turns 硬上限 + MaxTurnsExceeded 异常（或更新后的 error_handlers 实现优雅降级）、LangGraph 用 recursion_limit 与 RemainingSteps 做非崩溃式收尾，Model Context Protocol 规范则在协议层面定义了 per-request timeout 与双向 notifications/cancelled 协作式取消，三者共同构成了"预算（budget）+取消（cancellation）"的终止保证设计范式。

**来源**：
- [Exponential Backoff And Jitter — AWS Architecture Blog](https://aws.amazon.com/blogs/architecture/exponential-backoff-and-jitter/)
- [Idempotent requests — Stripe API Reference](https://docs.stripe.com/api/idempotent_requests)
- [What is Idempotency? A guide to API reliability — Google Cloud](https://cloud.google.com/discover/idempotency)
- [FrugalGPT: How to Use Large Language Models While Reducing Cost and Improving Performance (arXiv:2305.05176)](https://arxiv.org/abs/2305.05176)
- [Prompt caching — Claude Platform Docs (Anthropic)](https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching)
- [Parallel tool use — Claude Platform Docs (Anthropic)](https://platform.claude.com/docs/en/agents-and-tools/tool-use/parallel-tool-use)
- [Running agents (max_turns / MaxTurnsExceeded) — OpenAI Agents SDK](https://openai.github.io/openai-agents-python/running_agents/)
- [GRAPH_RECURSION_LIMIT — LangGraph Docs (LangChain)](https://docs.langchain.com/oss/python/langgraph/errors/GRAPH_RECURSION_LIMIT)
- [Cancellation — Model Context Protocol Specification](https://spec.modelcontextprotocol.io/specification/draft/basic/utilities/cancellation/)

### [L1 理解] 什么是 exponential backoff with jitter？为什么在重试外部 LLM API 调用或工具调用时，仅做指数退避是不够的，必须再加入随机抖动？

- **考察点**：考察候选人是否理解重试机制不是拍脑袋的 sleep(2^n)，而是有明确的分布式系统失败模式（thundering herd）作为动机，这是任何调用外部 LLM/工具 API 的 agent 系统都会遇到的基础问题。
- **参考答案要点**：
  - 重试针对瞬态错误：5xx、429 rate limit、连接超时，永久性错误（4xx 参数错误、401 鉴权失败）不应重试
  - AWS 的实验结论：纯指数退避只是降低了重试频率，但并未打散并发客户端的重试时间点，集群仍会同步碰撞（thundering herd 未被消除）
  - 加入 jitter（Full Jitter / Equal Jitter / Decorrelated Jitter）才能让并发客户端的重试时间错开，显著降低竞争工作量和总恢复时间
  - 必须设置最大重试次数/总超时上限（deadline），否则退避会无界增长导致用户侧超时或成本失控
- **追问**：
  - 如果一个 LLM API 返回 429 但响应头带了 Retry-After，你的退避策略应该如何调整？
  - 对于流式（streaming）响应，请求在中途失败重试会遇到什么额外问题？

### [L1 理解] 在 agent 的工具调用场景中，如何判断一个工具调用是否可以安全重试（idempotent）？请设计一个方案来处理'调用超时但不确定是否已生效'的写操作。

- **考察点**：考察对 idempotency 概念本身及其在 agent 工具执行链路中落地方式的理解——这是所有自动重试机制成立的前提条件，也是面试中区分'背过退避算法'和'真正理解为什么要退避'的分水岭问题。
- **参考答案要点**：
  - idempotent 的定义：重复执行不改变最终状态、无累加副作用（Google Cloud：可重复执行且每次都收敛到相同终态）
  - 读类/只读工具天然幂等，可无条件重试；写类/有副作用工具（发邮件、下单、写文件追加）默认不安全重试
  - Stripe 模式：client 生成 idempotency key 随请求下发，服务端缓存首次执行的结果（含错误），重放时直接返回缓存结果而非重新执行
  - 超时场景的关键矛盾：客户端不知道请求是在到达服务端前失败还是执行后响应丢失，必须由服务端做幂等去重而不能靠客户端猜测
  - 对 agent 而言：非幂等工具的超时应该先做一次带副作用检查的读操作（如检查订单是否已创建）而非直接重试
- **追问**：
  - 如果某个第三方工具本身不支持 idempotency key，你会在 agent 侧如何补一层幂等保证？
  - 只读工具的'幂等'假设在什么情况下会被打破（例如工具背后调用了有状态的搜索索引）？
- **对应 nanocodex**：工具系统 动态暴露（03_tools.md）：tools.rs 中按 read_only 属性分类工具，本质上是把'副作用类别判定'作为并发/重试安全性的前置门槛，与幂等性判断是同一类工程决策，但 nanocodex 目前只用它驱动并发调度，未见显式的 retry/idempotency-key 机制。

### [L2 权衡] cheap/strong 模型分级路由（model tiering / LLM cascade）应该如何设计？在什么信号下应该从便宜模型升级到更贵模型，这种设计与固定的'先分类再路由'方案相比有什么权衡？

- **考察点**：这是成本工程里最有'含金量'的设计题，考察候选人是否知道分级路由不是简单的if-else规则表，而是有质量估计和成本-延迟权衡的系统设计问题，也是绝大多数生产 agent 系统（含多档模型定价）必须面对的现实约束。
- **参考答案要点**：
  - FrugalGPT 的 LLM cascade：按序调用 cheap → expensive 模型，用一个 post-query quality estimator 判断当前答案是否'足够好'，不够才升级，实测可在几乎不损失精度下降低 50%~98% 成本
  - cascade（先跑后判）vs a priori 路由（先分类任务难度再选模型）的权衡：cascade 对每个 query 都要承担至少一次 cheap 模型调用的延迟开销（多一跳），a priori 路由无额外调用但分类器本身的误判会直接决定整条链路质量
  - quality estimator 阈值的校准问题：阈值过松→掉进'虚假自信'陷阱（cheap 模型给出看似合理但错误的答案未被拦截），阈值过紧→cheap 模型形同虚设、成本收益消失
  - agent 场景的额外复杂度：不是单轮 QA 而是多轮工具调用循环，'升级'的判断信号可以是工具调用失败次数、模型输出的自我不确定性表达、或任务复杂度分类（如 orchestrator 的 classify 节点）
- **追问**：
  - 如果 cheap 模型给出的答案'看起来正确'但实际错误（无法通过简单规则识别），你的质量估计器该如何设计？
  - 分级路由和 prompt caching 同时使用时，为什么频繁切换模型档位可能反而抵消缓存收益？
- **对应 nanocodex**：分层编排器 orchestrator flash/pro（05_orchestrator_comm.md）：orchestrator.rs 的 classify 节点按任务复杂度决定走 flash（cheap）还是 pro（strong）路径，这是一种 a priori 路由而非 FrugalGPT 式的 cascade escalation，二者机制不同但解决的是同一类问题，可作为对比案例。

### [L2 权衡] Anthropic 的 prompt caching 是如何组织 cache breakpoint 和失效层级的？在设计一个多轮 agent 的 system prompt / tool schema / 对话历史布局时，你会如何安排内容顺序来最大化缓存命中率，这与上下文压缩（context editing）之间有什么冲突？

- **考察点**：考察候选人是否理解 prompt caching 不是'开个开关就好'，而是有严格的层级失效规则，需要在 prompt 结构设计阶段就考虑，并且要和上下文管理（压缩/裁剪）联合权衡，这是成本优化里最容易被工程师忽略的细节。
- **参考答案要点**：
  - Anthropic 缓存失效层级为 tools → system → messages，任一层级发生改动会级联使其后所有层级的缓存失效，因此工具定义和系统提示应放在最前面且尽量稳定不变
  - ephemeral cache 默认 TTL 5 分钟（可扩展到 1 小时），cache-read token 价格约为标准 input 的 10%，但首次写入（cache write）成本略高于标准 input，需要有足够的重复调用次数才能摊平
  - 并发请求场景下，cache entry 要等第一个请求开始返回响应后才可用，因此并行发出的第一批请求之间不会互相命中缓存
  - 与 context editing/compaction 的冲突：为了控制 token 预算而对历史 tool 结果做压缩/丢弃最老前缀，会改变 messages 数组内容从而使已建立的 cache 前缀失效，本质是'压缩省的 token' vs '重算 cache 花的 token'之间的权衡，需要按压缩频率和缓存 TTL 联合调参
  - exact-prefix caching（KV cache 复用，需要请求内容逐字节相同）与 semantic caching（基于 embedding 相似度的近似缓存）是两种不同机制，后者引入了正确性风险（相似但不等价的问题被误判为缓存命中）
- **追问**：
  - 如果 agent 的 system prompt 里包含当前时间戳等易变字段，你会如何调整 prompt 结构以避免频繁打破缓存？
  - semantic caching 用于 LLM 响应层面时，如何设置相似度阈值来控制'误命中'风险？
- **对应 nanocodex**：上下文压缩 context editing（02_context_compression.md）：session.rs 的 edited_body 两趟压缩算法（压缩旧 tool 结果 + 超预算丢最老前缀）与非破坏发送视图 for_model_edited，恰好是会与 prompt caching 前缀稳定性产生直接博弈的机制——每次压缩都可能使已建立的 cache breakpoint 失效。

### [L3 深挖/故障] 当模型在一次响应里返回多个 tool_use blocks 要求并行执行时，可能出现哪些故障模式？请举例说明什么情况下'并行工具调用'会导致数据不一致或竞态问题，以及如何在协议/框架层面规避。

- **考察点**：这是一道深挖故障的题，考察候选人是否真正在生产环境里踩过并行工具调用的坑，而不是只知道'Claude/GPT 支持 parallel tool calls'这个表面特性。
- **参考答案要点**：
  - Anthropic tool-use API 明确：协议本身不规定多个 tool_use blocks 的执行顺序，是并发执行还是顺序执行完全是调用方（agent harness）自己的责任
  - 故障模式一：两个 mutating 工具并发写同一资源（如同时写文件同一行、同时修改同一条数据库记录）导致的写写冲突或后写覆盖前写
  - 故障模式二：一个工具的输出是另一个工具的隐含前提（顺序依赖），但模型把它们放进了同一批 parallel 调用中，若都并发执行会导致依赖工具读到过期/不存在的状态
  - 故障模式三：client tool 与 server tool（如 web_search、code_execution 这类在 Anthropic 基础设施上运行的工具）混在同一批 parallel 调用组里时，协议要求在 server tool 真正执行前把控制权交还调用方，如果没处理这个 handoff 会导致状态机卡住
  - 规避方案：按工具的副作用声明（read_only / mutating）做静态分类，只对声明为 read_only 的工具组自动并发，mutating 工具默认串行或需要显式的依赖图/锁；对不确定依赖关系的场景可用 disable_parallel_tool_use 强制模型一次只给一个 tool_use
- **追问**：
  - 如果两个只读工具调用了同一个有状态的搜索索引（存在最终一致性延迟），read_only 假设为什么可能失效？
  - 你会如何设计一个'依赖图'机制，让模型能表达工具调用之间的顺序依赖而不是让 harness 硬编码规则？
- **对应 nanocodex**：Harness 工程管理（01_harness.md）：AgentLoop 中'只读并发批量(parallel_run)'机制正是按 read_only 属性对工具调用做静态分类后再决定是否并发执行，直接对应此题里'如何在框架层面规避并行工具调用故障'的解法。

### [L4 开放设计] 如果让你从零设计一个 agent loop 的'预算与终止保证'机制（避免死循环、无限重试、无限模型升级），你会如何组合调用预算、超时/取消协议和异常行为检测？请说明单纯设置一个 max_turns 数字上限为什么不够。

- **考察点**：这是一道开放设计题，考察候选人能否跳出'加个计数器'的初级思路，综合考虑预算类型的分层设计、优雅降级（而非硬崩溃）、以及模型本身'不愿意停止'这种行为层面的风险，是agent工程里最容易被低估的可靠性问题。
- **参考答案要点**：
  - 单一 max_turns/recursion_limit 的局限：LangGraph 社区实践证明，如果 agent 本身陷入死循环，单纯调高上限只是'多付费跑更多轮次直到力竭'，不能解决根因，必须叠加'无进展检测'（如连续 N 轮工具调用结果/参数完全重复则提前终止）
  - 预算应分层设计而非单一计数：区分 max_model_calls（思考/决策轮次）与 max_tool_calls（实际动作次数）能分别捕捉'模型话痨但不干活'和'狂调工具但不收敛'两类不同的失控模式
  - 终止时应做优雅降级而非硬异常：OpenAI Agents SDK 的 error_handlers、LangGraph 的 RemainingSteps 都是在预算耗尽前主动收尾返回当前最佳结果，而不是抛出 MaxTurnsExceeded 类异常让上层业务崩溃
  - 超时与取消应该是协作式的、可组合的：MCP 规范定义了 per-request timeout + notifications/cancelled 双向通知，允许在等待中的请求被主动中断而不是被动等到超时，且允许通过 progress 通知续期非死循环的长任务
  - 预算耗尽只是'停止执行'的触发条件之一，还需要和'循环/无进展检测'配合，因为陷入死循环的 agent 往往不会自己报告失败，纯计数器无法区分'任务本身很复杂需要很多步'和'卡在同一个逻辑坑里出不来'
- **追问**：
  - 如果要检测'无进展'（agent 在重复做同一件事），你会用什么信号（工具调用参数相似度？模型输出的语义重复？）来判断，误判率如何控制？
  - 两层协作式取消（如 100ms 轮询 select）相比同步阻塞式取消，在正确性和延迟上分别有什么代价？
- **对应 nanocodex**：Harness 工程管理（01_harness.md）：AgentLoop::run_turn 的双预算(max_model_calls=60/max_tool_calls=120)保证终止，以及两层协作式取消(100ms select!)，正是此题所要求设计的机制在 nanocodex 里的真实实现，可直接对照讲解分层预算+协作式取消的落地方式。

---

## 7. 评估、可观测性与持续改进

**工程定义**：该类别的核心工程概念可拆成四根互相咬合的支柱:(1) Agent benchmark/任务设计——评测对象是 model+tools+memory+guardrails 组成的 compound system,不能只测 model-level 正确率,任务必须配一个可靠的 verifier/grader;arXiv:2507.02825《Establishing Best Practices for Building Rigorous Agentic Benchmarks》系统盘点了 SWE-bench-Verified、GAIA、τ-bench、WebArena 等 17 个主流 agentic benchmark 里的 verifier 缺陷(例如 SWE-bench-Verified 测试用例覆盖不足、τ-bench 把空响应误判为成功),这类 under-specified reward/verifier 会造成 agent 表现被高估或低估最高达 100% 的相对误差;(2) LLM-as-judge——用强 LLM 替代/辅助人工来评测开放式输出,奠基工作是 arXiv:2306.05685《Judging LLM-as-a-Judge with MT-Bench and Chatbot Arena》(Zheng et al., NeurIPS 2023),它用 MT-Bench(58 位专家多轮问答)和 Chatbot Arena(众包对战)验证强 LLM judge 与人类偏好一致率可达 80%+、接近人类专家间一致率,同时系统刻画了 judge 的 position bias、verbosity bias、self-enhancement bias 及有限推理/数学能力等系统性偏差,并提出交换位置、reference-guided grading 等缓解手段;(3) Trace/可观测性——OpenTelemetry 的 GenAI Semantic Conventions(spec: gen-ai-agent-spans,自 2024 年 4 月 GenAI Observability SIG 成立后持续演进)为 model 调用和 agent 级操作定义标准 span(agent span kind=CLIENT,继承并扩展 base GenAI span 约定,覆盖 token usage、latency、工具调用参数/结果),使工具调用、模型调用、多 agent 交互结构可被跨框架(LangChain/CrewAI/AutoGen 等)一致记录;Anthropic 工程博客《Writing effective tools for AI agents》进一步给出实践做法:trace review(读原始 transcript,含工具调用与返回)是定位 agent 卡住/困惑位置、诊断冗余工具调用(分页/token 限制问题)与频繁工具错误(描述不清)的一手手段;(4) 评测驱动的持续改进(Evaluation-Driven Development, EDD)——arXiv:2411.13768《Evaluation-Driven Development and Operations of LLM Agents: A Process Model and Reference Architecture》指出传统 TDD/BDD 假设确定性系统与固定规约,不能直接套用到开放式、持续自适应的 LLM agent,需要专门的评测驱动流程模型,把评测作为开发生命周期一等公民而非事后检查;Anthropic《Demystifying evals for AI agents》进一步区分 offline evaluation(开发期用固定任务集迭代)与 production observability(线上监控分布漂移)两个互补维度,并指出 Harbor、Braintrust 等工具分别覆盖容器化规模化跑分与评测+生产观测+实验追踪一体化两类典型形态。

**来源**：
- [Judging LLM-as-a-Judge with MT-Bench and Chatbot Arena (arXiv:2306.05685)](https://arxiv.org/abs/2306.05685)
- [Establishing Best Practices for Building Rigorous Agentic Benchmarks (arXiv:2507.02825)](https://arxiv.org/html/2507.02825)
- [Evaluation-Driven Development and Operations of LLM Agents: A Process Model and Reference Architecture (arXiv:2411.13768)](https://arxiv.org/pdf/2411.13768)
- [Demystifying evals for AI agents — Anthropic Engineering](https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents)
- [Writing effective tools for AI agents—using AI agents — Anthropic Engineering](https://www.anthropic.com/engineering/writing-tools-for-agents)
- [Semantic Conventions for GenAI agent and framework spans — OpenTelemetry](https://opentelemetry.io/docs/specs/semconv/gen-ai/gen-ai-agent-spans/)
- [τ-bench: A Benchmark for Tool-Agent-User Interaction in Real-World Domains (arXiv:2406.12045)](https://arxiv.org/abs/2406.12045)

### [L1 理解] 什么样的 agent benchmark 任务设计算合格?常见的 verifier/reward 设计缺陷有哪些,会造成什么后果?

- **考察点**：检验候选人是否理解“跑得出分数”和“分数可信”的区别,是否知道公开 benchmark 本身也可能有严重设计缺陷,而不是把 published benchmark 当金标准照单全收。
- **参考答案要点**：
  - 评测对象是 model+tools+memory+guardrails 组成的 compound system,不能只测 model-level 正确率
  - 任务需要客观可验证的成功判据(如 SWE-bench 用 fail-to-pass 测试执行,而非模型自评)
  - arXiv:2507.02825 指出 SWE-bench-Verified 测试用例覆盖不足、τ-bench 把空响应算通过,这类 verifier 缺陷可致最高 100% 相对误差的高估/低估(reward hacking)
  - 需要 held-out/frozen 测试集,防止对已知 eval set 过拟合
- **追问**：
  - 如果你维护的 eval 集通过率一直很高但线上体验差,你会怎么排查?
  - 如何设计任务防止“空响应/拒绝作答”被误判为成功?
- **对应 nanocodex**：ncx-forge subsystem — evaluator.py/forge.py 里 empty eval → Objectives(passrate=0, cost=+inf) 的映射(forge.py:291),专门防止“零任务/空跑”静默伪装成绿色冠军,正对应 τ-bench 空响应算通过这一类坑的工程应对。

### [L2 权衡] 什么时候该用 LLM-as-judge,什么时候该用规则/exact-match grading?LLM judge 本身有哪些系统性偏差,怎么缓解?

- **考察点**：权衡题,考察候选人是否只会无脑上强模型当裁判,还是理解 judge 的适用边界、已知偏差以及工程缓解手段。
- **参考答案要点**：
  - exact-match/程序化 grader 适合有唯一正确答案或可执行验证的任务(如 GAIA 精确匹配、SWE-bench 跑测试);LLM-as-judge 适合开放式、多维度、无单一标准答案的输出
  - arXiv:2306.05685 验证强 LLM judge 与人类专家一致率可达 80%+,但存在 position bias、verbosity bias(偏爱更长回答)、self-enhancement bias(偏爱同源模型输出)及有限推理/数学能力
  - 缓解手段:交换回答顺序消偏、reference-guided grading(给 judge 参考答案)、拆成 rubric 维度打分而非整体一次性打分、多 judge 集成
  - 需要一个人工标注的校准子集持续验证 judge 与人类的一致率,而不是部署后就不再检查
- **追问**：
  - 如果 judge 打分和人工评分系统性不一致,你会先怀疑 prompt 设计还是模型能力?
  - 怎么防止 judge 被 agent 输出里的对抗性文本影响、打出虚高分数?

### [L3 深挖/故障] 怎么设计 agent 的 trace/日志,让你能在不复现问题的情况下诊断一次线上失败?

- **考察点**：深挖故障场景,考察候选人是否有用 trace 排障的实操经验,以及是否知道该在什么粒度记录哪些字段。
- **参考答案要点**：
  - OpenTelemetry GenAI Semantic Conventions 为 model 调用与 agent 级操作定义标准 span(agent span kind=CLIENT,继承并扩展 base GenAI span 语义,覆盖 token usage、latency、工具调用参数/结果)
  - 多 agent 场景要抓的不只是单 agent 行为,还要抓 decision pattern 和交互结构,否则协作失败几乎无法定位根因
  - Anthropic《Writing effective tools》给出可操作动作:读原始 transcript(含工具调用/返回)找卡住点;高频重复工具调用提示分页/token 限制问题;高频参数错误提示工具描述不清
  - 失败轨迹进入下游分析/反馈闭环前要做敏感信息脱敏
- **追问**：
  - 如果 trace 显示 agent 反复调用同一工具却拿到相同错误,你会先怀疑 harness 层还是 prompt 层?
  - 全量记录 trace 和采样记录 trace 各自的取舍是什么?
- **对应 nanocodex**：ncx-forge subsystem — evaluator.py 的失败轨迹采集(evaluator.py:104-197):从 agent 自己的 session.jsonl 里留最后一条 assistant 消息 + 最后 12 个工具调用当诊断/训练信号,并对含 GRADER_MARKERS 的行做 _redact 脱敏,是 nanocodex 里“trace 驱动迭代”最贴近的具体实现。

### [L3 深挖/故障] 如果要在训练/优化流程里加一个“回归自检门”(self-check gate),防止一次改动静默失效却被误判为有效,你会怎么设计这个门?

- **考察点**：深挖机制设计题,考察候选人是否理解“确定性探针”与“用真实任务当探针”的本质区别,以及重试策略里的非对称陷阱。
- **参考答案要点**：
  - 用确定性、与任务语义无关的探针(如让 agent 回显一个暗号)而不是用真实任务冒烟测试——真实任务里任务指令本身会和“改动是否生效”竞争,产生噪声、难判定
  - 门至少要双向验证:改动确实注入生效(injected)且未注入时确实不出现(absent_baseline),只查一边无法排除巧合触发或从未生效两类假阳性/假阴性
  - 重试策略要区分模型偶发噪声导致的漏报和该严格拒绝的情形,两边重试次数不对称本身是需要被复核的设计假设,而非默认安全
  - 门未通过前,不应把后续基于该改动的评测结果当可信数据消费
- **追问**：
  - 如果“不该出现”检查偶发触发了一次误报,门要怎么处理才不会误杀一次正常改动?
  - 这类自检门要跑在每次改动之后,还是可以采样跑?
- **对应 nanocodex**：ncx-forge subsystem — SENTINEL 自检门(forge.py:51-112,493-495):写入让 agent 只回暗号 NCXFORGE_SENTINEL_4242 的基因组,跑通(injected)且基线无暗号(absent_baseline)才 PASS,否则除非 --no-gate 拒绝训练;其已知坑是 injection-check 重试 3 次而 absence-check 只跑一次,是非对称 retry 设计的真实案例。

### [L2 权衡] 怎么把 prompt/harness 的迭代,从“改完跑几个 case 看着顺眼就发”变成一个可信的、可持续跑的评测驱动流程?

- **考察点**：考察候选人是否理解 Evaluation-Driven Development(EDD)的工程含义而非表面理解成“写几个测试用例”,以及是否知道防止对已知 eval 集过拟合的机制。
- **参考答案要点**：
  - arXiv:2411.13768 指出传统 TDD/BDD 假设确定性系统与固定规约,不能直接套用于开放式、持续自适应的 LLM agent,需要专门的 evaluation-driven 流程模型,把评测当作开发生命周期一等公民
  - Anthropic《Demystifying evals for AI agents》区分 offline evaluation(开发期用固定任务集迭代)和 production observability(线上监控真实分布漂移),二者互补缺一不可
  - 需要 held-out/frozen 测试集机制防止“改 prompt 改到过拟合已知 eval 案例”——开发时用的评测集和最终验收集要分离,验收集只打一次分
  - 每次 harness/prompt 改动都应跑同一套回归评测集而非每次手挑 case,形成版本可对比、可回滚的迭代记录
- **追问**：
  - 如果某次 prompt 改动让 eval 分数涨了但线上反馈变差,你会先怀疑评测集本身出了什么问题?
  - 多大规模的团队/项目值得投入建这样一套评测驱动闭环?
- **对应 nanocodex**：ncx-forge subsystem — train() 的噪声感知爬山机制里 accept_margin 接受规则 + holdout 防过拟合 + frozen test 只打一次分(forge.py:233-237),是“防止对已知 eval 集过拟合、区分训练信号与无偏验收”这套工程原则的具体实现。

### [L4 开放设计] 从零给一个新的多步 web 调研 agent 设计评测体系:任务集、grader/judge、trace、回归门,你会怎么搭,重点防哪些坑?

- **考察点**：开放设计题,综合考察候选人能否把基准设计、LLM-judge、trace、回归门四件事串成一个完整、自洽、能落地演进的评测体系,而不是零散罗列工具名词。
- **参考答案要点**：
  - 任务集设计参考 SWE-bench 的 fail-to-pass 思路(客观可执行验证而非模型自评)与 τ-bench 的多轮工具-用户交互结构;显式规避 arXiv:2507.02825 点名的常见坑(verifier 覆盖不足、空/退化响应误判成功、环境非确定性未被察觉)
  - 开放式产出(如调研报告质量)用 LLM-as-judge,并按 arXiv:2306.05685 的经验加 reference-guided grading、位置交换、多 judge 集成缓解偏差,同时留一个人工校准子集持续验证 judge 一致率
  - 全链路 trace 按 OpenTelemetry GenAI Semantic Conventions 记录 model/tool/agent span,支撑失败复盘和多步骤归因
  - 上线前用确定性自检门(而非跑几个真实任务看着顺眼)验证 harness 改动本身未静默破坏,再叠加 held-out/frozen 集合防止对已知任务过拟合,形成可持续回归的评测驱动迭代闭环
- **追问**：
  - 如果预算有限,这四块你会先砍哪一块、先保哪一块?为什么?
  - 怎么判断你的评测集本身已经过时,需要换一批任务?
- **对应 nanocodex**：ncx-forge subsystem 整体训练闭环(SENTINEL 自检门 + evaluator.py 失败轨迹采集 + train()/evolve() 的 holdout/frozen/Pareto 机制 + splits.py 的 train/val/test 划分),是 nanocodex 里最完整对应“评测驱动持续改进闭环”的具体子系统,可作该设计题的一个具体参照实现。

---

## 用法建议

- **先定坐标系，再钻细节**：面试官问"XX 怎么通讯/怎么管理"这类问题时，先用本题库里对应类目的标准工程定义把问题分类到正确的范式（比如先说清是 supervisor-worker 还是 blackboard，是 workflow 还是 agent），再具体化到 nanocodex 的代码实现——这个顺序本身就是在展示"懂行业共识 + 有工程落地"两层能力。
- **引用来源要能扛住追问**：本题库每条工程定义都标了真实来源（论文/协议规范/大厂工程博客），面试时可以直接提作者/年份/发布方（如 "ReAct，Yao et al. 2022"、"Anthropic 2024 年 12 月的 Building effective agents"），比空泛地说"业界普遍认为"更可信，但不要背题库里的数字当自己实测的结果。
- **对应 nanocodex 的题优先深挖**：这些题在 [nanocodex-portrait.zh.md](nanocodex-portrait.zh.md) 里有对应子系统卡，可以用真实代码机制（函数名、行号、常量）具体化答案，这是把"通用理论"变成"我真做过"的关键一步。
- **没有 nanocodex 对应的题也要能独立作答**：部分题（如 prompt caching、benchmark 设计缺陷）在 nanocodex 里没有直接对应实现，这类题考的是你对行业实践的理解广度，作答时不必强行往 nanocodex 上靠。
