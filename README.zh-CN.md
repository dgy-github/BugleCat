# BugleCat

[English](README.md) | 简体中文

## 最新更新（2026-10-23）

> [!IMPORTANT]
> **第三阶段里程碑（2026-10-23）：自进化 Harness 正则化（对照 Google RRSI / Harness²，P0–P4 全量落地）与配对复利回放实证**
> - **P0 —— 噪声校准配对评测（[`evals/compounding_replay/analyze.py`](evals/compounding_replay/analyze.py)）与第二基准仓库（`python-tabulate`）**：
>   * **统计配对与噪声下限校准**：引入单臂噪声下限校准（解决数按 Jeffreys 平滑伯努利方差求 $\delta_{\text{resolved}}$，成本按逐题样本方差和求 $\delta_{\text{cost}}$）与双臂逐题配对检验（精确 McNemar 检验 + 4000 次按题配对 Bootstrap 95% 置信区间 + 解决率非劣效护栏）。
>   * **第二真实仓库题库（`astanin/python-tabulate`）**：加固 [`mine_tasks.py`](evals/compounding_replay/mine_tasks.py)（新增 `--body` 全文检索与合并提交支持、`issue["created_at"] <= fixed_at` 时间戳护栏精准拦截 7 个早年 Bitbucket 编号错配任务），通过 [`validate_gold.py`](evals/compounding_replay/validate_gold.py) 完成 9/9 官方黄金补丁验证。
>   * **`tabulate` 3 轮配对实测（每臂 27 次运行）**：Arm B 相比 Arm A 将主指标「每解决一题成本（`cost_per_resolved`）」降低了 **21.0%**（**`B / A = 0.7904`，按题配对 Bootstrap 95% CI `[0.6239, 0.9523]`，不包含 `1.0`**；每轮均摊成本 `1.313` 元 vs `1.586` 元，配对 95% CI `[-0.5207, -0.0685]`），同时严格解决率满足非劣效护栏（`22/27` vs `21/27`，`+3.7pt >= -14.9pt`）。注：用同一配对脚本重算早期 2 轮 `sqlparse_split` v2，得 `B/A = 0.7131`，95% CI `[0.3693, 1.1586]`（跨过 `1.0`，定性为未定论）。
> - **P1 —— `ncx-forge` 离线进化搜索的 RRSI 正则化（`train/`）**：
>   * **表面级退火编辑预算**：新增 `changed_surfaces` 与 `changed_lines`，支持按轮退火的表面数预算（$\max(1, \text{max\_surfaces} - \lfloor r / 2 \rfloor)$）与修改行数硬上限（`--max-surfaces`、`--anneal-surfaces`、`--max-changed-lines`）。
>   * **规则版反过拟合/反作弊审查器（`critic_check`）**：自动拦截在新增文本中硬编码训练集题号、`check.py`、`_check.py`、`hidden test` 的作弊候选。
>   * **证伪假设账本与停滞自动转向**：每个候选写入结构化实验账本 `train/runs/ledger_<stamp>.jsonl`，向 Teacher 自动注入最近被拒的证伪历史（`Falsified / Rejected Mutations`），并在连续 `--stagnation-limit` 轮停滞时自动转向探索最少的表面。
>   * **双通道成本感知验收门与盘后消融剪枝（`--prune`）**：支持「通过数提升 + `--max-token-growth` 防膨胀护栏」或「同分下显著节省 Token（`>= --delta-tokens`）」双通道晋级，并在搜索结束后对最终 Champion 逐面回滚重评，剔除零收益的搭便车冗余修改。
> - **P2 —— 扩大 `NCX_GENOME` 可进化面（`rust/` + `train/`）**：
>   * 在保持空 Genome 100% 字节级 No-op 等价保证（且不触碰冻结文件 `prompt.rs`）的前提下，新增 `[orchestrator_prompts]`（覆盖 `classify_sys`、`plan_sys`、`decompose_sys`、`worker_sys`、`verify_sys`）与有界 `[memory_config]`（`max_entries` $\in [1, 32]$、`max_chars` $\in [200, 16000]$），打通 `ncx --dump-genome` 与 `train/genome.py` 双向校验。
> - **P3 —— Harness² 在线记忆双层结构化、负向回归护栏与成本感知归因（`rust/crates/ncx-core/`）**：
>   * **模块级不变量提炼（`promote_module_invariant_if_due`）与负向回归护栏（`record_regression_guardrail`）**：当同一模块在外部裁判确认通过（`Accepted`）后累计达到 `>= 2` 条验证笔记时，自动合并提炼结构化模块不变量 `[Module Invariant: <module_path>]`（赋予 `+6` 召回优先级加分，并按规范化头部哈希保持原地更新时 `note_id` 与历史权重不变），同时将该模块名下的旧单题情景笔记裁剪至最多 `MAX_EPISODIC_PER_MODULE = 2` 条并同步重建 SQLite 索引（`rebuild_from_markdown`）；当某次修改导致 `pass_to_pass` 回归或 `Rollback` 时，自动按模块提炼 `[Regression Guardrail: <module_path>]` 负向护栏。
>   * **召回预算收紧与高消耗轮次精准打折（`record_unhelpful_injection`）**：默认召回上限从 `8 条 / 4000 字符` 收紧至 `6 条 / 2400 字符`；当高消耗轮次（`iterations > 16 || tool_calls > 24 || total_tokens > 80_000`）完成时，仅命中实际修改文件名的召回笔记获得 `accepted + 1`，未命中的搭便车笔记仅记 `injected + 1`。在 `tabulate` Arm B' 实测（`20261023-115758-B-tab-p3-r1`）中，`learnings_entries` 从第 4 题到第 9 题全程稳定封顶在 `6` 条，总成本 `1.413` 元（较 Arm A 均值下降 `10.9%`）。
> - **P4 —— Harness² 运行时与离线进化双层 `Propose–Probe–Compose` 闭环（[`orchestrator.rs`](rust/crates/ncx-core/src/orchestrator.rs)、[`train/forge.py`](train/forge.py)、[`train/lesson_lib.py`](train/lesson_lib.py)）**：
>   * **运行时多 Worker 非冲突文件级组合（`compose_with_paths` / `COMPOSE:i,j`）与重试探测合成（`build_compose_feedback`）**：当 Verifier 在输出 `BEST:i` 的同时给出 `COMPOSE:i,j` 时，`HarnessAgentRunner::compose_workers` 优先提升主 Worker 的全部修改，并将次级 Worker 在不冲突文件（主 Worker 未触碰或 SHA-256 指纹完全相同）上的修改自动组合提升到工作区；当首轮 $N \ge 2$ 个 Worker 未通过验证进入重试时，自动将 Verifier 审查意见与所有 Worker 的探测结论摘要（`[Propose-Probe-Compose Peer Digest]`）合成为下一轮输入。
>   * **离线 `ncx-forge` PPC（`--h2-k`、`--h2-mode parallel|sequential`、`--h2-blind`）与跨轮改进库（[`train/lesson_lib.py`](train/lesson_lib.py)）**：对单一失败任务在同条件下探测 incumbent 对照 + $K$ 个通用/领域特定候选（`keep_trajectory=True`），确定性生成隐藏裁判结果的纯行为对比（`build_contrast`），由 Teacher 合成 1 个 `composed` Genome 与 1 条元经验草案（越界时自动回退至最佳合法 Probe 候选），再进入外层全量 `train + holdout` 双通道门。`LessonLibrary`（`lessons.jsonl` + `adopted_harnesses.jsonl`）按症状重叠度 $\times$ Beta 后验 `(support+1)/(support+refute+2)` 检索经验，在外层门通过/被拒与 `--prune` 消融时更新 `support`/`refute`，并在 `refute >= support + 2` 时自动退役为证伪禁令；[`replay.py`](evals/compounding_replay/replay.py) 新增 `--genome PATH` 支持直接回放评测进化产物（详见 [`docs/harness2-forge-integration.zh-CN.md`](docs/harness2-forge-integration.zh-CN.md)）。
>
> ---
>
> **第三阶段底座（2026-10-19）：智能复利闭环、无人工介入完成率（ACR & 60 分奇点）+ 跨 Agent 平台升级**
> - **无人工介入完成率（ACR）与 60 分奇点门禁**：在单 Agent 与多智能体编排主控出口统一采集隐私脱敏的 `TaskOutcome`，收敛为 **4 个核心工程任务类别**（`code_fix_test`、`compile_repair`、`feature_impl`、`doc_refactor`；纯问答 `chat` 与子任务 `OrchestratorWorker` 排除出主分母）。当某类别在滚动 30 天内已定论样本 `n >= 30`、`ACR-strict` 的 Wilson 95% 置信下界 `>= 60%` 且返工率 `<= 15%` 时，正式判定该类别跨过 **60 分奇点**。
> - **零守护进程全链路后续信号（`I1–I5` + `L`）**：自动归因 `I1` 显式阻断/失败、`I2` 审批/提问介入、`I3` Checkpoint `/restore` 回滚、`I4` 中/英/日多语言纠正与同文件返工推断、`I5` **零常驻 `LazyGitUndoDetector`**（在新会话启动及下一轮开始前通过 `git merge-base --is-ancestor` 与 `git status --porcelain` 检测跨会话 `git reset` / `git revert` / 手工覆写 Agent 修改过的文件），以及 `L` 人工显式标签（CLI/GUI `/outcome good|bad` + GUI 消息级 `♡`/`♧` 精准按轮次绑定）。
> - **带 14 天基线门禁的在线回流（`shadow` → `on`）**：`[feedback] memory / routing = "off" | "shadow" | "on"`（默认 `"shadow"`）。从 `shadow` 切换至 `on` 受 **14 天 Shadow 基线门禁** 保护（需满足至少 14 天 Shadow 观测期且至少 2 个核心类别 `n >= 30`，支持 `--force` 强制覆写）。开启后执行有界 Beta 后验记忆调权（`±20%`，每个已定论任务仅计分一次）、低效记忆自动降级（`injected >= 8 && p < 0.25`）与编排器路由单调升级（`Simple -> Medium -> Complex`）。
> - **复合恢复链沉淀、Worker 隔离脱敏与频次优先晋升**：自动提取多步 `tool_failed -> diagnostic/read -> tool_succeeded` 复合恢复链（`CompoundRecoveryChain`）为 `Candidate` 候选记忆；子任务隔离副本（`.buglecat-subruns`）不直接写主库，仅在胜出 Worker 被主控 `promote` 且回合成功时，将临时路径脱敏还原为真实工作区路径后落盘；支持跨会话复现计数（`occurrences`，按 `命中 N 次` 降序排列）、零介入高置信度自动晋升（`memory_auto_promote_enabled`）与人工审阅（`ncx --memory-review` / GUI 审阅面板）。
> - **跨 Agent 核心机制吸收（Pi / OpenCode / Codex）**：
>   * **Pi 风格会话内建树状链路追踪（`_trace`）**：每条 `session.jsonl` 消息内嵌 `MessageTrace`（`id`/`parent_id` 树指针、轮次号、SHA-256 摘要、Git revision、生效模型与单步 `usage`、工具耗时与退出码、压缩位点 `first_kept_entry_id`），发往模型前自动剥离。
>   * **OpenCode 风格编辑后自动诊断回写**：`apply_patch` 与 `str_replace_editor` 修改文件后自动触发增量 LSP 诊断（`textDocument/didChange`）、JSON/TOML 语法校验与 Git 冲突标记检测，直接追加至工具返回值。
>   * **Codex 风格超长工具输出自动落盘卸载**：单次工具输出超 16,000 字符时完整落盘，内联返回前 8,000 + 后 8,000 字符预览及 `[tool_output_offloaded: path=... offset=...]` 句柄供 `read_file` 按需分段精读。
> - **常用命令与桌面端入口**：
>   * 查看工作区奇点与 ACR 报告：`ncx --outcomes report --since 30d --by category,model,policy`（或 REPL / GUI 输入 `/outcome report`）
>   * 跨项目历史会话冷启动提取：`ncx --outcomes bootstrap <仓库路径1> <仓库路径2> ...`
>   * 候选与降级记忆审阅：`ncx --memory-review [list | confirm <id> | reject <id> | restore <id>]`
>   * 桌面端可视化奇点仪表盘：打开 Tauri 桌面端右侧 **「记忆」** 面板，即可实时查看 **ACR & 60 分奇点仪表盘**、分类样本进度与 14 天基线状态。
> - **`v0.1.2` 预编译发行包与声明级核验基线（`host-rules-v3.4`）**：
>   * [GitHub Release `v0.1.2`](https://github.com/dgy-github/BugleCat/releases/tag/v0.1.2)（`BugleCat_0.1.0_x64-setup.exe`、`buglecat-20261014-final.zip`、macOS `arm64`/`x64` DMG）· [三模式验收收据](reports/buglecat-delivery-20261014.json) · [175 例声明漂移基准报告](reports/claim_drift_eval_report_v1.md)

## 合作与自测证据

最新：[9 月 8 日下午复跑报告](docs/reports/self_repair_20260908_081319.md) —— **首轮 6/6**，六份补丁独立重放全部通过，Agent 累计 609.09 秒。与下方早前运行使用相同模型和执行器；小样本结果不代表稳定成功率。

欢迎交流 **Agent 集成与定制、评测与自修复研究、联合开发**。合作联系：**[2537676793@qq.com](mailto:2537676793@qq.com)**。邮件中可介绍你或团队、使用场景、希望合作的内容与时间安排；具体范围和交付方式沟通后确定。

在 **2026 年 9 月 8 日**使用 `deepseek-v4-pro` 完成的六类源码自修复测试中，BugleCat **首轮通过 5/6 道，每题最多一次反馈后最终通过 6/6 道**，结果经过独立验收与补丁重放复查。这是指定目标文件、提供可见测试的小样本故障注入自测，不代表普遍成功率或生产可靠性认证。

[阅读完整自我测试报告](docs/reports/self_repair_20260908.md) · [评测证据与复跑方法](evals/self_repair/README.md) · [合作介绍](nanocodex.html#collaboration)

## 能力说明页

[打开能力说明页](nanocodex.html) · [BugleCat 仓库](https://github.com/dgy-github/BugleCat)

[设计说明 PDF](docs/ai-coding-agent-design-brief.pdf) · [设计说明 HTML](docs/ai-coding-agent-design-brief.html)

[自我测试报告：六类源码自修复](docs/reports/self_repair_20260908.md) · [评测子包与复跑入口](evals/self_repair/README.md)

📖 **[设计理念手册](docs/design-philosophy.zh-CN.md)** —— 系统讲解分层编排、递归子任务分解、无工具推理节点、渐进披露、视觉分流、记忆自进化与基准方法论“为什么这样设计”。

🛡️ **[上下文压缩安全协议](docs/context-compression-safety-protocol.zh-CN.md)** —— 解释 Coding Agent 为什么会在压缩后发生任务状态漂移，以及如何用任务快照、决策溯源、Git 证据和只读恢复控制风险。

📈 **[智能复利回流与 60 分奇点方案](docs/intelligence-compounding-feedback-plan.zh-CN.md)** —— 详解 BugleCat 如何将过程与结果数据串联为「行动 → 结果（`TaskOutcome` + `OutcomeFollowup`）→ 反馈 → 学习（有界记忆调权 + 路由单调升级 + 复合恢复链沉淀）」的闭环，并持续度量无人工介入完成率（ACR）与 60 分奇点距离。

🧬 **[自进化 Harness 架构与实现方案（对照 Google RRSI & Harness²）](docs/self-evolving-harness-rrsi-harness2.zh-CN.md)** —— 详解 BugleCat 如何打通 L0 任务内 `Propose–Probe–Compose`（Harness²）、L1 跨任务双层记忆（模块不变量 vs 单题情景）与负向回归护栏（Harness²）、以及 L2 离线 `ncx-forge` 正则化基因组进化（RRSI），并给出配对 Bootstrap 回放实证。

🧬 **[Harness² 接入 forge 技术说明](docs/harness2-forge-integration.zh-CN.md)** —— 设计稿：在 `train/forge.py` 中引入 Harness² 的 Propose–Probe–Compose 任务级递归与跨任务经验库，并与现有 RRSI 正则化关卡衔接。

[![BugleCat 桌面端真实截图：Tauri + Svelte 桌面客户端](assets/buglecat-desktop-real.png)](nanocodex.html)

**BugleCat** 是一个本地运行且可扩展的 Codex 风格编码 Agent。chat-completions 模型
提出结构化工具调用，Agent 在受控沙箱与审批策略下执行文件/shell/LSP/终端工具，记录会话与事件快照，并循环直到任务完成。
它可以对接 DeepSeek 托管 API，也可以对接任意 OpenAI 兼容的本地模型，并自带
MCP 集成、插件生态、Skills 系统、沙箱/审批状态机、上下文压缩、token 成本统计、Tauri 2 + Svelte 5 桌面客户端、定时器，以及 git worktree 的 A/B 对比。

项目由最初的 `nanocodex` 原型演进而来。为了兼容已有安装，配置目录（`~/.nanocodex/`）、环境变量（`NANOCODEX_*`）和
Python 兼容命令中仍保留旧名称；对外产品和新仓库统一使用 **BugleCat**。项目历经**三个清晰的演进阶段**：从 Python 快速原型探索，到原生 Rust + Tauri 强类型运行时重构，再到当前第三阶段的**智能复利回流与 60 分奇点闭环**。

## 项目阶段

### 第一阶段：Python 基础版

`nanocodex/` 下的 Python 实现是最早的兼容功能线，目标是快速验证产品形态：先把
agent 循环、工具体验、审批模型和桌面流程跑通，再决定哪些部分需要更强的工程边界。

**架构层面**

- 以一个紧凑的 async agent loop 为中心：调用模型 -> 执行工具 -> 更新 session ->
  进入下一轮模型调用。
- 工具、provider、sandbox、MCP、skills、memory、scheduler、compaction、GUI 都是
  可独立扩展的 Python 模块，适合在产品形态还不稳定时快速迭代。
- 运行时契约偏动态，因此新增 MCP marketplace、prompt 增强、图片输入、session
  resume/fork、A/B worktree 对比等功能成本很低。
- 早期原型 GUI 使用 Tkinter，第一版桌面体验依赖少、调试直接。

**性能与交付层面**

- 优势是迭代速度：没有编译步骤，实验快，mock provider 的离线测试也容易铺开。
- 离线单元测试覆盖 Python 功能线，不需要真实 API key，也不依赖网络。
- 交付仍依赖 Python 解释器、包环境和导入启动成本；对普通桌面用户来说，分发
  体验不如原生二进制直接。
- 动态边界适合探索期，但当沙箱、工具执行、记忆、MCP、并行 agent flow 变多时，跨模块
  状态和行为边界会越来越难推理。

### 第二阶段：Rust 重构版

`rust/` 下的 Rust 实现是原生发布底座。它保留 Python 树用于兼容与评测参考，同时把核心运行时重建为职责清晰的 Rust crates，并配套 Tauri v2 + Svelte 5 桌面端。

**架构层面**

- 工作区按职责拆分为 `ncx-config`、`ncx-provider`、`ncx-sandbox`、`ncx-tools`、
  `ncx-mcp`、`ncx-context`、`ncx-thread-store`、`ncx-protocol`、`ncx-app-server`、
  `ncx-core` 与 `ncx-cli`。
- provider 响应、工具调用、沙箱决策、session 消息、记忆条目、编排结果与前后端协议均有显式类型，
  跨 crate 边界时不再靠松散 dict/object 传递。
- 工具执行集中到 `ToolContext` 和 `ToolRegistry` 后面，沙箱策略、审批策略、超时、
  搜索和记忆都挂在真正执行动作的边界上。
- `AgentLoop` 由可替换的模型 Provider、命名 turn-context provider、工具注册表、工具中间件与可注入调度器组装而成；CLI、GUI 与编排 worker 通过同一套 `AgentRuntimeProfile` 解析预算、上下文编辑、并发与模型端点，避免前端配置漂移。
- 编排层加入任务分类、main/fast 模型路由、隔离 worker 工作区、verifier 选择，以及
  把胜出 worker promote 回真实工作区。
- 项目记忆保存在 `.ncx/memory/LEARNINGS.md`；启动时只做便宜的启发式去重，显式运行
  `ncx --memory-merge` 才会做成本更高的 LLM 记忆合并。
- 桌面线采用 Tauri v2 + Svelte 5，把原生 Rust 后端和 Web UI 表层分离，支持构建轻量级 Windows NSIS 安装包与 macOS DMG。

**性能与交付层面**

- CLI 构建成原生 `ncx.exe`，用户不需要准备 Python、虚拟环境或 editable install。
- CLI release 构建启用 strip、LTO、体积优化，并默认使用桌面安装包对应的 Windows MSVC target；CLI 包
  包含 README、LICENSE 和配置样例。
- 启动路径避开 Python 解释器和 import 开销，适合短的一次性命令，也适合交互 REPL。
- 显式所有权让并行 worker 隔离、结果选择和 promote 更容易推理，不容易出现共享可变状态
  泄漏。
- Rust 工作区与 GUI 测试套件覆盖 provider 与运行时契约、沙箱与审批策略、工具调度与恢复、MCP 热重载、持久后台进程、LSP、原生 PTY 会话、GUI 装配、声明核验以及编排器。

**平台控制面补齐**

- **Task budget（任务预算）：** 每次模型调用都会收到当前运行预算，包括模型调用次数、工具调用次数
  和上下文限制；模型调用或工具调用超预算时，loop 会干净停止，补齐未执行工具调用的
  tool result，并如实报告未完成的校验状态。
- **Context editing（上下文编辑）：** 本地完整 session 不会被删；发给 provider 的是发送时编辑视图，
  会压缩旧 tool result，并在超过上下文预算时丢弃更早的前缀。
- **Tool search（按需工具暴露）：** 工具注册时会进入 catalog。小工具集仍全量暴露；工具变多时只暴露核心
  工具和 `tool_search`，搜索命中的工具会在下一轮 schema 里出现（默认单轮最多暴露 16 个 schema）。
- **Harness 工具族：** Rust 运行时内置真实的 `rust-analyzer` LSP provider、带增量输出的有界后台进程，以及支持原始 stdin、光标读取、尺寸调整和关闭的持久 ConPTY/PTY 终端；Windows 子进程纳入 Job Object 容器管控。
- **Tool recovery（工具恢复）：** 只读工具失败会自动分类，瞬态故障可重试一次，并在不绕过中间件、审批与沙箱强制的前提下自动选择参数兼容的回退工具。
- **结构化工作区巡检：** `list_directory` 和 `path_info` 使用原生文件系统 API，`git_status` 和 `git_diff` 暴露固定的只读 Git 操作，无需模型手写平台相关的 `ls`/`dir`/`find` 命令。
- **Semantic memory（混合语义记忆）：** 项目记忆检索采用混合词法语义排序：关键词、标签、短语、
  Jaccard 相似度、时间新近度，以及一小组 agent/runtime 领域同义词。
- **确定性 Hooks：** `[[hooks]]` 可在匹配工具执行前后及轮次生命周期节点运行项目命令。`pre_tool` 或 `user_prompt` 失败会阻断动作；`post_tool` 和 `stop` 输出会追加到上下文，用于审计、格式化与质量门禁。
- **Checkpoint / restore：** Rust CLI 和 Tauri GUI 都会在模型轮次前创建文件
  checkpoint。CLI 提供 `/checkpoint`、`/checkpoints`、`/restore <id>`；GUI 提供
  checkpoint 面板用于手动保存、列表查看和恢复。
- **Pi 风格会话内建树状链路追踪（In-Conversation Trace Tree）：** 每条会话消息在 `session.jsonl` 内嵌结构化 `_trace`（`id` / `parent_id` 树指针、轮次号、SHA-256 摘要、工作区 Git revision、Assistant 生效模型与单步 Token `usage`、工具墙钟耗时 `duration_ms` 与退出码、以及压缩位点 `first_kept_entry_id`），发往大模型前自动剥离，实现单文件自包含追踪且对模型零额外 Token 污染。
- **OpenCode 风格编辑后自动诊断回写（Post-Edit Auto-Diagnostics）：** `apply_patch` 与 `str_replace_editor` 修改文件后，自动触发增量 `rust-analyzer` / LSP 诊断（自动同步 `textDocument/didChange` 并过滤 Info/Hint）、JSON/TOML 零进程语法校验与 Git 冲突标记残留检测，发现问题立即追加到工具返回结果末尾，省去额外一轮模型排查。
- **Codex 风格超长工具输出自动落盘卸载（Large Tool Output Offloading）：** 当 `shell`、`grep`、MCP 等工具单次输出超过 16,000 字符时，自动将完整未截断输出持久化至系统临时目录，并在内联返回值中给出「前 8,000 字符 + 后 8,000 字符 + `[tool_output_offloaded]` 完整文件路径与起始行号句柄」，供模型通过 `read_file(path, offset, limit)` 按需精读中间省略段落。

### 第二阶段为什么改用 Rust

切到 Rust 是因为第二阶段的目标是产品化，而不只是继续加功能：

- **架构硬化：** 沙箱、审批引擎、provider adapter、工具注册表、记忆存储和编排器都有
  显式类型契约，不再主要依赖 Python 的动态对象边界。
- **动作边界更清楚：** 文件、shell、搜索、记忆操作都经过同一个 tool context，审批和
  沙箱检查贴近真实执行点。
- **并行编排更稳：** 隔离 worker 副本、verifier 选择、结果 promote 回真实工作区这些
  流程，在显式所有权和类型系统下更容易证明不会互相踩写。
- **运行时控制面：** task budget、context editing、tool search、semantic memory 放在
  Rust runtime 边界里，而不是只靠模型提示词约定。
- **原生发布性能：** 小体积 `ncx.exe` 不需要解释器启动和环境配置，一次性 CLI 任务响应
  更直接，Windows 用户拿到包即可运行。
- **桌面打包路径：** Tauri 提供原生 shell + web UI 前端，比继续扩大 Tkinter 原型更适合
  长期发布。

### 第三阶段：智能复利回流与 60 分奇点闭环（当前最新阶段）

第二阶段完成了强类型的原生执行底座，但每次任务在统计意义上仍是“冷启动”的：`TurnResult` 算完 Token 成本即被丢弃，召回的记忆无法归因结果，编排器路由不看历史成功率，用户事后回滚、手改文件或出言纠正等最强负反馈信号也未被捕获。**第三阶段**对照《智能》“智能复利 / 60 分奇点”框架，将 BugleCat 升级为「行动 → 结果（`TaskOutcome`）→ 反馈（`OutcomeFollowup`）→ 学习（记忆调权 + 路由升级 + 复合恢复链沉淀）」的自进化闭环（完整设计方案见 [`docs/intelligence-compounding-feedback-plan.zh-CN.md`](docs/intelligence-compounding-feedback-plan.zh-CN.md)）：

1. **四大核心工程类别与 60 分奇点度量（`G1`）**
   - 在单 Agent 与多智能体编排主控（`{session}:orch-{turn}`）统一出口记录隐私安全的 `TaskOutcome`，为避免样本碎片化，将工程任务收敛为 **4 个核心类别**：
     - `code_fix_test`（逻辑缺陷修复、单测修复与断言补齐）
     - `compile_repair`（编译报错、类型检查与 Lint 修复）
     - `feature_impl`（新功能开发与端到端接线）
     - `doc_refactor`（文档、配置调整与结构重构）
   - 纯问答轮次（`chat`）和子任务分支（`OrchestratorWorker`、`OrchestratorNode`）仅作链路归档，**不计入主线 ACR 分母**；已完成但尚无后续定论信号的任务记为 `pending`，绝不虚增成功率。
   - **60 分奇点工程判据**：当某核心类别在滚动 30 天内已定论样本 `n >= 30`、`ACR-strict` 的 Wilson 95% 置信下界 `>= 60%` 且返工率 `<= 15%` 时，判定该类任务跨过从“人替 AI 兜底”到“AI 对结果负责”的结构性奇点。

2. **零守护进程的多维后续信号归因（`I1–I5` & `L`）**
   - `I1`（显式阻断/失败）：终态为 `NeedsHuman`、`Failed` 或 `Cancelled`。
   - `I2`（显式询问）：触发沙箱审批弹窗或 `ask_user_question`（`ACR-strict` 严格扣减所有 `I2`，`ACR-approval-tolerant` 允许审批通过的轮次）。
   - `I3`（回滚检测）：执行 `/restore <id>` 或 GUI 恢复 Checkpoint 时，自动向该快照之后最近的 `Primary` 任务追加 `Rollback` 负反馈。
   - `I4`（多语言纠正推断）：下一条 `DirectHuman` 消息命中确定性中/英/日纠正词表（或再次触碰上一轮修改过的同一文件）时，记为 `Correction { inferred: true }`。
   - `I5`（**零常驻 `LazyGitUndoDetector`**）：在新会话启动及下一轮主任务（含 GUI 多智能体编排轮次）开始前，对比上一轮记录的 `workspace_revision`（Git `HEAD` SHA）与 `files_touched`，利用 `git merge-base --is-ancestor` 与 `git status --porcelain` 零开销捕获跨会话的 `git reset --hard`、`git revert` 或人工覆写行为。
   - `L`（人工显式标注，最高优先级）：支持 CLI/GUI `/outcome good|bad [note]`、A/B worktree 采纳，以及 **GUI 每条 Assistant 消息底部的 `♡` / `♧` 按钮（按 `turnIndex` 精准绑定到对应历史回合的 `TaskOutcome`）**。

3. **带 14 天 Shadow 基线门禁的在线回流（`G2`：记忆有界调权 & 路由单调升级）**
   - **14 天 Shadow 基线门禁**：`[feedback] memory` 与 `[feedback] routing` 默认均为 `"shadow"`（实际注入与路由行为与 `"off"` 完全一致，仅在 `memory_rank_changed` 与 `shadow_recommended_route` 中记录反事实差异）。通过 `/config` 或设置切换至 `"on"` 时，强制校验 **至少 14 天 Shadow 观测期 + 至少 2 个核心类别样本数 `n >= 30`**（支持 `--force` 显式跳过）。
   - **可归因记忆召回与 Beta 后验有界调权**：`MemoryStore::recall_entries` 将笔记的内容哈希 `note_id` 写入 `TaskOutcome.recalled_note_ids`（多智能体模式下自动合并胜出 Worker 的召回笔记）。任务定论时在 SQLite `memory_feedback` 表中对 `injected / accepted / rejected` 计分一次（若后续收到人工标签可原地翻转计数）。按 Beta 后验 `p = (accepted + 1) / (injected + 2)` 对词法语义检索分施加 **`±20%` 有界乘子**；在 `"on"` 模式下当 `injected >= 8` 且 `p < 0.25` 时自动降级为 `Demoted`（不再召回但保留原行，可通过 `ncx --memory-review restore <id>` 恢复）。
   - **编排器路由单调升级**：`RoutingStats` 按 `(category, initial_complexity, route)` 聚合近 30 天 ACR 与成本中位数。在 `"on"` 模式下，若某类别在 `Simple`（或 `Medium`）管线上的 Wilson 下界 `< 40%`（`n >= 20`）且更高档位能提升下界 `>= 10%`，自动将该类任务单调升级至 `Medium` / `Complex`，绝不自动降级。

4. **复合恢复链沉淀、隔离 Worker 污染防护与频次优先晋升**
   - **多步复合恢复链（`CompoundRecoveryChain`）**：捕获同回合内「工具失败 → 中间诊断/只读排查 → 同一工具修复成功」的完整三元组并生成 `Candidate` 候选记忆。
   - **隔离 Worker 污染防护与路径脱敏**：运行在临时目录（`.buglecat-subruns`）中的子任务 Worker 绝不直接向宿主机写候选记忆；仅当主控最终采纳（`promoted`）胜出 Worker 且回合收敛成功时，才将其临时路径脱敏还原为真实工作区路径并写入 `MemoryIndex`，落败分支随副本一同丢弃。
   - **跨会话复现频次计数（`occurrences`）与双轨晋升**：重复命中的候选记忆自动累加 `occurrences` 并按 `occurrences DESC, created_at DESC` 排序（审阅列表显示 `命中 N 次`）。当 `memory_auto_promote_enabled = true`（默认开启）时，零人工介入且验证通过的高置信度候选会自动晋升写入 `.ncx/memory/LEARNINGS.md`；其余候选由人工通过 `ncx --memory-review confirm <id>` 或 GUI 记忆面板确认。

5. **跨项目历史会话冷启动与 GUI 可视化奇点仪表盘**
   - **多仓库冷启动回填（`ncx --outcomes bootstrap <paths...>`）**：扫描本地多个历史项目的 `.nanocodex/session.jsonl`，自动回填 `_trace` 链路并提取历史 `TaskOutcome` 与 `OutcomeFollowup` 写入全局指标库 `~/.nanocodex/global-outcomes/context.sqlite`。
   - **GUI「ACR & 60 分奇点仪表盘」**：在 Tauri 桌面端右侧 **「记忆」** 面板顶部提供可视化卡片，实时展示近 30 天 `ACR-strict`、面向 60% 奇点线的 Wilson 95% 下界进度条、四大核心类别各自的样本进度（`n/30`）与奇点达标徽章，以及 14 天 Shadow 基线就绪状态。

6. **自进化 Harness 正则化体系（P0–P4：配对复利回放校准、RRSI 离线进化、双层在线记忆与 Propose–Probe–Compose）**
   - **噪声校准与逐题配对评测（[`evals/compounding_replay/analyze.py`](evals/compounding_replay/analyze.py)）**：标定单臂噪声下限（$\delta_{\text{resolved}}$、$\delta_{\text{cost}}$），并对真实仓库长课程（`sqlparse`、`python-tabulate`）执行精确 McNemar 检验与按题配对 Bootstrap 95% 置信区间估计（在 `python-tabulate` 3 轮配对回放中，`cost_per_resolved` 比值 `B/A = 0.7904`，配对 95% CI `[0.6239, 0.9523]`）。
   - **离线进化 RRSI 正则化（`train/forge.py`、`NCX_GENOME`）**：将 `NCX_GENOME` 扩展至 `system_prompt`、`tool_descriptions`、`orchestrator_prompts`（5 个编排阶段提示词）与有界 `memory_config`（`max_entries`、`max_chars`），并配套按轮退火编辑预算、规则版反作弊审查器（`critic_check`）、证伪假设账本（`ledger_<stamp>.jsonl`）、连续停滞转向、双通道成本感知验收门与盘后结构化消融剪枝（`--prune`）。
   - **Harness² 双层在线记忆与负向回归护栏（`MemoryStore::promote_module_invariant_if_due`、`record_regression_guardrail`）**：区分单题情景层（每模块最多保留 `MAX_EPISODIC_PER_MODULE = 2` 条）、模块级不变量层（`[Module Invariant: <module_path>]`，`+6` 结构性召回加分，按规范化头部哈希稳定保留 `note_id` 与反馈权重）与负向回归护栏（`[Regression Guardrail: <module_path>]`，在发生 `pass_to_pass` 回归或 `Rollback` 时自动写入），配合高消耗轮次搭便车笔记精准打折（`record_unhelpful_injection`）。
   - **Harness² 任务内 `Propose–Probe–Compose`（[`orchestrator.rs`](rust/crates/ncx-core/src/orchestrator.rs)、[`workspace_promotion.rs`](rust/crates/ncx-core/src/workspace_promotion.rs)）**：支持 Verifier 给出 `COMPOSE:i,j` 时的多 Worker 非冲突文件级组合（`compose_with_paths`）与失败重试轮次的跨 Worker 探测结论合成（`build_compose_feedback`）。

## 目录

- [项目阶段](#项目阶段)
- [亮点](#亮点)
- [架构](#架构)
- [工具](#工具)
- [安装](#安装)
- [快速开始](#快速开始)
- [配置](#配置)
- [自定义 Slash Commands](#自定义-slash-commands)
- [本地模型 / OpenAI 兼容接口](#本地模型--openai-兼容接口)
- [沙箱与审批](#沙箱与审批)
- [MCP](#mcp)
- [插件生态](#插件生态)
- [Skills](#skills)
- [记忆与 AGENTS.md](#记忆与-agentsmd)
- [会话、恢复与历史](#会话恢复与历史)
- [上下文压缩](#上下文压缩)
- [Token 用量与成本](#token-用量与成本)
- [定时器](#定时器)
- [A/B worktree 对比](#ab-worktree-对比)
- [BugleCat 桌面端](#buglecat-桌面端)
- [测试](#测试)
- [Release 打包](#release-打包)
- [安全说明](#安全说明)

## 亮点

- **Codex 风格 agent 循环** —— 流式 token 输出、多轮工具调用、可取消、按轮统计
  用量。
- **DeepSeek + 任意 OpenAI 兼容后端** —— 把 `base_url` 指向托管 API 或本地服务
  （vLLM、llama-server、LM Studio……）。
- **沙箱与审批状态机** —— 三种沙箱模式、四种审批策略，拦截每一次文件/shell/网络
  动作。
- **MCP 集成 + 市场** —— 从 `mcp.toml` 加载服务，或从内置 / 远程目录一键安装；
  工具保留服务端提供的名称，重名会被拒绝；`/mcp reload` 支持进程内原子热重载外部工具集。
- **插件生态** —— 可从本地、Git、NPM 或社区市场安装兼容 Codex 的资源插件，把
  Skills、MCP、Apps、Hooks 和 UI 插槽组合发布，并在安装前检查兼容性和能力冲突。
- **可插拔 Agent 运行时** —— 模型 Provider、轮次上下文、工具注册表、调度器、中间件与权限配置均为显式运行时接缝，CLI 与 GUI 共享同一套装配契约。
- **Harness 工具族** —— 真实 LSP 查询、有界后台进程、持久原生 PTY 终端会话、结构化工作区巡检、Web 工具与会话检索。
- **Skills 系统** —— 用户 skill 加三个内置编码 skill；只注入名称 + 描述，正文
  按需加载。
- **自定义 slash commands** —— 项目/用户级 Markdown prompt 模板放在
  `.nanocodex/commands`，并兼容 `.claude/commands`。
- **持久记忆 + AGENTS.md / CLAUDE.md** —— 每轮注入的持久笔记和分层项目指令。
- **跨 Agent 长期记忆插件** —— 随仓库提供的 LLM Wiki 示例用自动触发 Skill 配合
  本地 MCP，在 BugleCat 与其他 Agent 客户端之间召回用户偏好和项目快照。
- **可浏览的会话历史** —— JSONL 日志、完整对话快照、恢复（resume）和分叉（fork）。
- **上下文压缩** —— 零成本的确定性摘要，或可选的模型摘要，按 token 预算触发。
- **缓存感知的成本统计** —— 用真实的按调用用量，按 DeepSeek 的命中/未命中费率
  计价。
- **自适应推理强度** —— `auto` 档根据请求选 `max`/`high`/`low`（多语言关键词表：
  英文 / 中文 / 日本語）。
- **定时器** —— 一次性/周期性保存的 prompt，连续失败自动禁用。
- **A/B worktree 对比** —— 同一 prompt 在两个隔离的 git worktree 里各跑一套配置，
  对比 diff/成本/延迟，采纳其中一侧。
- **prompt 增强、图片输入、中文优先回复**，以及基于 **Tauri 2 + Svelte 5** 的跨平台桌面客户端。

## 架构

```text
rust/
├── crates/
│   ├── ncx-config/        # 分层配置解析与运行时限额
│   ├── ncx-provider/      # 可替换的 OpenAI 兼容 / Anthropic 模型边界与诊断脱敏
│   ├── ncx-sandbox/       # 沙箱策略与四档审批状态机
│   ├── ncx-tools/         # 进程执行器（Windows Job Object）、后台任务、PTY、文件与补丁工具
│   ├── ncx-mcp/           # MCP 传输层、协议版本校验与外部工具热重载
│   ├── ncx-context/       # 关联记忆与上下文检索
│   ├── ncx-thread-store/  # 持久化会话与事件存储
│   ├── ncx-protocol/      # GUI / App Server 强类型协议契约（唯一事实源）
│   ├── ncx-app-server/    # 桌面端与宿主服务桥接层
│   ├── ncx-core/          # AgentLoop、ToolRegistry、中间件、声明核验（verification）与编排器
│   └── ncx-cli/           # 一次性与交互式命令行前端（ncx.exe）
└── gui/
    ├── src/               # Svelte 5 桌面 UI
    └── src-tauri/         # Tauri v2 原生宿主命令与共享运行时装配

nanocodex/                 # 第一阶段 Python 参考与兼容原型实现
```

## 工具

Rust `ToolRegistry` 每轮向模型暴露与当前任务相关的工具子集：核心工具常驻可见，命名匹配与 `tool_search` 按需挂载专用工具，避免每轮发送完整工具目录。

| 工具 | 用途 |
| --- | --- |
| `shell` | 执行 shell 命令，受沙箱/审批策略约束。 |
| `apply_patch` | 应用 Codex 风格补丁，创建/编辑/删除文件。 |
| `update_plan` | 为多步任务维护一个可见的步骤计划。 |
| `read_file` | 读取工作区里的文件（或指定行区间）。 |
| `str_replace_editor` | 执行精确的、受审批门禁保护的文本替换编辑。 |
| `grep`、`grep_literal`、`glob` | 无需 shell 解析即可搜索源码内容与文件路径。 |
| `list_directory`、`path_info`、`git_status`、`git_diff` | 结构化工作区文件与只读 Git 状态巡检。 |
| `web_search`、`web_fetch` | 在配置的网络策略下执行网页搜索与抓取。 |
| `lsp` | 查询符号、定义、引用、悬停文档（hover）与编译诊断。 |
| `background_start/poll/stop/list` | 管理带增量输出的有界后台长进程。 |
| `terminal_open/write/read/exec/resize/close/list` | 管理持久化的原生 ConPTY/PTY 终端会话。 |
| `ask_user_question` | 当前端提供处理器时，暂停并弹出 GUI/CLI 多选或自由输入交互问答。 |
| `session_search`、`session_trace`、`session_event_read/search/trace` | 检索历史会话快照与脱敏事件轨迹。 |
| `skill` | 按需加载已发现的 Skill 正文。 |
| `remember` | 往用户记忆里追加一条持久笔记。 |
| `<tool>`（MCP） | 已连接 MCP 服务暴露的工具，使用服务端提供的名称。 |

## 安装

### 方式一：直接下载预编译包（推荐）

前往 [GitHub Releases (`v0.1.2`)](https://github.com/dgy-github/BugleCat/releases/latest) 直接下载：

- **Windows 安装包（NSIS）**：`BugleCat_0.1.0_x64-setup.exe`
- **Windows 完整便携运行包（含 `BugleCat.exe`、`ncx.exe` 与离线核验工具）**：`buglecat-20261014-final.zip`
- **macOS 安装包**：`BugleCat-arm64.dmg`（Apple Silicon） / `BugleCat-x64.dmg`（Intel）

### 方式二：从源码构建

当前 Rust release 线：

```powershell
cd path\to\BugleCat
rustup toolchain install stable-x86_64-pc-windows-msvc
cargo +stable-x86_64-pc-windows-msvc build --manifest-path rust\Cargo.toml -p ncx-cli --release

cd rust\gui
npm ci
npm run tauri:build
```

这需要 Rust toolchain 和 Node.js 18 或更新版本。原 Python 实现仍可用于参考和原型：

```powershell
cd path\to\BugleCat
python -m pip install -e ".[dev]"
```

需要 Python ≥ 3.11。

## 快速开始

Rust CLI，当前 release 线：

```powershell
cd rust
cargo run -p ncx-cli -- "summarize this repository"
cargo run -p ncx-cli
cargo run -p ncx-cli -- --resume
cargo run -p ncx-cli -- --history
cargo run -p ncx-cli -- --memory-merge
cargo run -p ncx-cli -- --outcomes report --since 30d --by category,model,policy
cargo run -p ncx-cli -- --outcomes bootstrap D:\repo1 D:\repo2  # 扫描多个仓库历史会话冷启动提取全局 ACR 指标
cargo run -p ncx-cli -- --memory-review                         # list | confirm <id> | reject <id> | restore <id>

# Tauri 桌面开发版
cd gui
npm ci
npm.cmd run tauri -- dev --target x86_64-pc-windows-msvc
```

Rust REPL 里可以用 `/config` 查看解析后的配置文件路径、当前 model/sandbox/approval
值和可写 key。`/config key=value` 会把设置持久写入 `~/.nanocodex/config.toml`（将 `feedback.memory=on` 或 `feedback.routing=on` 开启时会校验 14 天 Shadow 基线门禁，加 `--force` 可强制跳过）；
provider、model、sandbox 或预算类变更需要重启 REPL 后影响当前会话。`/usage`（或
`/cost`）会显示上一轮和当前 REPL session 的原始 token 用量；`/outcome good|bad [note]` 可对上一轮主任务打人工标签，`/outcome report` 查看当前工作区 ACR 与 60 分奇点报告。

旧版 Python CLI（为兼容已有用户，命令名仍保留 `nanocodex`）：

```powershell
# 一次性任务
nanocodex "add a --json flag to the CLI"

# 在当前目录交互
nanocodex --cd .

# 启用 MCP 服务
nanocodex --mcp

# 启动 GUI
nanocodex-gui --cd .
```

旧版 Python GUI 在 Windows 上仍可双击 `nanocodex-gui.cmd`，或用
`scripts/make-shortcut.ps1` 生成开始菜单快捷方式。

## 配置

配置项按优先级解析：

```text
CLI 参数 > 环境变量 > ~/.nanocodex/config.toml > ~/.deepseek/config.toml > ~/.codex/config.toml > 默认值
```

真实 API key 应当留在仓库之外：

```powershell
$env:DEEPSEEK_API_KEY = "sk-..."
$env:NANOCODEX_API_KEY = "sk-..."
```

或创建 `~/.nanocodex/config.toml`：

```toml
api_key = "sk-..."
base_url = "https://api.deepseek.com/v1"
model = "deepseek-chat"

sandbox_mode = "workspace-write"   # read-only | workspace-write | danger-full-access
approval_policy = "on-request"     # untrusted | on-failure | on-request | never
reasoning_effort = "auto"          # auto | low | high | max | off
memory_auto_promote_enabled = true # 零人工介入且验证通过的高置信度恢复经验自动晋升

# 智能复利在线回流开关（off | shadow | on，默认 shadow）
# 切换至 "on" 需满足 >= 14 天 Shadow 观测数据且 >= 2 个核心类别样本数 n >= 30
[feedback]
memory = "shadow"
routing = "shadow"

# 可选
# context_token_budget = 512000
# context_window = 1048576
# max_iterations = 60
# max_tool_calls = 120
# max_parallel_tool_calls = 8  # 有效范围：1..=128；仅对只读工具生效
# context_edit_enabled = true
# context_edit_max_chars = 120000
# context_edit_keep_recent_messages = 30
# context_edit_max_tool_result_chars = 4000
# available_models = ["deepseek-chat", "deepseek-reasoner", "deepseek-v4-pro"]

# [[hooks]]
# event = "pre_tool"          # pre_tool | post_tool | user_prompt | stop
# matcher = "shell|apply_patch"
# command = "echo checking %NCX_HOOK_TOOL%"
# timeout_s = 10
```

运行时控制面参数也可通过环境变量设置：`NANOCODEX_MAX_ITERATIONS`、`NANOCODEX_MAX_TOOL_CALLS`、`NANOCODEX_CONTEXT_EDIT_ENABLED`、`NANOCODEX_CONTEXT_EDIT_MAX_CHARS`、`NANOCODEX_CONTEXT_EDIT_KEEP_RECENT` 与 `NANOCODEX_CONTEXT_EDIT_TOOL_RESULT_CHARS`。Rust CLI 同时支持 `--max-iterations`、`--max-tool-calls`、`--context-edit-max-chars`、`--context-edit-keep-recent`、`--context-edit-tool-result-chars` 和 `--disable-context-edit`。

完整示例见 `config.example.toml`。

Hooks 会在环境变量中收到 `NCX_HOOK_EVENT`、`NCX_HOOK_TOOL`、
`NCX_HOOK_ARGS`、`NCX_HOOK_RESULT` 和 `NCX_HOOK_WORKSPACE`。`pre_tool`
适合做确定性的风险拦截，例如阻止危险 shell 命令；`post_tool` 适合做审计和格式化；
`user_prompt` 可以在模型看到 prompt 前阻断或追加系统说明；`stop` 适合做轮次结束质量门
或通知。`UserPromptSubmit`、`Stop`、`PreToolUse`、`PostToolUse` 这类 Claude 风格
事件名会自动归一化。Hooks 会作为本地子进程运行，只配置你信任的命令。

## 自定义 Slash Commands

Rust REPL 可以把 Markdown prompt 模板变成 slash command。项目级命令放在
`.nanocodex/commands/<name>.md`；为了兼容 Claude Code，也会读取
`.claude/commands/<name>.md`。用户级命令放在 `~/.nanocodex/commands/<name>.md`，
并兼容 `~/.claude/commands/<name>.md`。

```markdown
---
description: Review one file
---
Review `$ARGUMENTS[0]` for bugs, regressions, and missing tests.
```

REPL 中可以这样调用：

```text
/review rust/crates/ncx-core/src/session.rs
/project:review rust/crates/ncx-core/src/session.rs
/user:review rust/crates/ncx-core/src/session.rs
```

`/name` 会优先解析项目级命令，再解析用户级命令。模板支持 `$ARGUMENTS` 表示原始参数串，
也支持 `$0`..`$9` 和 `$ARGUMENTS[0]`..`$ARGUMENTS[9]` 作为简单位置参数。模板里没有
占位符时，原始参数会自动追加到 `Arguments:` 块下。这类命令只会展开成普通用户 prompt，
不会自己运行本地 shell 代码。

## 本地模型 / OpenAI 兼容接口

BugleCat 走标准的 `/v1/chat/completions`，所以任何 OpenAI 兼容服务都能用——
vLLM、llama-server、LM Studio、Ollama 的 OpenAI 兼容层等等。把 `base_url`
指向该服务的 `/v1` 根路径即可。多数本地服务会忽略 API key，但仍需一个非空占位
值，因为 OpenAI SDK 需要它。

```toml
api_key = "local-dev-key"
base_url = "http://127.0.0.1:8005/v1"
model = "Qwen3.6-27B-Q4_K_M"
```

快速连通性检查：

```powershell
curl http://127.0.0.1:8005/v1/models
```

流式有一个有界的「响应头」超时（默认 45 秒，可用
`NANOCODEX_STREAM_OPEN_TIMEOUT_S` 覆盖），这样卡住的本地服务会带清晰提示快速
失败，而不是把 UI 一直挂住。

## 沙箱与审批

两个正交维度拦截每一次动作，对齐 Codex：

**沙箱模式** —— 物理上允许什么：

| 模式 | 读 | 写 | 网络 |
| --- | --- | --- | --- |
| `read-only` | 任意 | 无 | 关 |
| `workspace-write` | 任意 | 工作区 + 可写根 + 临时区 | 默认关，可显式开 |
| `danger-full-access` | 任意 | 任意 | 开 |

**审批策略** —— 动作超出沙箱时怎么办：`untrusted`、`on-failure`、`on-request`、
`never`。审批引擎把每次越权解析为 `ASK` / `AUTO_APPROVE` / `AUTO_DENY`。

在 Windows 上强制是**策略级**的：路径检查和可写根拦截发生在工具边界，不是内核级
隔离。

## MCP

MCP 服务是可选的（opt-in），并且运行在沙箱**之外**（它们会启动外部子进程）。在
`~/.nanocodex/mcp.toml` 里配置：

```toml
[mcp_servers.fetch]
command = "uvx"
args = ["mcp-server-fetch"]

[mcp_servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "D:\\projects"]
```

然后以启用 MCP 的方式启动：

```powershell
nanocodex --mcp
```

Rust `ncx` REPL 可以不重启而应用 MCP 配置变更：

```text
/mcp reload
```

重载会先准备每个已配置服务；某个服务连接失败时只跳过它，已成功准备的服务仍会原子替换
当前工具集。若所有已配置服务都失败，或发现工具重名，则保留原有工具集；空配置会移除全部
活动 MCP 工具。

每个已连接服务都有一个后台 stdio reader，按 JSON-RPC request ID 路由响应，因此独立的
只读调用可以乱序完成，也不会把响应配错到另一个工具调用；EOF、写入错误和请求超时都会
唤醒并清理对应的 pending 请求。同一服务共享一个有界调用闸门：最多 4 个明确声明为只读
的调用同时在途；副作用或未知调用会占满全部 permit，与其他调用互斥。闸门按服务隔离，
慢服务不会串行化其他服务；副作用调用仍会先经过审批，再取得闸门。

初始化握手会在注册工具前校验服务端明确返回的 MCP 协议版本。为兼容较旧的轻量服务，
省略该字段仍可接受；明确不兼容或格式错误的版本只会让当前服务启动失败，不影响其他服务。

每个服务的工具保留服务端提供的名称；原子重载会拒绝工具重名，因此应为 MCP 工具选择
互不冲突的名称。一个**市场**支持从内置精选
目录或远程目录（`NANOCODEX_MARKETPLACE_URL`）一键安装；每个条目都走和手动添加
服务相同的名称校验与去重，远程目录被当作不可信数据处理。更多见
`mcp.example.toml`。

## 插件生态

BugleCat 将两类插件明确隔离：

- **Codex 兼容资源插件**使用 `.codex-plugin/plugin.json`，可以组合 Skills、MCP、
  Apps、Hooks 和界面扩展。
- **外部进程插件**使用 `plugin.toml`，通过进程边界通信。BugleCat 不会把第三方
  DLL、SO 或 DYLIB 动态库直接加载进应用进程。

资源插件可以采用以下结构：

```text
my-plugin/
├── .codex-plugin/plugin.json
├── skills/<name>/SKILL.md
├── .mcp.json
├── .app.json
└── hooks/hooks.json
```

插件可以安装到用户全局目录 `~/.ncx/codex-plugins`，也可以安装到工作区目录
`.ncx/codex-plugins`；同 ID 的工作区插件会覆盖全局插件。市场发现兼容 OpenAI
Codex、Claude 和 Cursor 的目录结构，也支持 dshfind、DeepSeek 1024 Store 等 DSH
Community 来源与标准 HTTPS 目录。安装来源可以是本地路径、Git 仓库或 NPM 包；候选
插件会先进入隔离暂存区，完成校验后才安装。

运行时发现会隔离可选资源：损坏或暂时不可读的插件目录、Hooks 文档、Apps 文档或单个
App 会记录诊断并跳过，不会遮蔽其他插件的合法资源。显式的插件目录管理命令仍保持严格
错误，便于维护者定位并修复清单，而不是静默改写错误配置。

设置页会在安装前展示插件请求的能力和兼容性。如果已启用插件占用了重复能力，新安装
会被阻断，不会静默替换。界面插件可使用 `settings.plugins.tab`、
`sidebar.footer.action` 和 `shell.overlay` 三个 UI 插槽；Harness 运行诊断会展示当前
实际挂载的插件能力。

MCP 和外部附件解析插件默认关闭，因为它们可能启动子进程或把内容发送到其他服务。只
启用可信插件和市场来源。可参考
[`local-plugins/llmwiki-memory`](local-plugins/llmwiki-memory)，它在同一个插件中同时
提供 Skill 与 MCP 服务。

## Skills

Skills 是可复用的指令文档，每个一个目录：

```text
~/.nanocodex/skills/<skill-name>/SKILL.md
```

只有每个 skill 的**名称和描述**会被注入系统提示；完整正文按需读取，所以一个庞大
的库不会吃掉上下文窗口。模型也可以通过 `manage_skills` 工具在对话里创建/读取/
删除用户 skill。

最小 skill：

```markdown
---
name: code-review
description: Review code changes and focus on bugs, regressions, and missing tests.
---

# Code Review

Look for behavior regressions first, then missing tests, then maintainability.
```

包内自带三个**只读内置 skill**，位于 `nanocodex/builtin_skills/`：

- **code-review** —— 两遍审查（先正确性，再清理），按影响排序。
- **debug** —— 复现 → 定位 → 修复 → 验证；克制住「补第一处看似合理的行」的冲动。
- **write-tests** —— 测可观察行为，一个测试一个行为，优先纯函数而非 mock。

同名的用户 skill 会遮蔽内置的。

## 记忆与 AGENTS.md

三层互补的持久上下文：

- **用户记忆**（`~/.nanocodex/memory.md`）—— 持久的个人事实和偏好。由 `remember`
  工具写入、在 GUI 输入框里打 `# 内容` 快速捕获、或手工编辑。包在 `<user_memory>`
  块里。
- **项目记忆与智能复利生命周期**（`.ncx/memory/LEARNINGS.md` + `memory_index.sqlite`）——
  受在线反馈闭环管控的项目级经验：
  - **候选记忆（`Candidate`）**：自动捕获同回合多步复合恢复链（`CompoundRecoveryChain`），记录跨会话复现频次（`occurrences`，按 `occurrences DESC, created_at DESC` 排序并展示 `命中 N 次`）；隔离 Worker 的候选仅在胜出分支被主控采纳（`promoted`）并完成临时路径脱敏后才入库。当 `memory_auto_promote_enabled = true` 时，零介入且验证通过的高置信度候选会自动晋升写入 `LEARNINGS.md`，其余由 `ncx --memory-review confirm <id>` 或 GUI 面板确认。
  - **生效记忆（`Active`）**：混合词法语义排序召回，通过内容哈希 `note_id` 记入 `TaskOutcome.recalled_note_ids`，并按任务定论后的 Beta 后验得分 `p = (accepted + 1) / (injected + 2)` 进行 **`±20%` 有界调权**。
  - **降级记忆（`Demoted`）**：在 `[feedback] memory = "on"` 模式下，当 `injected >= 8` 且 `p < 0.25` 时自动降级（暂停召回但保留原行），可通过 `ncx --memory-review restore <id>` 或 GUI 一键恢复。
- **AGENTS.md / CLAUDE.md** —— 项目指令，从 `~/.codex/AGENTS.md` 和
  `~/.claude/CLAUDE.md` 开始，再分层读取从仓库根到工作区的每个 `AGENTS.md`、
  `CLAUDE.md` 和 `.claude/CLAUDE.md`，所以嵌套目录可以细化父级。总大小有上限，避免
  一个超大文件撑爆上下文。Rust CLI、orchestrator worker 和 Tauri GUI 都会在会话启动时注入这一块。

记忆讲「谁/什么」（偏好、事实、经验证的项目经验）；skills 讲「怎么做 X」；AGENTS.md / CLAUDE.md 是项目级指引。

### LLM Wiki 跨 Agent 长期记忆

[`local-plugins/llmwiki-memory`](local-plugins/llmwiki-memory) 是一个可选的本地插件，
用于提供更结构化的记忆生命周期。它把 `always_apply` Skill 与单一本地 MCP 工具组合
起来，通过可配置的本地知识库目录（见插件内 `.mcp.json`）作为实际记忆存储。

安装并启用插件及其 MCP 服务后：

1. 每个新 Agent 会话会在处理第一个业务请求前自动召回用户画像。
2. 进入已有项目时只召回简短的 L1 项目快照；需要历史细节时才按需读取分片，避免占满
   上下文窗口。
3. MCP 存活期间会增量采集 BugleCat 已写盘的新会话到 L0 语料，通常最迟约一分钟，
   不需要新建对话或手工导入。
4. 任务交接时可记录已确认的项目决策、进度、待办和验证证据，供后续 Agent 召回。

各层采用不同的信任规则：L0 是采集到的会话语料；L1 是带来源引用的项目短记忆；L2
是稳定用户画像。新对话内容不会直接修改 L2；画像候选必须满足证据阈值，并在用户明确
批准前保持待审核。Token、Cookie、密码、Secret、数据库口令和敏感业务正文不得写入。

长期记忆只用于恢复方向，**不是代码事实源**。Agent 修改项目之前仍须重新核对仓库需求、
契约、当前文件、Git diff、测试和数据库状态。如果 MCP 暂不可用，当前任务会继续并明确
提示，不会阻塞会话，也不会静默改成直接写 Wiki 文件。

## 会话、恢复与历史

- 每个对话都追加进一个 **JSONL 会话日志**（base64 图片数据会从日志里抹掉以保持
  精简）。
- 一个**全局索引**（`~/.nanocodex/sessions.jsonl`）每个对话存一行摘要，最新在前，
  供 GUI 的历史列表使用。
- 一个**单会话快照**（`~/.nanocodex/snapshots/<id>.json`）冻结完整对话，所以详情
  视图回放的是真实对话，而非摘要。
- Rust CLI 支持 `--resume`，启动前读回工作区 `.nanocodex/session.jsonl`；
  也支持 `--history` 列出最近的全局会话摘要。Tauri 后端每轮 GUI 对话结束后会记录
  同一套 snapshot。
- Rust CLI 和 Tauri GUI 还会在每个模型轮次前保存工作区文件 checkpoint。CLI 用
  `/checkpoints` 查看，`/checkpoint <label>` 手动创建，`/restore <id>` 恢复文件；
  GUI 则在 checkpoint 面板里提供同一套保存 / 列表 / 恢复流程。恢复前会先给当前
  状态创建一个 safety checkpoint。
- 原 Python GUI 可以从保存的 snapshot **分叉（fork）**一条历史会话，且不会修改源会话。

## 上下文压缩

长对话会被折叠以保持在 token 预算内，同时保留系统消息和最近一段尾部（尾部总是从
一条 `user` 消息开始，所以不会切断 tool-call/result 对）。两种策略共用一个接口：

- **deterministic（默认，零 API 成本）** —— 被折叠的中段变成事实性的、基于规则的
  摘要。
- **summarizer（可选，消耗 token）** —— 一次模型调用把中段写成散文。

触发估算用偏中文的 chars/token 比例，所以中文为主的对话不会压缩得太晚。

Rust CLI 里的 `/compact` 会把当前 context-edit 策略物化到 live session，并重写工作区
session 日志；后续对话和 `--resume` 都会从压缩后的历史继续。

## Token 用量与成本

provider 每次调用返回真实 `usage`，包含 DeepSeek 的缓存命中/未命中拆分。Rust REPL
里的 `/usage` 和 `/cost` 会显示上一轮和 session 累计的模型调用数、工具调用数、输入
token、输出 token、缓存命中/未命中 token。Rust 命令刻意只展示原始用量；Python 线的
`pricing.py` 会把 usage 折算成美元成本：

- **缓存感知** —— 一个缓存命中的输入 token 比未命中便宜约 120×；各按自己的费率
  计价。拆分缺失时，整段 prompt 按未命中费率计，所以成本永不低估。
- **对陈旧诚实** —— 价格是一份硬编码快照，带来源和「截至日期」；未知模型返回
  「成本未知」而不是一个错误数字。

## 定时器

把一个 prompt 存起来自动运行——一次性在某个未来时间，或按间隔周期运行：

```powershell
nanocodex schedule add "run the tests" --at 2026-06-08T09:00:00
nanocodex schedule add "summarize new issues" --every 3600
nanocodex schedule list
nanocodex schedule run        # 让它一直跑，任务才会触发
```

连续失败 5 次的任务会**自动禁用**（成功会重置计数器；重新启用会清零），所以一个
坏掉的任务不会永远循环。模型也可以通过 `manage_schedule` 在对话里管理任务。

## A/B worktree 对比

用**两套配置跑同一个 prompt**并对比结果，且不冒着破坏工作树的风险。每一侧都在自己
隔离的 **git worktree** 里跑，所以真实的 `shell`/`apply_patch` 改动永不冲突：

1. 选两套配置（model / 推理强度 / 沙箱 / 审批）。
2. BugleCat 从干净的 `HEAD` 建两个 worktree，串行地在每个里跑这个 prompt，
   审批 auto 放行但范围锁在 worktree 内。
3. 你拿到并排对比：diff、token 成本、延迟、迭代步数、停止原因。
4. **采纳**其中一侧（它的 diff 被应用到真实工作区），或两侧都丢弃；worktree 总会
   被清理。

要求工作区是干净的 git 仓库（无未提交改动），否则入口禁用。

## BugleCat 桌面端

BugleCat 当前桌面端使用 Tauri 2 + Svelte 5（`rust/gui`）：

- 流式对话，推理/答案分离，带 Stop 按钮，且每条 Assistant 消息底部支持**按轮次精准绑定的 `♡` / `♧` 评价按钮**（直接归因到对应轮次的 `TaskOutcome`）。
- 右侧「记忆」面板内置可视化 **「ACR & 60 分奇点仪表盘」**：实时展示近 30 天 `ACR-strict`、面向 60% 奇点线的 Wilson 95% 下界进度条、四大核心任务类别的样本进度（`n/30`）与奇点达标徽章、14 天 Shadow 基线就绪状态，以及带复现频次（`命中 N 次`）的候选/降级记忆审阅操作（采纳 / 拒绝 / 恢复）。
- 工作区切换、运行中模型切换、`/outcome good|bad|report` 快捷指令，以及多分区设置中心（含高置信度恢复经验自动晋升开关 `memory_auto_promote_enabled`）。
- 可浏览的会话历史（点击回放完整对话）。
- 文件、Git diff、分支、项目记忆和 checkpoint 面板。
- 图片附件、Skills、MCP、插件、权限模式和模型商配置。
- 可移动侧栏、审批弹窗及真实的 `ask_user_question` 交互。

CLI 与桌面端共用 Provider、权限、预算、上下文编辑、并发和 Harness 装配路径。

## 测试

### 证据型评测 Harness

评测脚本位于 evals/，分数来自隐藏测试、工具执行结果、验证事件和持久状态
快照，不接受模型自报分：

    # 回放 bench/tasks 中的编码任务（默认每任务 240s；可显式覆盖）
    python -m evals.harness benchmark --agent nanocodex --repeats 3 --timeout 240

    # 直接运行 benchmark；报告同时写入 Markdown/JSON 和逐 attempt 的 post-edit trace JSONL
    python bench/run.py --agent nanocodex --repeats 1 --timeout 240

    # 统计一份 JSONL trace（工具、验证、恢复、步骤、token、人工接管）
    python -m evals.harness trace .\trace.jsonl --output .\evals\reports\trace.json

    # 直接分析真实会话日志
    python -m evals.harness session .\.nanocodex\session.jsonl

    # 比较压缩前后的 Goal/Constraint/Task/Artifact 状态
    python -m evals.harness compaction before.json after.json

    # 生成可重复的 50 个 compaction 样本；--mutate 注入已知丢失/重复回归
    python -m evals.harness generate --count 50 --mutate --output .\evals\generated

报告 schema 为 buglecat.eval.v1。空证据返回 null，不会伪造 0% 或 100%；缺少状态字段
按失败处理。Trace 事件格式与扩展方式见 evals/README.md。
`bench/run.py` 的报告还会记录目标文件快照差异、缺失文件、Python 编译结果、timeout、
脱敏后的 agent 输出尾部和 grader 失败轨迹；post-edit 检查默认单文件 15s，可用
`--post-edit-timeout` 调整。

Windows 下也可以直接双击 scripts/run-eval.cmd：它会自动构建 Rust Agent、回放
benchmark、生成 50 个 compaction 样本并输出报告。已有二进制时可使用
scripts/run-eval.ps1 -SkipBuild；追加 -OpenReport 会自动打开 JSON 报告。

Rust release 线：

```powershell
cd rust
cargo test --workspace --target x86_64-pc-windows-msvc
```

Python 线：

```powershell
python -m pytest -q
```

两套测试都完全离线：mock 过的 provider、可注入的 I/O，不需要真实 API key 或网络
请求。

### 声明级矛盾与 Agent 工具漂移核验（Host Verification）

BugleCat 内置基于宿主操作系统的确定性规则核验层（当前策略版本 `host-rules-v3.4`，生产默认 `off`），用于检测模型在长上下文中脱离实际工具结果的夸大汇报或范围漂移：

```powershell
# 执行 175 例全量中英双语声明级矛盾与漂移评测
cargo run --manifest-path rust/Cargo.toml --example eval_claim_drift -- evals/verification/claim_drift_dataset_v1.json reports/claim_drift_eval_report_v1.json

# 重新生成格式化 Markdown 评测报告
python scripts/format_claim_drift_report.py

# 独立离线审计实际会话中的 .verification.jsonl 日志（支持 CI --strict 门禁）
python scripts/replay_verification.py path/to/.verification.jsonl --strict

# 离线审计声明核验协议与独立样本池配额（零网络调用）
python scripts/audit_claim_source_pool.py
```

评测覆盖 11 类规则分支，使用 175 例预先构造声明与证据的合成输入。v2 指标对检出和误拦统一统计 `NeedEvidence`、`Contradiction`，`Unknown` 不算检出。结果属于规则层回归，不属于端到端或独立生产认证。详见 [报告](reports/claim_drift_eval_report_v1.md)。

### 可靠性回归门禁与本地开发

回归套件（含 Golden `unit` 层 24 项与 `cargo` 层 11 项门禁）重点守护以下工程边界：

- **MCP 权限与传输隔离**：只读分类仅在完整声明 `readOnlyHint=true` 且 `destructiveHint=false` 时生效（缺失或冲突一律 fail-closed 进入审批）；同一服务最多允许 4 个只读调用并发，副作用调用独占闸门；协议版本不匹配或单个插件/server 损坏只跳过当前项，不影响其他合法服务。
- **Provider 诊断脱敏**：OpenAI 兼容与 Anthropic Provider 的非 2xx 响应仅暴露 `HTTP <status>`，绝不把远端错误正文或 API 凭据写入模型上下文、会话日志或诊断文件。
- **前后端强类型协议契约**：`ncx-protocol::ClientRequest` 是 GUI App Server 方法契约的唯一事实源，`scripts/check-protocol-version.mjs` 自动同步协议版本与全部 70 个方法名，构建前强制执行漂移检查与 TypeScript 类型检查。
- **工作区与会话状态一致性**：会话 Resume/Fork/新建、权限模式切换、Memory 整理与 Forge 任务均绑定工作区快照与精确 `threadId` / generation，防止跨工作区串写或残留状态泄漏；持久 Goal 状态转换与自动续轮共享同一互斥边界。

推荐在提交前运行完整本地门禁（无需真实 API Key，零付费调用）：

```powershell
python scripts/check_rustfmt.py
cargo clippy --manifest-path rust\Cargo.toml --workspace --all-targets --all-features -- -D warnings
cargo clippy --manifest-path rust\gui\src-tauri\Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path rust\Cargo.toml --workspace --lib
cargo test --manifest-path rust\gui\src-tauri\Cargo.toml --lib
npm.cmd --prefix rust\gui run test:protocol
npm.cmd --prefix rust\gui run protocol:check
npm.cmd --prefix rust\gui run typecheck
npm.cmd --prefix rust\gui run build
python -m evals.golden.runner --tier unit --scope-to-tiers
python -m evals.golden.runner --tier cargo --scope-to-tiers
python -m pytest -q
python -m ruff check .
```

本地启动 Tauri 开发版：

```powershell
cd rust\gui
npm.cmd ci
npm.cmd run tauri -- dev --target x86_64-pc-windows-msvc
```

开发前端默认监听 `http://localhost:5179/`。如果该端口已有 BugleCat 实例，优先复用并等待热更新，不要重复启动第二个实例。

修改 `ncx-protocol::ClientRequest` 或 `PROTOCOL_VERSION` 后，先重新生成并检查 GUI
契约，再启动 Tauri：

```powershell
npm.cmd --prefix rust\gui run protocol:generate
npm.cmd --prefix rust\gui run protocol:check
```

## Release 打包

推荐的 Windows release 入口：

```powershell
.\scripts\build-rust-release.ps1
```

脚本会先跑 Rust workspace 测试，再构建 Windows MSVC release 二进制，生成
`releases\BugleCat-<version>-x86_64-pc-windows-msvc.zip`，构建 Tauri NSIS
installer，并写出 `releases\SHA256SUMS.txt` 和 `releases\release-manifest.json`。
只需要 CLI 包时可加 `-SkipTauri`；只有同 target 已在 CI 或本地 release 验证通过时，
才建议加 `-SkipTests`。

手动 Windows MSVC CLI release：

```powershell
cd rust
cargo build --release --workspace --target x86_64-pc-windows-msvc
```

手动 Tauri 桌面 installer：

```powershell
cd rust\gui
npm.cmd ci
npm.cmd run tauri:installer
```

桌面构建现在明确产出 Windows NSIS installer，安装包位于
`rust\gui\src-tauri\target\x86_64-pc-windows-msvc\release\bundle\nsis\`。
GUI 的 Settings 弹窗也会展示解析后的 `~/.nanocodex/config.toml` 路径，并提供打开配置文件
和配置目录的入口。

仓库中的 [`macOS Release`](.github/workflows/macos-release.yml) workflow 会分别在 Intel
（`x86_64-apple-darwin`）和 Apple Silicon（`aarch64-apple-darwin`）runner 上构建 macOS
安装包，再把两个安装包挂到同一个 GitHub Release。优先生成 DMG；若 runner 无法创建磁盘
镜像，则自动回退为 `.app.zip`。推送 `v*` 标签会自动触发，也可以在
**Actions → macOS Release → Run workflow** 中手动选择 Release tag。当前 workflow 产出的是
未签名安装包；要给外部用户正式分发，请在 GitHub Actions 中配置 Apple Developer 签名和
notarization secrets。

Tauri crate 特意保留 `crate-type = ["lib"]`；改成 `cdylib` 或 `staticlib` 曾让
Windows GNU 链接器的 export ordinal 表溢出，修改前需重新验证目标 release。

## 安全说明

- **绝不提交真实 API key。** `.env`、`*.key`、`*.pem`、token 文件以及本地交接文件
  都被 git 忽略；`config.toml` / `mcp.toml` 放在 `~/.nanocodex/`，在仓库之外。
- 在 Windows 上沙箱是**策略级**的——它拦截工具行为和可写根，但不是内核级隔离。
- **MCP 工具运行在沙箱之外**，作为外部子进程。只启用你信任的服务；市场会校验
  名称但不审查行为。
- **Hooks 会在工具执行前后运行本地命令**。把 hook 配置当作代码审查后再启用。
- 外部内容（文件内容、命令输出、web/MCP 结果）被当作不可信数据，而非指令。

## 许可证

MIT —— 见 [LICENSE](LICENSE)。
