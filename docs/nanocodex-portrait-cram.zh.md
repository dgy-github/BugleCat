# nanocodex 速记页（考前快速过）

> 配套 `nanocodex-portrait.zh.md` 使用。考前 3-4 分钟扫一遍：先全局连接图，再 8 子系统的「主线 + 支柱名 + 必背数字 + 最易被追问」。

## 全局连接图（30 秒）
- **一回合端到端**：`main.rs::run` 启动装配（load_config → SandboxPolicy 纯决策 → MemoryStore+consolidate → discover_skills → Genome::from_env → compose_system_prompt → ToolContext 能力袋 → ToolRegistry → AgentLoop）→ `AgentLoop::run_turn`：`for 0..max_model_calls` 交替 `call_model`（先 `for_model_edited` 压缩视图，再 `active_provider().chat`）→ `run_tools`（`ToolRegistry::execute`，连续 read_only 走 `join_all` 并发、写/未知串行）→ 无 tool_call 即 `completed`。
- **训练/运行接缝**：唯一一道是 `NCX_GENOME`——离线 ncx-forge 写 `winner genome.toml`，运行时 `Genome::from_env()` 读它，只覆盖 `system_prompt`+工具 `description`（**纯文本不改行为**）；unset/empty/malformed → `Genome::default()` 可证明 no-op。
- **5 是 1 的外层调度器**：`--orchestrate` 时 `Orchestrator::handle` 把每个 worker/子任务节点变成一次全新 `AgentLoop` run（plan/verify 用空工具集）。
- **三条贯穿不变式**：① `?Send` / current-thread runtime + `Rc<RefCell>`（全 trait `#[async_trait(?Send)]`）；② OpenAI 历史合法性（每个 tool_call 必有配对 reply，由 Harness backfill + 压缩前缀丢弃 + resume sanitize 三处共守）；③「模型是第一杠杆、harness 是第二杠杆」（5 靠结构、8 只进化文本、4 托底执行）。

## 8 子系统速记

### 1. Harness 工程管理
- **主线**：一个有预算上限、可取消、永远让对话保持 API 合法的单一回合循环（`call_model→run_tools` 交替，无 tool_call 即结束、双预算触顶强制终止）。
- **支柱**：TaskBudget 预算有界循环、两层协作式取消、正确性闸门下并发 parallel_run、动态 tool-schema 选择 schemas_for_query、非破坏 context 视图 for_model_edited、API 不变量稳健性 backfill、Hooks 守边界 + vision 路由。
- **必背数字**：`max_model_calls=60`、`max_tool_calls=120`、取消 tick `100ms`（biased select!）、`DEFAULT_VISIBLE_TOOL_LIMIT=9`。
- **最易被追问**：`with_task_budget(0)` 跑一轮（floor `.max(1)`），`with_max_iterations(0)` 跑零轮（不 floor，`0.min(0.max(1))=0`）。

### 2. 上下文压缩 · context editing
- **主线**：`edited_body` 在发送时算出的非破坏性历史视图，用字符代理预算两步走（先压旧 tool 结果到 `max_tool_result_chars`，再超 `max_chars` 丢最老前缀），`self.messages` 不动，只有 `/compact` 落盘。
- **支柱**：ContextEditPolicy+Stats、字符代理预算 json_chars、Pass 1 压窗外旧 tool、Pass 2 干净边界丢前缀、for_model_edited 非破坏视图、compact 破坏性孪生、call_model 每轮重算。
- **必背数字**：`max_chars=120_000`、`keep_recent_messages=30`、`max_tool_result_chars=4_000`、`estimate_tokens` 2 chars/token+8（仅 UI）、记忆 note `8`/`4_000`、干扰项 `context_token_budget=512_000`/`context_window=1_048_576`（UI）。
- **最易被追问**：Pass 1 函数内本地参数名叫 `max_chars` 但绑定的是 4k（`max_tool_result_chars`），不是 120k 预算——同一旋钮管阈值与留头长度。

### 3. 工具系统 · 动态暴露
- **主线**：注册工具 >9 时每轮只把「核心集 ∪ 上次 tool_search 写入的 hints ∪ 本轮 user prompt 词法匹配」裁剪到 9 个 schema 喂模型，确定性词法打分（100/50/20）控 prompt 体积，`read_only` 驱并发、`genome` 注册期统一改描述。
- **支柱**：Tool trait（?Send + to_schema，execute 返 String）、schemas_limited_for_query 三路并集裁剪、tool_search 写 ctx.tool_hints、catalog_score 词法打分、read_only→并发批、genome 描述覆盖。
- **必背数字**：`DEFAULT_VISIBLE_TOOL_LIMIT=9`、`ALWAYS_VISIBLE_TOOLS=6`（read_file/apply_patch/update_plan/shell/tool_search/skill，实际 seed 5–6 个）、base 工具 9（+memory=10、+skills=11）、catalog_score `+100/+50/+20`、tool_words len≥2、tool_search max_results 默认 `8` clamp `1..20`。
- **最易被追问**：默认裸跑 9 个正好 `<=9` 不裁剪，但挂 memory/skill/MCP 任一即到 10、立即触发裁剪；`read_only` 管并发不 gate 沙箱写。

### 4. 沙箱 · 审批状态机
- **主线**：不是状态图而是两层正交纯函数 + 一层进程容器——`SandboxPolicy` 判「物理是否允许」、`Approver::classify` 判「越界怎么办」、`PolicyExecutor` 只做进程容器不碰审批。
- **支柱**：三档 sandbox 模式、四档 approval policy→三态 Decision、untrusted 三重过滤、ApprovalHandler trait+SessionGrants、shell 判官顺序、PolicyExecutor 容器。
- **必背数字**：approval policy `4`（默认 on-request）、Decision `3`、sandbox 模式 `3`、TRUSTED_COMMANDS `29`、GIT_WRITE_SUBCMDS `15`、dangerous_patterns `7`、WRITE_TOOLS `2`、`timeout_s=120`（shell 限 1..600）、`active_process_limit=512`、timeout→`exit 124`、`MAX_OUTPUT=16000`（head/tail 各 8000）。
- **最易被追问**：`step_decision` 是死代码（有定义+单测但活流程未调用）；记住决定靠 `ApprovalDecision::Always`+`require_edit_approval`。

### 5. 主子 agent 如何通讯
- **主线**：无消息总线无 peer 对话——三层闭环「单向广播（prompt 按值序列化进全新无状态子会话）+ 文件落地（worker 改隔离拷贝、`promote_worker` copy winner 回真 workspace）+ 裁决回灌（`PASS`/`FAIL`/`BEST:<n>` 驱动重试与落地）」。
- **支柱**：Prompt 线程化文本工件（下行）、文件系统 IPC（隔离-提升）、裁决回路、tool-stripped reason() 节点（代码级 ToolRegistry::empty）。
- **必背数字**：默认 `max_depth=1`、`<2` 子任务回退 Main 上 best-of-N、`verdict_passed = !contains("FAIL")`、`BEST:<n>` 缺失/畸形→index 0、`parse_complexity` 无法识别→Medium。
- **最易被追问**：best-of-N 可 `join_all` 并行（每 worker 私有 scratch），递归子任务必须 `for` 串行（先提升再进下一个，因子任务可能依赖）——同一原则「真 workspace 任一时刻恰好一个写者」。

### 6. 项目记忆 · 自进化
- **主线**：两个正交层——MEMORY（机器写、可演化、按 query 每轮临时注入）vs INSTRUCTIONS（人写、静态、启动整块注入）；「自进化」只指 MEMORY 侧 `remember` 写入 + `consolidate`/`summarize_consolidate` 去重折叠。
- **支柱**：单文件 markdown 存储、写+精确去重+newest-N 截断、混合词法语义召回、启发式近重折叠 consolidate、LLM 折叠 summarize_consolidate、INSTRUCTIONS 层。
- **必背数字**：`MAX_ENTRIES=200`、召回 `8`/`4_000` chars、Jaccard 阈值 `0.85`、打分 tag `+8`/substr `+4`（else-if 互斥）/phrase `+6`/jaccard `×20`、keyword 最短 `3`、phrase 窗口 `2`、排序键 `overlap*1_000_000+min(ts,999_999)`、instructions 封顶 `16_000`。
- **最易被追问**：recency tie-break 对真实 epoch（~1.7e9）饱和到 999_999 基本失效；召回无最低分下限——空/离题 query 也按 recency 填满 8 条最新 note。

### 7. Skills · MCP · 视觉
- **主线**：三者都是可选/外部能力插进同一套 Tool+turn 机器，共享「索引常驻便宜、重内容按需拉取」——Skills 渐进披露、MCP 把外部 JSON-RPC server 工具包成本地 `Tool` 走同一 Approver 路、视觉 per-turn 路由，主循环对 provider 与工具来源无感。
- **支柱**：Skills L1 索引块、Skills L2/L3 skill 工具、Skill::load_body 双源、McpClient stdio JSON-RPC、McpTool+共享 client、Per-turn 视觉路由。
- **必背数字**：MCP `PROTOCOL="2024-11-05"`、`REQ_TIMEOUT=30s`、`clientInfo={nanocodex,0.1}`、read-only 名字 `6` 前缀+`5` 精确词、builtin skill `1`（commit-message）、skill 发现根 `2`（~/.ncx/skills < <ws>/.ncx/skills）、`DEFAULT_VISIBLE_TOOL_LIMIT=9`。
- **最易被追问**：一个 server 全部工具共享一个 `Rc<Mutex<McpClient>>`（Mutex 串行化单管道、Rc 因单线程 !Send）；视觉 flag 一 turn 设一次粘整 turn（后续纯文本跟进仍走 vision provider）。

### 8. ncx-forge · 骨架训练框架
- **主线**：API-only/黑盒训练框架——不碰权重，把 genome（base `system_prompt` + 每工具 DESCRIPTION）当纯文本替换面进化：shell 调真实 `ncx.exe`、教师面板提议 TOML 覆盖、以 bench 通过率 delta 当梯度做爬山/Pareto；Rust 侧 `unwrap_or_default` no-op + SENTINEL 自检门是统计可信前提。
- **支柱**：Genome=system_prompt+tool_desc、no-op-on-failure 硬保证、--dump-genome 基线真相源、SENTINEL 自检门、train() 单冠军噪声感知爬山、evolve() Pareto 小种群、Evaluator 失败轨迹收割+token 成本解析、Teacher 面板 3 后端。
- **必背数字**：`SENTINEL='NCXFORGE_SENTINEL_4242'`、self_check `timeout=90,attempts=3`、train 默认 `rounds=3/repeats=1/timeout=120s/budget=1800s/accept_margin=1`、`pop_cap=4`、`SIZE_CAP_MULTIPLIER=3`/`FLOOR=12000`、top-`3` 失败轨迹、`MAX_TRAJECTORY_CHARS=2000`/arg≤120/留 12 个工具调用、empty eval→`(passrate=0,cost=+inf)`、splits `[train,train,train,val,train,train,test,val]`、taskgen reference 跑 2 次/`-n=3`/6 DIMENSIONS。
- **最易被追问**：加载失败为何 no-op 而非报错——优化器按通过率 delta 当梯度，silent-no-op 会把基线分误归给坏候选，故 SENTINEL 门在烧预算前先证明注入真生效。
