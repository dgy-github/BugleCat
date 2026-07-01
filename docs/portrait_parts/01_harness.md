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
