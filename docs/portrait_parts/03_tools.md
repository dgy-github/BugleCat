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
