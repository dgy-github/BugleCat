## 项目记忆 · 自进化

### 一句话主线
"项目记忆·自进化" 是两个正交的层：MEMORY（`memory.rs`，机器写、可演化、按 query 每轮注入的经验库）和 INSTRUCTIONS（`project_instructions.rs`，人写、静态、启动整块注入的 CLAUDE.md/AGENTS.md）；"自进化" 只指 MEMORY 一侧的 `remember` 写入 + `consolidate`/`summarize_consolidate` 去重折叠，INSTRUCTIONS 永远是不可变的人类输入。

### 30 秒 / 2 分钟 / 深挖 三档

**30 秒（主线 + 双层 + 一句存储真相）**
MEMORY 是 agent 自己写、可自演化的经验库，落地为单个人类可读 markdown 文件 `.ncx/memory/LEARNINGS.md`（不是 JSONL），每条是 `<!-- ts:<epoch> tags:<a,b> -->` 注释头 + 正文；INSTRUCTIONS 是人写只读的 CLAUDE.md/AGENTS.md，启动一次整块进系统提示。两者区别一句话：机器写 vs 人写、每轮按 query 召回 vs 启动整块注入。

**2 分钟（加上三条数据通路 + 关键数字）**
- 写：模型调 `remember(text,tags,now)` 工具 → trim → `normalize()`（折叠空白 + 小写）精确去重 → 追加 → 按 ts 升序排 → 超过 `MAX_ENTRIES=200` 从最旧端 drain → 全文件重写。
- 召回：每轮 `recall(query, 8, 4000)`，用 `semantic_score`（tag 精确 +8 / 子串 +4 / 相邻词 phrase +6 / Jaccard×20）混合排序，query 先过 `semantic_aliases` 同义词扩展；对单个 query 词，tag 精确命中(+8) 与正文 substr 命中(+4) 是 else-if 互斥、不叠加，phrase(+6) 与 Jaccard(×20) 再单独累加；结果包成 `[memory recall for this prompt]` 临时系统 note，永不写进 session 历史。
- 维护（自演化）：每次 CLI/GUI 启动跑 `consolidate(0.85)`（启发式，只丢近似重复，保最新）；`--memory-merge` 显式触发 `summarize_consolidate(0.85)`（用 fast model 把同簇合并成一条）。

**深挖（机制内核 + 取舍 + 坑）**
- 存储：每次 mutation 调 `write_all` 全文件重写（`# Project memory (nanocodex)` 标题 + 每条 header），`parse_entries` 靠 `strip_prefix("<!-- ")`/`strip_suffix(" -->")` 识别头、split token 解 `ts:`/`tags:`，文件缺失/不可读 → 空 Vec。
- 召回打分细节：keyword 长度 ≥3、phrase 窗口 = 2 相邻词、最终排序键 `overlap*1_000_000 + min(ts,999_999)`；字符截断是 greedy break（一条超了直接停，不跳过）。
- 自演化两档：`consolidate` 只能 DROP（newest-first 贪心：每个条目与 *所有已建立的簇代表* 逐一比 Jaccard，落入第一个满足阈值的簇即 first-match，无匹配则自成新簇代表，保最新、idempotent）；`summarize_consolidate` 同样是 first-match 贪心单链聚类，但能真正 MERGE（取簇内 max ts + tags 并集），summarizer 返回 None/空串 → 退化为保最新。两者比对对象都是全部代表而非单一 FIRST 代表，是 order-dependent、非传递的单链聚类。
- 召回与去重共用同一套 `word_set`/`jaccard` 原语：检索的 "相似" 和去重的 "重复" 是同一个 Jaccard 度量，只是阈值不同（召回无下限，consolidate=0.85）。

### 核心机制 · 6 根支柱

1. **单文件 markdown 存储** — `.ncx/memory/LEARNINGS.md`，每条 `<!-- ts: tags: -->` 头 + 正文，每次 mutation 全文件 `write_all` 重写 — 显式选人类可读 markdown（不是 JSONL）让开发者能手开手改，≤200 条规模下原子正确性比追加吞吐更重要。
2. **写 + 精确去重 + newest-N 截断** — `remember` 用 `normalize()` 阻止同事实大小写/空格变体重存，超 200 条丢最旧 — 保持库可信 + 有界增长 + 近期偏置；`now` 由调用方传入便于测试确定性。
3. **混合词法语义召回（每轮注入）** — `recall` = tag+8/substr+4（同词互斥 else-if）/phrase+6/jaccard×20，query 经 `semantic_aliases` 扩展，按 query 注入临时 note — ≤200 条上嵌入模型是杀鸡用牛刀，零依赖的关键词+Jaccard 混合 "够用"；按 query 召回只把相关 note 入 context 并限 token。
4. **启发式近重折叠 consolidate** — Jaccard≥0.85，newest-first 贪心：每条与所有已建簇代表逐一比、落入第一个达标的簇（first-match），只丢近似重复保最新，idempotent，每次启动自动跑 — 模型会写语义重叠的同一教训变体，折叠保召回信号纯净且库小；幂等且廉价所以可无脑每次启动跑。
5. **LLM 折叠 summarize_consolidate** — 同样的 first-match 贪心聚类（与全部代表逐一比），但把 size>1 的簇用 fast model 合成一条（max ts + tags 并集），None → 退化保最新，仅 `--memory-merge` 触发 — 启发式只能丢，LLM 能真正把多条同主题事实合成更丰富的一条；因有 LLM 成本所以不每次启动跑。
6. **INSTRUCTIONS 层（与 memory 分离）** — `load_project_instructions` 按 `~/.codex/AGENTS.md`→`~/.claude/CLAUDE.md`→repo-root 向下到 workspace 的 AGENTS/CLAUDE/.claude-CLAUDE 顺序拼接、16000 字符封顶，启动一次进系统提示 — 镜像 Codex/Claude-Code 生态的人写持久指引惯例；parent-before-child 让嵌套指令能细化 repo 级。

### 关键数字 / 必背细节
- `MAX_ENTRIES = 200` — 存储硬上限，超出从最旧端丢（`memory.rs:31`）。
- `MEMORY_RECALL_MAX_ENTRIES = 8` / `MEMORY_RECALL_MAX_CHARS = 4_000` — 单 prompt 召回块的条数与字符预算（`agent_loop.rs:20-21`）。
- consolidate / summarize_consolidate 阈值 = **0.85**（Jaccard）；单测里另有 0.8（`memory.rs:549` 等，仅测试用）。
- 打分权重：tag 精确 +8 *或* substr +4（对同一 query 词二者 else-if 互斥）、phrase +6、jaccard×20（四舍五入）（`memory.rs:327-339`）。
- keyword / word_set 最短长度 = **3** 字符；phrase 窗口 = **2** 相邻词（`memory.rs:276,354,362`）。
- recency 打包：`overlap*1_000_000 + min(ts,999_999)`（`memory.rs:109`）。
- project instructions 字符封顶 = **16_000** — 加载 + 封顶在 `runner.rs:100`（`load_project_instructions(workspace, 16_000)`）、`bridge.rs:318`（GUI 的 `load_workspace_instructions(..., 16_000)`）；注入/拼接在 `runner.rs:101`（`compose_system_prompt`）。
- 存储路径 = `<workspace>/.ncx/memory/LEARNINGS.md`（`memory.rs:53`）。
- 必背函数名：`remember` / `recall` / `consolidate` / `summarize_consolidate` / `normalize` / `semantic_score` / `semantic_aliases` / `parse_entries` / `write_all`；`Summarizer` trait 是 `#[async_trait(?Send)]`、单方法 `merge(&[String]) -> Option<String>`。
- 三个 0.85 自动/手动触发点：`consolidate` 在每次 CLI start（`main.rs:167`）、每次 LiveRunner 构造（`runner.rs:39`）、GUI 按钮（`lib.rs:879`）；`summarize_consolidate` 仅 `--memory-merge`（`main.rs:128`）。

### 取舍与坑
- **召回无最低分下限**：overlap=0 也照样按 recency 排序并填满 8/4000 caps，空/离题 query 会把 8 条最新 note 当相关推出（`memory.rs:104-129`）。
- **字符 cap 是 greedy break 不是 skip**：某条超 `max_chars` 就 `break`，一条长的早 note 会切掉本可放下的更短相关 note（`memory.rs:123-125`）。
- **recency tie-break 对真实数据基本失效**：`min(ts,999_999)` 对真实 epoch（~1.7e9）饱和到 999_999，所有真实时间戳低位相同，tie-break 实际只在 ts<999_999（即测试）下生效 — 待跟进。
- **`semantic_aliases` 单向硬编码**：是一张固定小表（如 search→web/tavily），只对这几个领域帮到语义召回（`memory.rs:295-308`）。
- **聚类是 order-dependent 的 first-match 贪心单链**：每条与 *所有* 已建簇代表逐一比 Jaccard、落入第一个达标的簇（不是只比单一 FIRST 代表），newest-first、非传递完整，渐变漂移的 note 链可能不全折叠（`memory.rs:148-160`、`193-209`）。
- **`parse_entries` 把缺失/畸形 `ts:` 静默归 0**：该条会排成最旧、最先被 cap 丢掉（`memory.rs:418`）。
- **`write_all` 全文件重写且非原子**（`std::fs::write`，无文件锁）：写到一半崩溃可能截断 `LEARNINGS.md`，单 checkout 并发 session 会 race（即已知 shared-worktree 隐患）。
- **CLI 会吃用户全局 `~/.claude/CLAUDE.md`，GUI 不会**：GUI 用 `load_workspace_instructions`（home=None）故意排除开发者个人配置，CLI 不排除，所以 CLI 跑会带上用户的 HANDOFF/GLM-debug 规则（`project_instructions.rs:24-33`）。

### 高频追问与应答

**Q1：自进化具体指什么？是 LLM 自动从对话提炼吗？**
不是自动提炼。写触发是模型驱动 —— agent 必须主动调 `remember` 工具，没有任何东西自动从一轮对话抽 learning；唯一 "VERIFIED, not guesses" 的约束就在工具描述里。"自演化" = `remember` 写入 + `consolidate`/`summarize_consolidate` 去重折叠，且只发生在 MEMORY 一侧，INSTRUCTIONS 不演化。

**Q2：为什么不用 embedding / 向量库做召回？**
≤200 条规模上嵌入模型是杀鸡用牛刀。用 tag+substr+phrase+Jaccard 的廉价混合加一张手curated 同义词表（`semantic_aliases`）就拿到 "够用" 的语义检索，零依赖。注意打分里同一 query 词的 tag(+8) 与 substr(+4) 是 else-if 互斥、不叠加，phrase(+6) 与 jaccard(×20) 才另算。代价是 `semantic_aliases` 覆盖窄、且召回无最低分下限。

**Q3：consolidate 和 summarize_consolidate 区别？为什么要两个？**
聚类逻辑相同（Jaccard≥0.85 的 first-match 贪心单链：每条与所有已建簇代表逐一比、落入第一个达标的簇），但 `consolidate` 只能 DROP 近似重复保最新，启发式、幂等、廉价，所以每次启动无脑自动跑；`summarize_consolidate` 用 fast model 把同簇真正 MERGE 成一条（取 max ts + tags 并集），更丰富但有 LLM 成本，所以只在 `--memory-merge` 显式触发，且 None/空串 → 退化为保最新，保证模型不可用时安全降级。

**Q4：召回为什么每轮算、且不写进 session？**
按 query 召回（用 `tool_query` 不是原始 user_input）只把相关 note 入 context、限住 8/4000 token；包成 `[memory recall for this prompt]` 临时 note 注入 per-iteration `notes`，never persisted —— 否则会污染对话历史并跨轮累积。它每轮 model call 都重现，但每 turn 只算一次。

**Q5：memory 和 project instructions 怎么区分？容易混吗？**
很容易混但接线完全分开：instructions 是人写、静态、启动一次加载并 16_000 封顶（`runner.rs:100`）后整块拼进系统提示（`compose_system_prompt`，`runner.rs:101`）；memory recall 是机器写、每轮临时、永不入 session（`agent_loop.rs:701-703`）。一个 whole-block startup，一个 query-scoped per-turn。

**Q6：存储为什么用单 markdown 文件而不是 JSONL/DB？**
显式选人类可读 markdown，让开发者能直接打开手改 store；注释头保持元数据机器可解析、正文是纯 prose。≤200 条小库下全文件重写简单且原子正确性优先于追加吞吐 —— 代价是非原子写、无锁、并发会 race。

### 自测 · 主动回忆

1. [L1] `LEARNINGS.md` 单条的格式是什么？文件每次怎么写？
2. [L2] `remember` 的精确去重靠什么？超过上限丢哪端？
3. [L2] `recall` 的四类打分权重各是多少？哪两类对同一 query 词是互斥的？最终排序键怎么打包 recency？
4. [L3] recency tie-break 在真实 epoch 数据下为什么基本失效？
5. [L3] 召回的字符 cap 有什么坑？无最低分下限会导致什么？
6. [L4] consolidate 与 summarize_consolidate 的聚类比对对象是什么？两者在 "能做什么" 和 "何时触发" 上各自的设计取舍？
7. [L4] 为什么召回每轮算且不写进 session，而 instructions 启动一次整块注入？这反映了什么设计意图？
8. [L2] CLI 和 GUI 在加载 project instructions 上有什么故意的差异？为什么？

**答案要点**
1. `<!-- ts:<epoch> tags:<a,b> -->\n<正文>\n\n`，前面带 `# Project memory (nanocodex)` 标题；每次 mutation 由 `write_all` 全文件重写（非原子 `std::fs::write`）。
2. 靠 `normalize()`（折叠空白 + 小写）对比已有条目，任一相等则 Ok(false) 不写；先按 ts 升序排，超 `MAX_ENTRIES=200` 从最旧（FRONT）drain。
3. tag 精确 +8、子串命中 +4、相邻词 phrase +6、`jaccard×20`（四舍五入）；其中对同一 query 词 tag(+8) 与 substr(+4) 是 else-if 互斥、不叠加，phrase 与 jaccard 再单独累加；排序键 `overlap*1_000_000 + min(ts,999_999)` 降序。
4. `min(ts,999_999)` 对 ~1.7e9 的真实 epoch 饱和到 999_999，所有真实条目低位相同，tie-break 退化为 no-op，只在 ts<999_999（测试）下生效。
5. cap 是 greedy `break` 而非 skip，一条长的早 note 会切掉后面本可放下的更短相关 note；无最低分下限 → overlap=0 也按 recency 填满 8/4000，空/离题 query 推出 8 条最新 note。
6. 两者都是 first-match 贪心单链聚类：每条与 *所有* 已建簇代表逐一比 Jaccard、落入第一个达标的簇（非只比 FIRST 代表，order-dependent）。`consolidate` 只能 DROP 近似重复（保最新），启发式/幂等/廉价 → 每次启动自动跑；`summarize_consolidate` 用 fast model 真正 MERGE 成一条（max ts + tags 并集），有 LLM 成本 → 仅 `--memory-merge` 触发，None/空串退化为保最新。
7. recall 按 query 限相关 + 限 token，临时注入不持久化避免污染历史与跨轮累积；instructions 是稳定的人写指引，整块进系统提示一次即可。意图：机器经验做动态、按需、可丢；人类指引做静态、全局、不可变。
8. GUI 用 `load_workspace_instructions`（home=None）故意排除 `~/.codex`/`~/.claude` 全局文件，让 end-user 的 chat 跟随打开的项目而非开发者个人配置；CLI 用 `load_project_instructions`（含 home），不排除，会吃用户全局 CLAUDE.md。

### 别发散到这
- session 历史 / 对话持久化机制 —— 那是 Session/AgentLoop 范畴，这里只需说 "recall 不写进 session"。
- `tool_query` 怎么选 tool schema —— 属工具路由子系统，这里只借它当召回 query。
- fast/pro 分层 orchestrator、模型路由 —— 另一个子系统；summarize_consolidate 只是 "用 fast model" 一句带过。
- skill / prompt-hook 注入细节 —— recall 只是和 budget note、prompt-hook 输出并列被 append，不展开。
- 嵌入模型 / 向量检索实现 —— 本系统明确不用，点到 "杀鸡用牛刀、零依赖" 即止。

### 一句话收尾
记住这条主线就不会浅也不会散：MEMORY 机器写、可演化、按 query 每轮临时召回；INSTRUCTIONS 人写、静态、启动整块注入；"自进化" = `remember` + `consolidate`/`summarize_consolidate`（均为 first-match 贪心、与全部簇代表逐一比对），全部围绕同一套 Jaccard 原语，阈值 0.85、上限 200、召回 8/4000。
