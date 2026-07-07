## Skills · MCP · 视觉

### 一句话主线
这三者都是「可选/外部能力」插进同一套 Tool + turn 机器：共享同一个设计直觉——常驻 context 永远便宜，重内容只在相关时才拉进来。Skills 用渐进披露(只把 name+description 常驻，body 按需 load)、MCP 把外部 JSON-RPC server 的工具包成本地 `Tool`(走同一条 Approver 路)、视觉用 per-turn 路由(带图的 turn 整体切到 vision provider)。三者都不改主循环——loop 对 provider 和工具来源是无感的。

### 30 秒 / 2 分钟 / 深挖 三档

**30 秒**
- Skills:两/三层渐进披露。L1 把 `discover_skills` 收集的 skill 的 `- name: description` 索引注入 system prompt(常驻、便宜);L2 模型调 `skill` 工具按名 load 全文 body;L3 用返回的目录路径配 `read_file` 读 bundled 资源。
- MCP:`McpClient` 是 stdio 上的 JSON-RPC-2.0 客户端,spawn server → initialize 握手 → list/call;`register_mcp_server` 连一次、列工具,把每个工具包成本地 `McpTool`,写类工具走 `Approver` 审批。
- 视觉:每个 turn 开头算 `use_vision_this_turn = vision_provider.is_some() && has_image_block(user_input)`,为真则该 turn 所有 model 调用走 vision provider,否则走主 provider。

**2 分钟**(加机制名+数字)
- Skills 影子链三层:builtin(`include_str!` 烤进二进制) < home(`~/.ncx/skills`) < workspace(`<ws>/.ncx/skills`),`BTreeMap<String,Skill>` 按 name 去重、同名后者覆盖,`.into_values()` 出来按名排序。`skills_index_block` 头是固定 `INDEX_HEADER`,每行 `- name: description`,无 skill 返回空串。`skill` 工具 `read_only()=true`、入参只有一个 string `name`、大小写不敏感匹配,且写死在 `ALWAYS_VISIBLE_TOOLS` 里不被 9 工具可见性上限砍掉。
- MCP:协议版本 `2024-11-05`,每个 request 30s timeout,`clientInfo={name:'nanocodex',version:'0.1'}`。`request()` 写一行 newline-delimited JSON,然后 read-until-matching-id(跳过 id 不符的通知/响应)。一个 server 的所有工具共享 **一个** `McpClient`,包在 `Rc<Mutex<McpClient>>` 里——`Mutex` 串行化单条 stdin/stdout 管道,`Rc`(非 `Arc`)因为整个 REPL 单线程。`is_read_only_name` 是纯名字启发式:6 个前缀 + 5 个精确词判读。
- 视觉:`has_image_block` 只认顶层 JSON 数组里 `type=='image_url'` 的块。flag 在 `run_turn_inner` 入口设一次,贯穿该 turn 所有迭代(包括 tool-result 跟进),保证整个 turn 不换 provider。无 vision provider 配置时图片 turn 静默留在主 provider——优雅降级。

**深挖**(实现细节与边界)
- `Skill::load_body()`:builtin(`embedded.is_some()`)直接返回已解析的 clone、不碰文件系统;否则 read 文件 + `strip_frontmatter().trim()`。`builtin_skills()` 用 const `BUILTINS` 数组的 `include_str!`,目前仅 1 个(commit-message),无 name 的 builtin 防御性跳过。
- frontmatter 解析容错:`frontmatter_lines` 判首行用 `l.trim() == "---"`,即先 trim 再比较,故首行带前后空白(如 `--- `)仍算合法 fence——它并不要求字节级精确等于 `---`(下面自测答案 8 给出的正是这个精确说法);无闭合 fence 当 malformed(空)。`strip_frontmatter` 则剥 BOM、要求开头 `---` 后跟 `\n`/`\r`、找闭合 fence,找不到则整篇当 body。`scan_root` 缺 name 回退目录名,trim 后空 name 跳过,读不了的目录/缺 SKILL.md 跳过(`let-else` 守卫)。所以纯 markdown 无 frontmatter 也能用。
- `format_content` 把 MCP 的结构化 content 数组拍平成单字符串:`type:'text'` 块(`Some("text")`)用 `\n` join;`type` 是别的非空字符串(`Some(other)`)渲染成 `[<type> content]`;而 **缺 `type`(或 `type` 非字符串)的块走 `None => {}` 分支被静默丢弃、不渲染**。有非空 `structuredContent` 时追加;空内容按 `isError` 给 `(tool error with no content)` 或 `(no content)`。逐行对齐 Python `format_result`。
- `McpTool.execute()` 审批分流:非 read_only 时先从 approval_policy 造一个 Approver——`Approver::new(&ctx.approval_policy).classify(&self.def.name, true)`(`classify` 是 Approver 实例方法,第二个参 `true` 是 `needs_escalation`) → AutoDeny 返错误串、Ask 走 `ctx.approver.request(command:'mcp:<name> <args>', escalated:true)` 不批则中止、AutoApprove/无 approver 放行;然后 `client.lock().await` + `call_tool`。和 ShellTool 同一条逃逸/审批路。
- 视觉:`active_provider()` 仅当 flag 为真 **且** `vision_provider` 是 Some 才返回 vision provider(`NCX_TRACE` 时打 `[ncx-trace] routing image turn -> vision provider`),否则主 provider;`call_model()` 永远 `active_provider().chat_streaming(...)`,所以循环本身 provider-agnostic。

### 核心机制 · 6 根支柱
- **Skills L1 索引块** — `discover_skills` 收集后 `skills_index_block` 只把 name+description 拼成索引注入 system prompt — 让常驻成本扁平,再多/再大的 skill body 也不进 prompt。
- **Skills L2/L3 `skill` 工具** — read-only、单 string `name`、大小写不敏感 find,命中返 `body + 目录字符串`,L3 靠该目录 `read_file` 读 bundled 文件 — 全文只在模型显式请求时才展开进 context。
- **`Skill::load_body` 双源** — builtin 走 `include_str!` 嵌入二进制、返回已解析 clone 不碰盘;filesystem 源 read + strip frontmatter — builtin 无需安装、零文件系统命中,贴合单二进制目标。
- **`McpClient` stdio JSON-RPC** — spawn server,initialize+notifications/initialized 握手,`request()` 同步 write+read-until-matching-id,30s timeout,Drop 时调 `start_kill()`(fire-and-forget 发起 kill、非阻塞等待、忽略结果) — 因 agent 工具调用本就串行,简单同步循环比后台 reader+响应表「少很多机器、行为相同」。
- **`McpTool` + 共享 client** — 一个 server 全部工具共享一个 `Rc<Mutex<McpClient>>`,`is_read_only_name` 名字启发式分流审批 — 单条 stdin/stdout 管道必须串行;MCP 无机器可读副作用标志,故用名字让明显读类跳审批、写类全走 ShellTool 那条升级路。
- **Per-turn 视觉路由** — turn 入口算一次 `use_vision_this_turn`,带图整 turn 切 vision provider,文本 turn 留主 provider — 单次多模态调用发给真能看图的模型,且 flag 一 turn 设一次保证整 turn 不换 provider,无 vision 配置时优雅降级。

### 关键数字 / 必背细节
- MCP 协议版本 `PROTOCOL = "2024-11-05"`;每 request `REQ_TIMEOUT = 30s`。
- `clientInfo = {name:'nanocodex', version:'0.1'}`。
- `DEFAULT_VISIBLE_TOOL_LIMIT = 9`;`ALWAYS_VISIBLE_TOOLS = 6` 个(read_file, apply_patch, update_plan, shell, tool_search, **skill**)。
- `McpToolDef.input_schema` 缺省 = `{"type":"object"}`(server 省略 inputSchema 时)。
- read-only 名字判定 = 6 前缀(read_/get_/list_/fetch_/search_/find_)+ 5 精确词(read|get|list|search|find)。
- builtin skill 现 = 1 个(commit-message);skill 发现根 = 2 个有序(`~/.ncx/skills` 然后 `<ws>/.ncx/skills`)。
- 视觉触发 = 顶层数组块 `type == "image_url"`。
- home 目录解析顺序:`USERPROFILE` 然后 `HOME`。

### 取舍与坑
- **MCP id 匹配脆弱**:`request()` 用 `v.get("id").and_then(|x| x.as_u64()) != Some(id)` 来决定是否 `continue` 跳过(`v.get("id")` 返 `Option<&Value>`,本身没有 `.as_u64()`,故 `.and_then` 是承重的;实现里用的是取反的 `!=` 形式)。`next_id` 是 u64 从 0、用前自增(故 id 是 1,2,3…)。若 server 把 id 返成 JSON 字符串,`as_u64()` 得 None,该响应被跳过 → 最终 30s timeout。
- **MCP server stderr 被丢**:spawn 时 stderr → `Stdio::null()`,server 诊断信息全丢,只能拿到泛泛的 `closed stdout`/`timeout` 错误。
- **`is_read_only_name` 纯名字启发式**:无 server 副作用信号。`getOrCreate` 会被当只读跳审批;`read_and_delete` 也被当只读。匹配在 `to_lowercase()` 上;但 `getx`(无下划线、不是精确词)**不**算只读。
- **`skill` 工具仅当 `ctx.skills` 非空才注册**(tools.rs:271)。`builtin_skills()` 恒 ≥1,故经 `discover_skills` 实际总在;但未调 `with_skills()` 构的 `ToolContext` skills 为空、无 skill 工具。
- **视觉 flag 粘整 turn**:`use_vision_this_turn` 在 turn 入口设一次,贯穿所有迭代,带图开局的 turn 即使后续是纯文本 model 调用也仍用 vision provider。
- **skill 同名大小写**:工具匹配 `eq_ignore_ascii_case`,但 `BTreeMap` 按精确 name 去重——仅大小写不同的两个 skill 都能存活发现,工具返回 iterator 先找到的那个。
- **frontmatter fence 检查不一致**:`strip_frontmatter` 用字节前缀(开头 `---` 后须跟 `\n`/`\r`),`frontmatter_lines` 用 `l.trim() == "---"`,首行 `--- `(带尾空格)在两条路上行为微妙不同:`frontmatter_lines` 因 trim 仍判合法,byte-prefix 路因尾空格判定不同。

### 高频追问与应答
- **Q:为什么不把 skill 全文都放进 system prompt?**
  A:常驻成本要扁平。L1 只放 name+description 索引,body 可能很大,只在模型调 `skill` 工具时按需 load(L2),bundled 资源再靠返回目录 + `read_file`(L3)。和 tool_search 同一两层模式。
- **Q:一个 MCP server 的多个工具如何共享连接?并发安全吗?**
  A:`register_mcp_server` 只 connect 一次,把单个 `McpClient` 包成 `Rc<Mutex<McpClient>>`,每个 `McpTool` 持 `shared.clone()`。stdio 是单进程单管道,必须串行,`Mutex` 负责串行化;用 `Rc` 不用 `Arc` 因 REPL 单线程(!Send 运行时),`Mutex` 不是为跨线程而是为管道独占。
- **Q:MCP 怎么决定哪个工具要审批?**
  A:无 server 端副作用标志,只能用 `is_read_only_name` 名字启发式(6 前缀+5 精确词)。判为只读的放行,其余走 `Approver`——和 ShellTool 完全同一条:`Approver::new(&approval_policy).classify(name, true)` → AutoDeny 拒 / Ask 调 `approver.request(escalated:true)` / AutoApprove 放行。
- **Q:带图的 turn 里后续 tool-result 跟进调用走哪个 provider?**
  A:仍走 vision provider。`use_vision_this_turn` 在 `run_turn_inner` 入口设一次、贯穿该 turn 全部迭代,`call_model` 每次都 `active_provider()`,所以整 turn 不换。这是刻意的——避免一 turn 内 provider 抖动。
- **Q:没配 vision provider 时发图会怎样?**
  A:优雅降级、不报错。`active_provider()` 要求 flag 为真 **且** `vision_provider` 是 Some 才切,缺 provider 时 `has_image_block` 检测永不改变行为,图片 turn 静默留在主 provider。
- **Q:一个没有 frontmatter 的纯 markdown SKILL.md 能用吗?**
  A:能。`scan_root` 缺 name 回退目录名;`strip_frontmatter` 找不到合法 fence 就把整篇当 body。malformed 文件被跳过而非 crash discovery。

### 自测 · 主动回忆
1. [L1] Skills 的三层渐进披露分别在哪一步把什么放进 context?
2. [L2] `skills_index_block` 的影子覆盖顺序是什么,用什么数据结构去重?
3. [L2] 一个 MCP server 的多个工具为什么、怎么共享同一个 `McpClient`?为何用 `Rc` 不用 `Arc`?
4. [L3] `McpClient.request()` 如何把响应和请求对上号?这个机制在什么输入下会触发 30s timeout?
5. [L2] 视觉路由的触发条件是什么?flag 在哪里、何时设,作用域多大?
6. [L3] `is_read_only_name` 的判定规则是什么?举一个误判为只读和一个该只读却不算的例子。
7. [L4] `Skill::load_body()` 对 builtin 和 filesystem 源分别做什么?builtin 的 body 从哪来、为何不碰文件系统?
8. [L4] frontmatter 解析里 `frontmatter_lines` 与 `strip_frontmatter` 在首行 `--- `(尾空格)上的行为差异。

**答案要点**
1. L1:`discover_skills`→`skills_index_block` 把 name+description 索引注入 system prompt(常驻);L2:模型调 `skill` 工具按名 `load_body()` 把全文 body 放进 tool result;L3:用返回的目录 + `read_file` 拉 bundled 资源。
2. 顺序 builtin < home(`~/.ncx/skills`) < workspace(`<ws>/.ncx/skills`),同名后者覆盖;`BTreeMap<String,Skill>` 按 name 去重并排序,`.into_values()` 输出。
3. `register_mcp_server` connect 一次,单个 client 包进 `Rc<Mutex<McpClient>>` 由各 `McpTool` `clone` 共享;stdio 单管道必须串行,`Mutex` 串行化;`Rc` 因 REPL 单线程、!Send 运行时,无需跨线程的 `Arc`。
4. 写一行 JSON 后 read-until-matching-id:实现用 `v.get("id").and_then(|x| x.as_u64()) != Some(id)`(取反形式,匹配不上就 `continue`),id 不符的(通知/其它响应)跳过。若 server 把 id 返成 JSON 字符串,`as_u64()` 得 None,永远匹配不上 → 整个 read 在 30s timeout 内耗尽。
5. 条件 `vision_provider.is_some() && has_image_block(user_input)`,`has_image_block` 认顶层数组块 `type=='image_url'`;flag `use_vision_this_turn` 在 `run_turn_inner` 入口设一次,作用域是整个 turn(含所有 tool-result 迭代)。
6. 小写后:前缀 read_/get_/list_/fetch_/search_/find_ 或精确词 read|get|list|search|find 判只读。误判只读:`getOrCreate`、`read_and_delete`;该只读却不算:`getx`(无下划线,不是精确词)。
7. builtin(`embedded.is_some()`)返回已解析 body 的 clone、不碰盘;filesystem 源 read 文件 + `strip_frontmatter().trim()`。builtin body 来自 const `BUILTINS` 的 `include_str!`(编译期烤进二进制),故零文件系统命中、无需安装。
8. `frontmatter_lines` 判首行用 `l.trim() == "---"`,故 `--- ` 经 trim 后仍当合法 fence,且它并不要求字节级精确 `---`;`strip_frontmatter` 用字节前缀、要求开头 `---` 后紧跟 `\n`/`\r`,`--- ` 因尾空格在 byte-prefix 路上判定不同——两条路在该输入上行为微妙不一致。

### 别发散到这(属于其它子系统)
- tool_search 的可见性过滤/9 工具上限本体机制(这里只借「skill 在 ALWAYS_VISIBLE_TOOLS」这一点)。
- fast/pro 分层 orchestrator、`-o` 标志、fast_model 路由(那是 capability/orchestrator 子系统)。
- `ShellTool` 本身的 readonly 沙箱与 Approver 内部 classify 细节(这里只说 MCP 复用了同一条路)。
- memory layer / `MEMORY_RECALL_MAX_*`(虽在 agent_loop.rs 同文件,但与这三能力无关)。
- provider 自身的 `chat_streaming` 实现、SSE 解析、DeepSeek/具体 vision provider 的 wire 协议。
- `apply_patch`/`read_file` 的补丁格式与文件读语义(只作为 L3 的下游消费者出现)。

### 一句话收尾
记住一根脊:三个能力都是「外部/可选能力插进同一套 Tool+turn 机器」,统一靠「索引常驻、重内容按需」——Skills 按需 load body、MCP 按需 spawn+串行调用、视觉按需切 provider,主循环始终无感。
