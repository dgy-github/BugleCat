# 工具工程 + ReAct 执行范式 · 精炼刷题卡（口播自检版）

> 第 4 个横切主题。此前散在项目里没单独成题，现补齐。挂钩 nanocodex `agent_loop.rs`/`orchestrator.rs`/`tools.rs`/`approval.rs`。
> 刷法：只看【题目】录音 → 对【骨架】自检 → 背【一句话】。★备弹是被深挖时的第二发。
> 配套可视化：`tool-engineering-react-flowcharts.zh-CN.html`。

## 题眼（先记这句）
**工具工程收窄"模型能做什么"，执行范式约束"模型做多久/失败怎么办"**。三条贯穿不变量：① 分类权不给模型(代码静态声明+保守白名单) ② 中途退出必 backfill 保历史合法(否则 400) ③ 语法合法≠语义正确(schema 断言+有界重试+显式降级)。

---

## Q0 · 总纲【必刷】给一个会跑命令/改文件/调外部 API 的 coding agent，怎么从"工具设计"和"执行范式"两个维度把它做可控？
**骨架**：两条主线。工具侧：五判据(副作用可控/参数可校验/读写分明/失败可观测/单次有边界)→读写分离(`read_only()` 静态声明，读 `join_all` 并发免批，写串行过审批)→权限做成纯函数 `Approver::classify` 四档→危险能力**物理隔离**(read-only 下 `can_write` 恒 false、plan mode 拒所有写)→所有调用必经 `ToolRegistry::execute` 的 pre/post-hook 挂闸。执行侧：ReAct 有界循环(`for 0..max_model_calls`，双预算取小)驱动主流程，高复杂度切 orchestrator 的 Plan(classify→plan 无工具→worker 并行 best-of-N→verify)。贯穿三不变量(见题眼)。

## Q1 · 【必刷】ReAct 和 Plan-and-Execute 怎么选？为什么项目里两个都做了？update_plan 算 Plan-and-Execute 吗？
**骨架**：口诀——**不确定就 ReAct(自适应)，要可控就 Plan(可审可并行可分层)**。nanocodex 双实现：coding 主循环用 ReAct(改哪个文件/要不要先 grep 得看实时结果，列不出精确清单)；orchestrator 对 classify 判 high 的任务才切真 Plan(`PLAN_SYS` 写死 "You have NO tools" 防强模型规划阶段偷改代码，破坏 best-of-N 隔离)。**`update_plan` 不算真 Plan**——它是 ReAct 里的软清单(advisory，只给 UI 展示不改控制流，没有独立执行器)。真 Plan 必须有独立于规划的执行节点。

## Q2 · 【必刷】执行清单某一步失败了，重试同一步还是整体重规划？边界怎么定？重试用尽了怎么办？
**骨架**：按失败性质**四选一**：瞬时/环境错(网络抖动/超时/锁)→重试同一步；方案错/前提被推翻→整体重规划；达标不了有次优解→降级；越权/需人判→上报。分界线=**错误局部且计划仍成立就重试，错误全局或路线错就重规划**。三条硬边界缺一不可：`max_steps`(总步数)、`max_retries`(单步重试)、`terminate`(什么算完成/彻底失败)。**重试必须喂反馈**(FAIL 理由拼进下轮 prompt，不喂反馈=抽卡)。用尽仍不过：`promote_worker(best)` 落地打 "[unverified after retries]"(**降级而非硬失败或假装成功，fail-loud**)。**诚实点**：当前是 classify 静态分流(simple 走重试/high 走重规划)，不是运行中按失败性质动态切换，更强设计是让 verifier 多输出"该重试还是该重规划"信号。

## Q3 · 【必刷】结构化输出怎么保证"能被程序消费"？JSON mode 已经保证合法 JSON 了，为什么还要 schema 校验？
**骨架**：三条路可靠性递增通用性递减：function calling(arguments 字符串仍可能畸形) < JSON mode(保证能 parse 但不保证符合 schema) < 约束解码(采样层卡 grammar 几乎 100% 合法但要能改采样器)。可靠性是**四段流水线**：强制格式→容错解析(畸形塌缩成 `{}` 不 panic)→schema 校验(execute 开头 `args.get().and_then()` else 返回可读 Error 字符串当 isError 喂回)→有界重试。**语法合法≠语义正确**，任何通道后面都要挂确定性 schema 断言。排产 `check_expected` ~16 类确定性断言、`rule_pass_rate≥1.0` 才 exit0 是同一原则的另一处落地。

## Q4 · 【补充】backfill_unanswered_tool_calls 到底解决什么？不做会怎样？为什么不干脆删掉那条消息？
**骨架**：解决 **OpenAI 兼容协议硬约束**——assistant 消息里每个带 id 的 tool_call 下轮必须有配对 role:tool 应答，否则请求直接 **400**(表现为"一取消就神秘 400"，很难联想到是消息序列结构问题)。触发场景：用户 Stop 或预算耗尽，模型吐了 3 个 tool_calls 只跑了 1 个。`cancel_result`/`budget_result` 都先调 `backfill_unanswered_tool_calls`(placeholder)——只对差集补占位消息，按退出原因区分文案。**不删消息的三个原因**：可能含有价值 reasoning、可能只有部分未应答删整条会带走已应答的、删消息破坏 append-only 事实流。resume 侧 `sanitize_restored_messages` 同逻辑。这是**协议层可靠性**最易被忽略的一条。

## Q5 · 【补充】只读工具 join_all 并发，怎么保证真的无副作用可以并发？分类权凭什么不给模型？
**骨架**：安全性建立在 `read_only()` **静态契约**上——工具作者在 trait 上声明(默认 false，写/未知一律当写)，`agent_loop` 只信声明不信模型自述。**分类权不给模型**是因为让模型自称"这是只读"来决定并发/免批，一次幻觉就会并发写踩踏或跳审批。真实踩坑点(主动点出加分)：①MCP 工具 read_only 靠名字前缀启发式(`read_/get_/list_`)，若 server 把会写的命名成 `get_and_reset_counter` 就会误判；②shell 参数是自由命令串没法从声明层保证只读，一律标 not read_only 走串行+审批，真判读写在 Approver 层用白名单+危险正则保守判断(**宁可误判成写多问一次**)。

## ★备弹（被深挖时，8 条最硬的坑）
- **超长工具输出怎么截断喂回模型？**→ 调试轨迹截断(NCX_TRACE 200字)和真正喂回模型的内容治理是两回事。原则：工具产出是要进上下文预算的一等资源，必须有 cap。`read_file` 已强制 offset/limit 分页；shell/grep 这类自由输出应设字节/行上限，超限**头尾保留+中间省略并显式标注**"[truncated N lines]"——截断必须对模型可见，否则模型把"没搜到"和"输出被截断"混为一谈。
- **错误反复回灌会不会让模型对着噪声打转？**→ 会，这是"错误当消息喂回自纠"的反面风险。缺细粒度熔断：应检测**重复失败签名**(命令+错误类型)，连续 N 次同类失败触发"换策略/上报"而非让模型在预算里空转烧钱。工具结果走 role:tool 而非 system，模型协议上不该当指令，但仍需防工具返回的对抗性内容被当指令执行(prompt injection via tool result)——高危动作仍过 Approver，注入绕不过物理隔离。
- **并发读到的是一致快照吗？best-of-N 隔离副本怎么建、promote 会不会冲突？**→ 一致性靠"读写不混批"保证：同一批 `join_all` 全是 read_only 且写强制串行退回，批内不会有写穿插；跨批次状态会变，这是 ReAct 边走边看的设计接受项。best-of-N 各跑独立工作区副本是幂等前提，promote 只让 winner 落地——需注意副本建立机制(worktree/clone)、N 的磁盘/IO 成本上界、promote 前对主区做基线校验防并发覆盖。
- **不可逆外部副作用(转账/发消息/建资源)怎么防重复执行？**→ 工作区隔离只救本地文件类，救不了真正的外部副作用。必须靠**幂等键**：调用带业务去重 token，服务端去重保证 at-most-once；没有服务端幂等支持的只能"写前 verify 是否已执行"或降级到 on-failure/人工。schema 演进：resume 时不校验旧参数是否符合当前 schema，靠 execute 开头的 `args.get()` else 兜底，畸形就当 isError 喂回重来——这是运行时软失败非启动时拒绝，更强设计要给工具带 version 并在 resume 时校验。
- **人在回路的插入点在哪几个粒度？等待人审时状态怎么挂起恢复？**→ 三粒度：工具级(每次危险调用 Ask)、步级(`require_step_approval` 把自动过的 write 升级 Ask，但 AutoDeny 永不软化)、计划级(整体 dry-run+人审)。恢复：待审动作本质是一条待应答的 tool_call，和 backfill 同构——挂起时若不落地会断线丢状态，恢复靠 resume+sanitize 重建序列。edit-and-approve(人改参数再执行)比纯拒绝信息量高。**审批超时默认应走 AutoDeny**(缺省拒绝而非缺省放行)，对齐"安全建在代码、宁可挡错不可放错"。
- **执行清单中途前提变了(文件被改/依赖下线)怎么发现该重规划？**→ 四选一里最难落地的一支，判定信号材料没讲清是真实缺口。当前是 classify 静态分流(非运行中动态判断)。更强设计：每步 execute 前做轻量 precondition check(依赖的文件/状态还在不在)，verify 节点除了判 PASS/FAIL 还应多输出"局部可重试 vs 前提已变需重规划"这个信号，把重规划触发从人工经验变成可执行的显式输出。
- **工具太多，渐进暴露的词法打分会不会漏召回该用的工具？**→ 会，这是渐进暴露的失败模式。核心矛盾：上限 9 裁掉长尾工具降低选择困难，但纯词法打分(名字精确100/含50/描述含20)可能漏掉没被 query 词命中的工具，而反馈回路(`tool_search`)依赖模型"主动想到调它搜"——若模型不知道某工具存在，可能根本不会去搜(冷启动盲区)。兜底：①`ALWAYS_VISIBLE` 核心集恒可见保证探索入口在；②`tool_search` 描述里显式告诉模型"工具没全给你，缺能力就搜"；③命中的 hints 写进下轮带上多轮收敛。升级方向是 embedding 语义检索，但引入索引成本，当前选确定性优先的词法方案。
- **观察本身可能是噪声或误导，怎么区分真信号？**→ ReAct 的软肋是完全信任观察。具体噪声源：①**silent success**——`ok()` 只看 `exit0 && !timed_out && !sandbox_denied`，退出码 0 不等于语义成功(命令返回 0 但文件内容错)；②stderr 有内容不代表失败，别让模型看到 stderr 就以为炸了；③grep 转义错返回空 vs 真没命中，模型无法区分两种"空"；④时间戳/随机数让模型误判状态在变。核心原则：**观察是证据不是真相，exit code 是弱信号，语义层成功必须靠独立 verify(回读+schema 断言)另判**——这正是"语法合法≠语义正确"那条不变量的另一面：命令跑通≠事情做对。

---

### 刷题优先级
- **先刷 Q1/Q2/Q3**(ReAct vs Plan / 异常四选一 / 结构化输出四段流水线)——这三条是面试最爱深挖的执行范式核心。
- ★备弹里 **"silent success / ok() 只判进程层"** 和 **"重复失败熔断"** 最容易被资深面试官钻出真实生产事故味道，优先备。
- 口径统一：这里的"分类权不给模型" = 排产的"确定性路由不给 LLM" = 网关的"mode 由调用方传不由模型定"，都是"决策权留在代码，不留在模型自述"。
