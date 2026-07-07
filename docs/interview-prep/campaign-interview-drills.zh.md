# campaign 多 Agent 编排 · 精炼刷题卡（口播自检版）

> 对 `agentbot/campaign`（多 Agent 编排框架）刷题、录语音。少而尖：1 总纲 + 必刷 3 + 补充 2 + 选刷 1。
> 刷法：① 只看【题目】录一段 → ② 对【答题骨架】自检 → ③ 背【一句话】 → ④ 想【换业务/易错点】。
> 配套流程图：`campaign-flowcharts.zh-CN.html`。核心主张：**不是更多 Agent，而是更好的指挥系统。**

---

## Q0 · 总纲（场景迁移）
**题目：** 同一套 agent harness 换业务，哪些环不变、哪些环重新「承重」？拿你的多 agent 框架举例。

- **考点：** 可迁移性 + 场景决定承重。
- **答题骨架：**
  1. **不变骨架**：prompt 钉角色边界 / context 供可信数据 / 工具面收窄副作用 / ground-truth 校验 + 人审兜出口 / LLM 原始输出不直接落地。
  2. **变的承重**：LLM-native 还是 LLM-as-interface → LLM 是判断者还是被编排者。
  3. **风险形态**：怕假阴性还是假阳性 / 还是**状态不一致与雪崩**（多 agent 特有）。
  4. **campaign 落点**：多 agent 协作，最怕状态漂移和连锁失败 → **Workflow(编排+A2A) + Memory(事件溯源) + Guardrails(治理)** 三环承重。
- **一句话：** 同一套方法论——合规长成 RAG+judge、排产长成确定性 workflow、编码长成沙箱审批、**多 agent 长成事件溯源 + DAG 编排 + 全链路治理**；骨架同，承重随业务。

---

## Q1 · 事件溯源 【必刷】
**题目：** 多个 agent 协作，状态怎么管？为什么不用一个共享的可变黑板？

- **考点：** Memory 环 · 可恢复/可审计的高性价比做法。
- **答题骨架：**
  1. **不存可变共享态**：所有动作 append 成 `Event`，当前态 = `derive_state(events, run_id)` 纯函数回放派生。
  2. **run_id 隔离**多运行不混流；`truncate_after(seq)` 是真回滚/分叉。
  3. **收益**：可回放、可审计、并发安全；天然 **durable resume**（换进程/设备读同一事件库 replay 即恢复）。
  4. 事件总线队列满时 `dropped_events` 计数<b>不静默丢</b>。
- **一句话：** 事件溯源把"状态一致性"问题转成"事件顺序"问题——不存可变态，状态永远由回放派生。
- **易错点：** 会爆吗？会，所以 run_id 隔离 + 派生视图缓存；高并发写要换共享有序日志(Postgres/Kafka)，已在 SCALING 标明。

---

## Q2 · 编排 / 无隐藏状态 【必刷】
**题目：** agent 之间怎么协作？谁决定谁干活？会不会互相偷偷改状态？

- **考点：** Workflow 环 · 编排的确定性 + 职责分离。
- **答题骨架：**
  1. **Coordinator 拆解 → DAG 拓扑分层调度**：`topological_layers` 按 `depends_on` 分层，层内 `Semaphore` 限流并发，依赖未完 → `task.skipped`。
  2. **无隐藏共享状态**：agent 间不靠可变全局对象通信，一切经事件 + 父节点显式派发。
  3. **裁判/运动员分离**：Coordinator 只调度，**Reviewer 独立验收**，不让 worker 自评自夸（无 reviewer 可 fail-closed）。
  4. 单任务链多道闸：制动 → HITL 审批 → 选 agent(跳熔断) → PolicyGate → 超时执行 → 验收。
- **一句话：** 像分布式系统一样设计角色与不变量——DAG 定依赖、事件定状态、裁判独立于运动员。
- **换业务：** 需要 peer 协商的（售后决策↔风控）加 A2A `query`，但保留"父节点唯一中枢"防协商环路。

---

## Q3 · PolicyGate 全链路治理 【必刷】
**题目：** 预算 / 权限 / 数据出域这些红线怎么保证不是摆设？

- **考点：** Guardrails 环 · Policy-as-Code + 并发安全。
- **答题骨架：**
  1. **单一执行闸 PolicyGate**：`Action{actor,kind,cost,sensitive}` 过 Governor + 规则链——BudgetRule / DataEgressRule / AuthorityRule / InjectionScanRule。
  2. **对所有角色一视同仁**：Coordinator 自己的动作也受检；督军独立上报，不受 Coordinator 节制。
  3. **并发安全**：`asyncio.Lock` 序列化预算读改写——asyncio 单线程也有跨 `await` 竞态。
  4. **全链路 vet**：Runtime 每任务 + LLMClient 每次调用共享同一闸；违规写 `governance.violation`。
- **一句话：** 制度治理没有爱——Policy-as-Code + 独立督军 + 全链路每个真实动作边界都 vet。
- **易错点（★加分）**：审查时发现一批安全机制"**接了线但没被真实信号驱动**"（预算 cost 恒 0、health 没人更新）= 形同虚设——要顺数据流验证"看起来接了 ≠ 真在起作用"。

---

## Q4 · A2A + 幂等 【补充】
**题目：** 本地 agent 和远程 agent 怎么用同一套协议？重试会不会导致重复执行？

- **考点：** 接口设计 / 分布式两条铁律。
- **答题骨架：**
  1. **依赖倒置**：定义 `Message/Part/AgentCard` + `Transport` 抽象；编排只依赖抽象，换 `InProcess ↔ HTTP/JSON-RPC ↔ SSE` 实现即单机→分布式，**不动编排**。
  2. **幂等是重试前提**：HTTP 加重试+指数退避(仅瞬时错误)后，必须配 `message_id` 幂等去重，否则 worker 重复执行/重复扣预算。
  3. **能力发现**：`agent/cards` 让协调端自动注册远端。
  4. **信任边界**：发送方 allowlist 防伪 + `untrusted` 标记 + 注入扫描。
- **一句话：** 依赖倒置让传输可换、幂等是重试的前提——这是分布式调用的两条铁律。
- **换业务：** 跨部门/跨机的 agent 协作直接换 HTTP transport；本地演示用 InProcess，语义一致。

---

## Q5 · 抗毁性：熔断 / 减员 / HITL 【补充】
**题目：** 某类 agent 大面积失能，系统怎么不崩？

- **考点：** 韧性工程。
- **答题骨架：**
  1. **两层抗毁**：单点**熔断**（≥3 失败 tripped，cooldown 自愈）+ 链路**协同制动**（broadcast `brake.signal`，上游背压停派活防雪崩）。
  2. **三级减员**：`CapacityLedger.assess` 按 health(真实失败驱动)定级 → LIGHT 替补 / MEDIUM 降级(checkpoint+弱模型+Reviewer 加严) / SEVERE 战时动员(+human 升级)。
  3. **降级原则**：执行可降级、**把关不降级**（Reviewer 仍用原档且更严）、高难宁冻结。
  4. **HITL durable resume**：`input_required` 事件持久，跨进程 `resume` 恢复。
- **一句话：** 熔断停单点、制动防雪崩、减员补兵力、把关永不降级——把系统的"响亮失败"设计成一等事件。
- **换业务：** 高可用生产场景把预备队/leader/共享 breaker 补齐（当前是每进程，见 SCALING）。

---

## Q6 · 诚实的工程边界 【选刷 · ★判断力加分】
**题目：** 你这个框架有没有没做完的地方？分布式一致性到位了吗？

- **考点：** 判断力 / 可测性即"做没做"的判据。
- **答题骨架：**
  1. 分布式是**可运行 MVP**（HTTP + 共享 SQLite 事件库 + remote proxy）；生产级一致性需共享预算/health/breaker/leader——**没有就不假装做**，写进 `SCALING.md`。
  2. 长期知识默认词法 TF-IDF，非神经 embedding（留 adapter 缝）。
  3. 测试全离线（mock provider + 本地 store），不需要 key/网络。
- **一句话：** 诚实标注未实现边界，胜过假装完成——没有可测的真实底座时硬做就是自欺；接口都留成抽象(EventLog/Transport/Gate)，需要时换实现。

---

### 刷题优先级
- **先刷 Q1 / Q2 / Q3**（事件溯源、DAG 编排无隐藏状态、PolicyGate 治理）——campaign 最差异化、最能体现"像分布式系统一样设计 agent"。
- **Q0 开场框架**；**Q3 的"接了线没被信号驱动"** 和 **Q6 诚实边界** 是两个高分追问点。
- **口径统一**：campaign"数据不出域"(DataEgressRule) = 网关脱敏 = 排产敏感本地 = 合规通道隔离，同一信条不同承重。
