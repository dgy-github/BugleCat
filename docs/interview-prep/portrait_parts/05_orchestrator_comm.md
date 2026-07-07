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
