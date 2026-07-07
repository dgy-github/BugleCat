# nanocodex · 驾驭模块深挖追问题库（被追问时的备弹）

> 定位：现有刷题卡是「承重环」顶层题；本文件是**面试官往下钻时的备弹**——按驾驭工程每个模块列细节追问，每题给 1–2 行答题点（不是完整 STAR，是"能对答如流"的要点）。
> 用法：录音刷完承重题后，逐模块过一遍这些追问，卡住的标记出来重点背。全部基于真实代码（`ncx-cli/main.rs`、`ncx-core`、`ncx-sandbox`、`ncx-tools`、`train/`）。

---

## 模块 · Prompt / genome（Prompt + Feedback 接缝）
- **追问：genome 到底能改什么、不能改什么？** → 只能改 `system_prompt` + 每个工具的 `description`（**纯文本**）；绝不改工具行为/权限/执行。
- **追问：`NCX_GENOME` 没设置或格式错会怎样？** → `Genome::default()`，与硬编码脚手架**字节级一致**（可证明 no-op）；正因如此离线"全绿"才不是假象。
- **追问：最终 system prompt 怎么拼出来的？** → `compose_system_prompt(base ⊕ instructions ⊕ skills_index ⊕ plan_note)`，`base = genome.base_system_prompt(SYSTEM_PROMPT)`。
- **追问：genome 覆盖在哪一步生效？** → 注册工具时 `genome.describe(name, default)` 烘焙进 tool_catalog；每回合 `schemas_for_query` 后再应用一次覆盖。

## 模块 · Context 上下文压缩
- **追问：两趟压缩各压什么？** → ① 旧 `tool` 结果压到 `max_tool_result_chars`；② 仍超 `max_chars` 则在 **user 边界**丢最旧前缀。
- **追问：为什么切点绝不能落在 tool_result？** → 否则历史以孤立 `tool` 消息开头 → 下次请求 **400**。
- **追问：压缩是破坏性的吗？resume 怎么办？** → 非破坏：`for_model_edited` 是**发送时视图**，`self.messages` 原始不动 → resume/回放/换模型重算都用原始。
- **追问：token 怎么估的？** → `chars/4` 保守高估；图片按固定字符数（`ESTIMATED_IMAGE_CHARS`）折算。

## 模块 · Tools 工具系统
- **追问：工具太多模型选不过来怎么办？** → `schemas_for_query` 渐进暴露：`tools≤9` 全暴露，否则 `ALWAYS_VISIBLE_TOOLS` + 上回合 `tool_hints` + `catalog_score` 词法打分，**上限 9**。
- **追问：tool_search 怎么形成反馈回路？** → 它重写 `ctx.tool_hints`，命中工具的 schema **下一回合**浮现。
- **追问：什么时候并行执行工具？** → 当前 + 下一个调用**都 read_only** 才 `join_all` 并发；写/未知一律串行。
- **追问：MCP 工具怎么接、怎么保证不乱序？** → `Rc<Mutex<McpClient>>` 串行化 stdio RPC；调用前过 `Approver` 门控。

## 模块 · Guardrails 权限（最重，追问最多）
- **追问：两道门分别判什么？** → `SandboxPolicy.can_write` 判"物理能不能写"（3 档 + 词法路径包含）⊥ `Approver::classify` 判"越界怎么办"（4 档 → 3 态）。
- **追问：can_write 解符号链接吗？** → `can_write` 本身纯词法（转绝对 + normalize，不碰 FS）；但调用前 shell 先 canonicalize workdir、apply_patch 先 canonicalize 每个目标——符号链接在两条路上都已解析。真正不对称：shell 只查 `can_write(WORKDIR)`，apply_patch 逐目标查。
- **追问：untrusted 三筛顺序？为什么正则第一？** → ① 7 条危险正则扫全串（任意位置命中即 false）② `first_token` 查 29 白名单 ③ `base==git` 查 15 写子命令。正则第一是 fail-safe——`rm -rf` 即使被可信 token 引导也先拦。
- **追问：SessionGrants 精确到什么程度？** → 精确字符串匹配：`ls -la` ≠ `ls -l`；session 级 `Rc<RefCell>`，new/fork/resume 即丢。apply_patch 的 Always 仅当无越界目标才设 `allow_edits`。
- **追问：danger-full-access 是不是全放行？** → 否，只清零 escalation bit、不改 approval policy；`untrusted + danger` 下未知命令**仍 Ask**（classify 的 is_trusted 与 escalation 无关）。
- **追问：on-failure 重跑是不是放松了沙箱？** → 没有，重跑**同一条命令**走同一 PolicyExecutor；PolicyExecutor 根本没有第二个非沙箱模式，重跑只是用户 bless 了一次。
- **追问：ApprovalHandler 为什么住 ncx-core 不在 sandbox crate？** → 让 sandbox 保持 **prompt-free 纯决策**；体现 policy(物理)/approval(越界)/prompt(人类) 三层解耦。
- **追问：PolicyExecutor 参与审批决策吗？** → 完全不参与，只做 Job Object 容器（KILL_ON_JOB_CLOSE 杀进程树）+ timeout（超时 exit 124）；`.run()` 被调时 auto/ask/deny 已定死。

## 模块 · Memory 记忆
- **追问：recall 注入的是"事实"吗？** → 不，标为 **「线索而非事实」**（`RECALL_HEADER`），不当 ground truth 用。
- **追问：什么内容才会被 remember？** → 仅 **CONFIRMED** + 精确去重 + 200 上限。
- **追问：consolidate 干嘛的？** → 启动即跑的**幂等启发式近重复合并**（阈值 0.85），防近重复堆积。

## 模块 · Workflow / Harness（run_turn）
- **追问：循环上限怎么定？** → `max_model_calls = max_iterations.min(task_budget.max_model_calls.max(1))`；每步查 cancel + 剩余工具预算。
- **追问：cancel / 预算耗尽时历史怎么保持合法？** → `backfill_unanswered_tool_calls` 为每个未应答 tool_call 合成 `[interrupted…]` 占位回复，否则历史非法 → 400。
- **追问：视觉模型什么时候用？** → `use_vision_this_turn = vision_provider.is_some() && has_image_block(user_input)`，按回合切 `active_provider()`。

## 模块 · Orchestration best-of-N（多节点 Workflow）
- **追问：父子 agent 之间怎么通讯？有总线吗？** → 无总线：① prompt 下发（状态按值序列化进全新无状态子会话）② 文件隔离-提升（真 IPC）③ `PASS/FAIL/BEST` 裁决回灌。
- **追问：并行 worker 会不会互相覆盖文件？** → 不会，每个 `ncx_worker_{pid}_{n}` 私有递归拷贝；真 workspace 只在 verify 后 `promote_worker` 写一次。
- **追问：classify/plan/verify 凭什么拿不到工具？** → `reason()` 走 `ToolRegistry::empty`——**代码级剥夺**，不是 prompt 劝阻（verify 的 prompt 甚至没写 "no tools"）。
- **追问：verify 输出畸形怎么办？** → `verdict_passed = !contains("FAIL")`（位置无关）；`BEST:` 缺失/畸形兜底 index 0——偏向"往前推进"不卡死。
- **追问：递归子任务为什么串行？** → 子任务可能依赖，`for` 循环保证子任务 k 先 `promote` 落地，k+1 才能 copy_tree 到已含 k 改动的 workspace。

## 模块 · Evals / Feedback（ncx-forge 离线）
- **追问：离线自动优化怎么保证不引入安全风险？** → 只演化文字 + 执行永过沙箱 = **训练接缝即安全边界**；被不可信失败轨迹喂养的 teacher 也注入不了新能力。
- **追问：teacher 怎么变异、怎么接受？** → `reflective mutation → TOML 覆盖`；evaluator = bench + **隐藏 check.py**；accept 要 TRAIN margin & VAL 无回归；pareto 择优 passrate↑ / cost↓。
- **追问：sentinel 门是干嘛的？** → 无法证明注入确实生效前**拒绝训练**——防"离线全绿是假象"。

---

### 追问命中率自检
按模块过一遍，卡住的标 ✗：
`[ ] Prompt/genome  [ ] Context 压缩  [ ] Tools 渐进暴露  [ ] Guardrails 权限(重)  [ ] Memory  [ ] Workflow  [ ] best-of-N  [ ] ncx-forge`
> Guardrails 那栏追问最密（8 条），面试被钻的概率最高，优先刷到全绿。
