## 上下文压缩 · context editing

### 一句话主线 (the single thesis the candidate opens with — prevents both shallowness and sprawl)

> "context editing 是 `Session::edited_body` 在**发送时**算出来的一个**非破坏性历史视图**:用 `.chars().count()`(不是 token)做预算,两步走 —— 先把 keep_recent 窗口之外的旧 `tool` 结果截断到 `max_tool_result_chars`,再在超预算时按干净边界丢弃最老的前缀;`self.messages` 永不改动,只有 `/compact` 走 `compact` 把它落盘。"

一句话锁死四个考点:**send-time、非破坏、字符代理、两步算法**。后面所有追问都挂在这根主线上。

---

### 30 秒 / 2 分钟 / 深挖 三档 (a depth ladder — each level adds specific named mechanisms & numbers, so the candidate scales to the interviewer's probing instead of stopping short)

**30 秒档(机制骨架):**
context editing 由 `ContextEditPolicy{enabled, max_chars=120k, keep_recent_messages=30, max_tool_result_chars=4k}` 驱动。`edited_body` 两步:Pass 1 压缩旧 tool 结果,Pass 2 超预算时丢最老前缀。`for_model_edited` 把它做成临时视图发给 provider,`self.messages` 不动。`/compact` 是它的破坏性孪生,会落盘。

**2 分钟档(加机制名 + 数字):**
- 预算用 **字符代理**:`json_chars` = `serde_json::to_string(msg)` 后的 `.chars().count()`,**没有 tokenizer**。`estimate_tokens`(2 chars/token)是另一套、只给 UI 看的、**不参与门控**。
- **Pass 1**:`recent_cutoff = len - keep_recent_messages`(saturating),对 `i < recent_cutoff` 且 `role=="tool"` 的消息调 `compress_tool_result`:内容是字符串且长于 `max_tool_result_chars`(4k)时,`.chars().take(max_tool_result_chars)` 留头,追加 `[context edited: omitted the rest of prior {name} result; original_chars=N]`。注意:`compress_tool_result` 内部那个本地参数名叫 `max_chars`,但调用处传的是 `policy.max_tool_result_chars`(4k),**不是** 120k 预算 —— 阈值和留头长度是**同一个旋钮**(4k)。
- **Pass 2**:仅当 `total_chars > max_chars` **且** `body.len() > keep_recent` 才触发。`start = len - keep_recent` → 前进到窗口内第一个 `user` → 跳过开头的 `tool` → 若 `0<start<len` 丢前缀并记 `dropped_messages=start`。
- `compact` 复用同一算法,force `enabled=true`,**仅当** `compressed_tool_results>0 || dropped_messages>0` 才覆盖 `self.messages` 并 `rewrite_log()` 重写 JSONL。
- `call_model` **每次迭代**都重算视图(`agent_loop.rs:238-258`),因为一个 turn 内追加 tool 结果会让历史增长。

**深挖档(配置 + 完整性 + 非显然点):**
- 四个旋钮在 `config.rs:98-101/138-141`,通过 `positive_usize(value, fallback)` 映射:`<=0` 或溢出的 i64 **静默回退**到默认 —— 所以 `keep_recent=0`/`max_chars=0` **设不出来**,只有 `enabled=false` 真能关。
- **完整性三件套**:`sanitize_restored_messages`(为孤儿 tool_call 补合成 tool 回复)、`backfill_unanswered_tool_calls`(运行时 cancel/budget 路径)、`redact_image_data`(`data:` 图片转占位)。Pass 2 跳过开头 tool 与这三者守同一个不变量:**每个 tool_call 都有配对 tool 回复 / 没有孤儿 tool**。
- `resume()`/`fork()` 在 sanitize 之前先 **filter 掉 `role=="system"`**,系统提示故意丢弃、用新建的替换。
- 只有 **一套** editing 算法:`enabled` 标志 + caller 是否把结果赋回去,决定了恒等视图 / 临时编辑 / 破坏性落盘三种语义。

---

### 核心机制 · 7 根支柱 (a BOUNDED list — each pillar: 名字 — 机制一句 — 为什么一句.)

1. **ContextEditPolicy + Stats** — 值类型策略 `{enabled, max_chars, keep_recent_messages, max_tool_result_chars}` + `ContextEditStats{original_chars, edited_chars, compressed_tool_results, dropped_messages}`(`session.rs:16-41`)。 — 一个值类型策略让同一算法靠翻 `enabled` 同时服务临时视图和破坏性落盘;默认值与 Python Config / config.rs 完全对齐。

2. **字符代理预算** — `json_chars` = 序列化 JSON 的 `.chars().count()`,`total_chars` 累加 system + notes + 每条 message(`session.rs:443-453`)。 — 不依赖 tokenizer、不联网,每个 turn 都能廉价、确定、离线地算出编辑决策;数 JSON 框架/键大致对应 token 会计入的部分。

3. **Pass 1 — 压缩窗口外旧 tool 结果** — `compress_tool_result` 对 `recent_cutoff` 之前、`role=="tool"` 且字符串长于 `max_tool_result_chars`(4k)的内容用 `.chars().take(max_tool_result_chars)` 留头 + `original_chars=N` 标记(`session.rs:217-225, 493-513`)。阈值和留头长度是**同一个** `max_tool_result_chars`(在函数内被混淆地命名为本地参数 `max_chars`,但绑定的是 4k 值,不是 120k 预算)。 — 旧 tool 转储是上下文膨胀主因,但近期的仍在被推理;`.chars().take` 而非字节切片是为了不在 UTF-8 多字节中间 panic;标记告诉模型被截断了多大,可重新取。

4. **Pass 2 — 干净边界丢最老前缀** — 超预算且 `body.len()>keep_recent` 时,从 `len-keep_recent` 前进到第一个 `user`、跳过开头 `tool`,丢前缀记 `dropped_messages`(`session.rs:227-241`)。 — 必须落在自洽边界:从 `user` 起每个保留 turn 自包含,跳开头 `tool` 防止孤儿 tool 回复(其 assistant tool_call 刚被丢)让历史不符合 OpenAI schema。

5. **for_model_edited — 非破坏视图** — 算出 `(body, stats)` 后拼 `[system] + 每条非空 note 各自一条 system 消息 + body`,重算 `edited_chars`;`self.messages` 不动(`session.rs:167-187`)。 — 完整审计日志必须留在内存与磁盘上,编辑只作用于本轮发出去的内容;notes(预算/hook/记忆)作为临时 system 消息注入,也不持久化。

6. **compact — 破坏性落盘孪生** — clone policy、force `enabled=true`、跑 `edited_body`,**仅当真的变了**才覆盖 `self.messages` 并 `rewrite_log()` truncate+重写 JSONL(`session.rs:189-201, 321-348`)。 — `/compact` 必须持久化,让后续 turn 和 `--resume` 看到更小历史;no-op 守卫避免在未超预算时白白重写日志。

7. **call_model 每轮接线** — `call_model` 每次迭代调 `for_model_edited(system_notes, &context_edit)` 流式发送并返回 stats,喂给 NCX_TRACE(`agent_loop.rs:238-258, 367-392`)。 — 一个 turn 内追加 tool 结果会让历史在单轮内跨过预算,所以编辑必须每次模型调用都重算,而非只跨 turn。

---

### 关键数字 / 必背细节 (the exact constants, thresholds, function names to nail)

- **默认值**:`enabled=true`、`max_chars=120_000`、`keep_recent_messages=30`、`max_tool_result_chars=4_000`(`session.rs:24-33`,与 `config.rs:138-141` 一致)。
- **Pass 2 触发条件(必背)**:`total_chars(system,notes,body) > max_chars` **AND** `body.len() > keep_recent_messages`(`session.rs:227-229`)。
- **Pass 1 cutoff**:`recent_cutoff = body.len().saturating_sub(keep_recent_messages)`,只动 `i < recent_cutoff`。
- **Pass 1 阈值 + 留头长度(同一旋钮)**:都用 `max_tool_result_chars=4_000`;`compress_tool_result` 仅当 `content.chars().count() > 4_000` 才压(`<= 4_000` 不压),压时 `.chars().take(max_tool_result_chars)` 留头(`session.rs:500-503`)。函数内本地参数名叫 `max_chars` 是误导,绑定的是 4k,**不是** 120k 预算。
- **截断标记原文**:`[context edited: omitted the rest of prior {name} result; original_chars={N}]`,`name` 默认 `"tool"`,N 是真实 `.chars().count()`。
- `estimate_tokens`:`CHARS_PER_TOKEN=2` + 每条 message `+8` 框架开销(`session.rs:461-491`)—— **UI 专用,不门控编辑**。
- 干扰项(别和编辑预算混):`context_token_budget=512_000`、`context_window=1_048_576`(`config.rs:136-137`)都是 UI,不被编辑器使用。
- 记忆 note 上限:`MEMORY_RECALL_MAX_ENTRIES=8`、`MEMORY_RECALL_MAX_CHARS=4_000`(`agent_loop.rs:20-21`)。
- `positive_usize(value, fallback)` = `usize::try_from(value).ok().filter(|v|*v>0).unwrap_or(fallback)`(`main.rs:1205-1210`):`<=0`/溢出 → fallback。
- 关键函数名:`edited_body`、`for_model_edited`、`for_model`、`compact`、`compress_tool_result`、`json_chars`/`total_chars`、`rewrite_log`、`sanitize_restored_messages`、`backfill_unanswered_tool_calls`、`redact_image_data`、`context_edit_from_config`。

---

### 取舍与坑 (design trade-offs + soft spots — the material for follow-ups.)

- **字符 ≠ token ≠ 字节**:用 `chars().count()` over 序列化 JSON 做预算。CJK-heavy / code-heavy 文本的真实 token 数会偏离这个 120k 字符门 —— 真实 prompt 大小会漂移。`estimate_tokens` 是**另一套**计算,不是门控路径。
- **两套字符门、两个旋钮别混**:Pass 1 的阈值与留头长度都是 `max_tool_result_chars`(4k,per-result 上限);Pass 2 的超预算判定才用 `max_chars`(120k,整段预算)。函数内 `max_chars` 这个本地参数名是误导 —— Pass 1 实际留的是 4k 头,不是 120k。
- **`keep_recent=0` / `max_chars=0` 设不出来**:`positive_usize` 把它们当"未设"静默回退成 30 / 120_000。只有 `context_edit_enabled=false` 真正关闭。这是有意的(0 会压/丢活跃 turn),但对想极限调小的用户是反直觉的。
- **Pass 1 与 Pass 2 独立**:压缩可能已把总量压到 max_chars 以下,丢前缀就不触发。但压缩看 **content 字符串**的 `chars()`(`<= 4k 不压`),超预算看整条消息的 **`json_chars`** —— 一个刚好 `<= 4k` 的 tool 结果不被压,却仍带着 JSON 框架计入预算。
- **`compress_tool_result` 只对字符串 content 生效**(`session.rs:497`):tool 结果若存成 JSON 数组/对象 content,会被**整个跳过**,不压缩。
- **Pass 2 找 user 不向窗口前搜**:用 `body[start..].position(role==user)`;若近期窗口内**没有** user 消息,`start` 停在 `len-keep_recent` 再跳开头 tool —— 即丢到 keep_recent 边界(减去开头 tool)。
- **`redact_image_data` 只脱敏 `type=="image_url"` 且 url 以 `data:` 开头**(`session.rs:423-428`):http(s) 图片 URL 原样落盘,非数组 content 原样返回。
- **`/compact` 会丢失原始时间戳**:`rewrite_log` 给每条幸存消息重打一个新 `_ts`(`session.rs:343`),compact 后所有消息都是 compaction 时刻的戳。
- **compact 与运行时视图的 char 总量会有微差**:`compact` 用 `edited_body(&[], policy)` **不带 notes**,而运行时 `for_model_edited` 带 budget/hook/memory notes —— 触发前缀丢弃的总量略有不同。

---

### 高频追问与应答 (4-6 likely follow-ups, each with a crisp model answer.)

**Q1:为什么不用 tokenizer,用字符数?**
A:编辑决策要廉价、确定、离线、每轮都能跑,引 tokenizer 要么加依赖要么联网。`chars().count()` over 序列化 JSON 是稳定的跨平台代理,还顺带数进了 JSON 框架/键(token 会计也大致计入)。代价是 CJK/code 文本真实 token 会漂移 —— 这是已知 trade-off,不是 bug。

**Q2:为什么 Pass 2 要前进到 user、再跳开头 tool,不直接砍 `len-keep_recent`?**
A:为了让截断后的历史仍符合 OpenAI tool-call 配对 schema。从 `user` 起保证每个保留 turn 自包含;开头若留着 tool 回复、而它配对的 assistant tool_call 刚被丢进前缀,就成了孤儿 tool,schema 会拒。这跟 `sanitize_restored_messages`/`backfill_unanswered_tool_calls` 从反方向守的是同一个不变量。

**Q3:`/compact` 和自动 editing 什么关系?为什么能随便点?**
A:同一个 `edited_body`,`/compact` 走 `compact` force `enabled=true` 并把结果赋回 `self.messages` + `rewrite_log()`。它有 no-op 守卫:**仅当 `compressed_tool_results>0 || dropped_messages>0`** 才重写 JSONL,所以未超预算时反复 `/compact` 是纯空操作,不动磁盘、不动时间戳(`compact_noops_when_under_budget` 测了)。

**Q4:一个 turn 内会发生编辑吗?**
A:会。`call_model` **每次模型迭代**都重算 `for_model_edited`,因为 turn 内不断追加 tool 结果,单轮就可能跨过 120k 预算 —— 同一 user turn 的连续两次模型调用看到的历史可能不同。

**Q5:`self.messages` 到底动不动?哪条路径会落盘?**
A:自动 editing(`for_model_edited`)永不动 `self.messages`,只产出临时发送视图,磁盘日志保持完整审计。**只有** `/compact` 走 `compact` 会覆盖 `self.messages` 并 truncate+重写 JSONL,让 `--resume` 看到压实后的历史。

**Q6:配置 `max_chars=0` 想彻底关编辑,行不行?**
A:不行。`positive_usize` 把 `0`/负/溢出当"未设",静默回退到 120_000。想真正关只有一条路:CLI `--disable-context-edit`(把 enabled 置 `Some(false)`)或配置 `context_edit_enabled=false`。三个数值旋钮是 `Option<i64>` 覆盖,enabled 被特判。

---

### 自测 · 主动回忆 (5-8 self-test questions tagged [L1]-[L4])

1. **[L1]** `ContextEditPolicy` 的四个字段和默认值分别是什么?
2. **[L1]** Pass 1 压缩一个旧 tool 结果时,留下什么、追加什么标记?它用的是哪个旋钮 —— max_chars 还是 max_tool_result_chars?
3. **[L2]** Pass 2 触发需要哪两个条件同时成立?为什么要前进到 `user`、跳开头 `tool`?
4. **[L2]** `for_model_edited` 和 `compact` 都用 `edited_body`,语义为何相反?靠什么区分?
5. **[L3]** 配置里把 `keep_recent_messages` 设成 0 会发生什么?为什么这么设计?
6. **[L3]** 一个 tool 结果刚好 3.9k 字符,Pass 1 不压它,但它仍可能把会话推过 max_chars,为什么?压缩门和超预算门各用哪个字符量?
7. **[L4]** 编辑用字符代理而非 token,在什么输入下真实 prompt 大小会偏离 120k 门?这是 bug 吗?
8. **[L4]** 截断后的历史如何保证仍能合法发给 provider?有哪三个机制守同一不变量?

**答案要点(看完再对):**
1. `enabled=true`、`max_chars=120_000`、`keep_recent_messages=30`、`max_tool_result_chars=4_000`(`session.rs:24-33`)。
2. 留 `.chars().take(max_tool_result_chars)` 的头(即前 4k 字符),追加 `[context edited: omitted the rest of prior {name} result; original_chars=N]`,N 是真实字符数,name 默认 `"tool"`。用的是 **`max_tool_result_chars`(4k)**,不是 120k 的 `max_chars` —— 函数内本地参数虽叫 `max_chars`,但调用处传的是 `policy.max_tool_result_chars`,绑定 4k。
3. `total_chars > max_chars` **且** `body.len() > keep_recent_messages`。前进到 `user` 让每个保留 turn 自包含;跳开头 `tool` 防止孤儿 tool 回复破坏 OpenAI tool-call 配对 schema。
4. 区分靠 `enabled` 标志 + caller 是否把 body 赋回 `self.messages`。`for_model_edited` 产临时视图、不赋回(非破坏);`compact` force enabled + 赋回 + `rewrite_log()`(破坏性落盘)。同一套算法。
5. 设不出来。`positive_usize` 把 0 当"未设",回退到 30。因为 `keep_recent=0` 会压缩/丢弃正在进行的活跃 turn,是退化行为,故意禁止。
6. 因为 Pass 1 压缩看的是 **content 字符串**长度(`<= 4k 不压`,用 `max_tool_result_chars`),而超预算判定用整条消息的 **`json_chars`**(含 JSON 框架/键,与 `max_chars=120k` 比),多条这样的消息累加 `total_chars` 仍可超 `max_chars`。
7. CJK-heavy 或 code-heavy 文本:它们的真实 token 数与字符数比例偏离,而门控只数 `chars()`,所以真实 prompt 大小会漂移。不是 bug,是已知 trade-off(换取无 tokenizer、可离线、确定性)。
8. Pass 2 丢前缀时跳开头孤儿 `tool`;`sanitize_restored_messages` 在 resume 时为缺回复的 tool_call 补合成 tool 消息;`backfill_unanswered_tool_calls` 在运行时 cancel/budget 路径补。三者守"每个 tool_call 有配对 tool 回复 / 无孤儿 tool"。

---

### 别发散到这 (a short DO-NOT list)

- **tokenizer / 真实计费 token 数** —— 编辑器**不用** tokenizer;`estimate_tokens`(2 chars/token)是 UI 数字,属另一条路径,点到为止。
- **`context_token_budget=512_000` / `context_window=1_048_576`** —— UI 预算与窗口展示,**不被编辑器使用**,别拿来当编辑门控讲。
- **memory recall / prompt hook 的内容生成** —— 它们只是作为 `system_notes` 注入,属 agent_loop 记忆子系统,这里只关心"作为 note 计入 char 预算且不持久化"。
- **resume / fork 的完整流程** —— `read_log`、filter `system`、`redact_image_data` 属会话恢复子系统;这里只引用它们**与 Pass 2 守同一 tool-call 配对不变量**这一点。
- **provider 流式细节 / NCX_TRACE 输出格式** —— 属 provider 与可观测性子系统;这里只说 stats 被喂给它。

### 一句话收尾.

记住主线:**send-time 算出的非破坏视图,字符代理预算,两步(压旧 tool 到 max_tool_result_chars / 超 max_chars 丢老前缀),`self.messages` 不动 —— 只有 `/compact` 落盘** —— 沿这根脊柱按 30 秒 / 2 分钟 / 深挖三档伸缩,既不会答浅,也不会跑偏。
