# 更新日志

[English](CHANGELOG.md) | 简体中文

BugleCat 按日期记录的更新说明。[README](README.zh-CN.md) 描述当前行为；这里记录改了什么、何时改的，最新的在前。

## 2026-10-23 —— 审查后的正确性与安全修复

对 2026-10-23 里程碑做代码审查时，发现文档描述与实际运行之间有出入。已修复：

- **堵住 shell 审批绕过（`ncx-sandbox`）**：`untrusted` 下 `python`/`node`/`pytest` 不再自动放行；`git config` 写操作与可执行类覆盖（`core.hooksPath`、`core.pager`、`-c alias.*`、`GIT_SSH_COMMAND=…`）需要审批；通配或转义的敏感文件（`cat .en*`、`.e""nv`）以及先 `cd %USERPROFILE%` 再写入的命令链会触发审批；联网检测覆盖 `certutil -urlcache`、`bitsadmin`、`npx`、`gh`、内联代码中的 socket，以及更多包管理器子命令。
- **整棵进程树受控（`ncx-tools`）**：Windows 上命令以挂起方式启动，加入 Job Object 后才开始运行；Unix 上命令自成进程组，超时时整组结束。`on-failure` 只在真正被沙箱拒绝后才提供重试。
- **Propose–Probe–Compose 真正生效且经过复验**：验证提示词已说明 `COMPOSE:` 的用法；组合结果先在暂存副本中复验，通过才提升，否则回退为只采用最佳 Worker；重试用尽仍未通过的轮次不再组合；验证结论按开头的 `PASS`/`FAIL` 判定。
- **forge 验收门高于噪声**：候选须以采样噪声下限（`--noise-z`）以上的幅度胜过现任 champion；默认 `--repeats 3`、`--accept-margin 2`；接受后重评 holdout；lesson 只在 compose 声明实际采用时才记功记过。
- **回归护栏**：否定措辞（"no regressions"、"未引入回归"）与单纯的 Rollback 不再写入回归护栏，同一模块后续验证成功会清除护栏。
- **文档**：2026-10-19 条目中提到的 `memory_auto_promote_enabled` 只写在文档里，从未实现；已验证笔记的自动晋升只在 `[feedback] memory = "on"` 时发生。该配置项已从 README 和 `config.example.toml` 中移除。
- **仍未解决**：shell 命令靠策略与命令行检查拦截，没有 OS 层面的文件系统/网络隔离（见 [沙箱与审批](README.zh-CN.md#沙箱与审批)）。

## 2026-10-23 —— 第三阶段里程碑：自进化 Harness 正则化（对照 Google RRSI / Harness²，P0–P4 全量落地）与配对复利回放实证

- **P0 —— 噪声校准配对评测（[`evals/compounding_replay/analyze.py`](evals/compounding_replay/analyze.py)）与第二基准仓库（`python-tabulate`）**：
  * **统计配对与噪声下限校准**：引入单臂噪声下限校准（解决数按 Jeffreys 平滑伯努利方差求 $\delta_{\text{resolved}}$，成本按逐题样本方差和求 $\delta_{\text{cost}}$）与双臂逐题配对检验（精确 McNemar 检验 + 4000 次按题配对 Bootstrap 95% 置信区间 + 解决率非劣效护栏）。
  * **第二真实仓库题库（`astanin/python-tabulate`）**：加固 [`mine_tasks.py`](evals/compounding_replay/mine_tasks.py)（新增 `--body` 全文检索与合并提交支持、`issue["created_at"] <= fixed_at` 时间戳护栏精准拦截 7 个早年 Bitbucket 编号错配任务），通过 [`validate_gold.py`](evals/compounding_replay/validate_gold.py) 完成 9/9 官方黄金补丁验证。
  * **`tabulate` 3 轮配对实测（每臂 27 次运行）**：Arm B 相比 Arm A 将主指标「每解决一题成本（`cost_per_resolved`）」降低了 **21.0%**（**`B / A = 0.7904`，按题配对 Bootstrap 95% CI `[0.6239, 0.9523]`，不包含 `1.0`**；每轮均摊成本 `1.313` 元 vs `1.586` 元，配对 95% CI `[-0.5207, -0.0685]`），同时严格解决率满足非劣效护栏（`22/27` vs `21/27`，`+3.7pt >= -14.9pt`）。注：用同一配对脚本重算早期 2 轮 `sqlparse_split` v2，得 `B/A = 0.7131`，95% CI `[0.3693, 1.1586]`（跨过 `1.0`，定性为未定论）。需要注意：Bootstrap 只在 9 道题上重抽样，百分位置信区间并不稳健；非劣效余量（`14.9pt`）也宽到无法排除少解一题。在补充更多题目之前，成本下降应视为初步结果。
- **P1 —— `ncx-forge` 离线进化搜索的 RRSI 正则化（`train/`）**：
  * **表面级退火编辑预算**：新增 `changed_surfaces` 与 `changed_lines`，支持按轮退火的表面数预算（$\max(1, \text{max\_surfaces} - \lfloor r / 2 \rfloor)$）与修改行数硬上限（`--initial-max-surfaces`（别名 `--max-surfaces`）、`--anneal-surfaces`、`--max-changed-lines`）。
  * **规则版反过拟合/反作弊审查器（`critic_check`）**：自动拦截在新增文本中硬编码训练集题号、`check.py`、`_check.py`、`hidden test` 的作弊候选。
  * **证伪假设账本与停滞自动转向**：每个候选写入结构化实验账本 `train/runs/ledger_<stamp>.jsonl`，向 Teacher 自动注入最近被拒的证伪历史（`Falsified / Rejected Mutations`），并在连续 `--stagnation-limit` 轮停滞时自动转向探索最少的表面。
  * **双通道成本感知验收门与盘后消融剪枝（`--prune`）**：支持「通过数提升（须超过 `analyze.pass_noise_floor` 给出的采样噪声下限，`--noise-z` 默认单侧 95%；接受后用新 runs 重评 holdout）+ `--max-token-growth` 防膨胀护栏」或「同分下显著节省 Token（`>= --delta-tokens`）」双通道晋级，并在搜索结束后对最终 Champion 逐面回滚重评，剔除零收益的搭便车冗余修改。
- **P2 —— 扩大 `NCX_GENOME` 可进化面（`rust/` + `train/`）**：
  * 在保持空 Genome 100% 字节级 No-op 等价保证（且不触碰冻结文件 `prompt.rs`）的前提下，新增 `[orchestrator_prompts]`（覆盖 `classify_sys`、`plan_sys`、`decompose_sys`、`worker_sys`、`verify_sys`、`compose_verify_sys`）与有界 `[memory_config]`（`max_entries` $\in [1, 32]$、`max_chars` $\in [200, 16000]$），打通 `ncx --dump-genome` 与 `train/genome.py` 双向校验。
- **P3 —— Harness² 在线记忆双层结构化、负向回归护栏与成本感知归因（`rust/crates/ncx-core/`）**——仅在 `[feedback] memory = "on"` 时生效；默认的 `"shadow"` 模式不写入任何笔记，以下机制都不会运行：
  * **模块级不变量提炼（`promote_module_invariant_if_due`）与负向回归护栏（`record_regression_guardrail`）**：当同一模块在外部裁判确认通过（`Accepted`）后累计达到 `>= 2` 条验证笔记时，把该模块最新（最多 3 条）已验证笔记的片段合并为一条模块不变量 `[Module Invariant: <module_path>]`（赋予 `+6` 召回优先级加分，并按规范化头部哈希保持原地更新时 `note_id` 与历史权重不变），同时将该模块名下的旧单题情景笔记裁剪至最多 `MAX_EPISODIC_PER_MODULE = 2` 条并同步重建 SQLite 索引（`rebuild_from_markdown`）；只有在明确报告回归时（`p2p_regressed>0`，或负向标注中出现未被否定的"回归"说法；单纯的 Rollback 不算）才按模块写入 `[Regression Guardrail: <module_path>]` 负向护栏，该模块之后验证成功即清除。
  * **召回预算收紧与高消耗轮次精准打折（`record_unhelpful_injection`）**：默认召回上限从 `8 条 / 4000 字符` 收紧至 `6 条 / 2400 字符`；当高消耗轮次（`iterations > 16 || tool_calls > 24 || total_tokens > 80_000`）完成时，仅命中实际修改文件名的召回笔记获得 `accepted + 1`，未命中的搭便车笔记仅记 `injected + 1`。在 `tabulate` Arm B' 实测（`20261023-115758-B-tab-p3-r1`，仅 1 轮）中，`learnings_entries` 从第 4 题到第 9 题全程稳定封顶在 `6` 条，总成本 `1.413` 元（较 Arm A 均值下降 `10.9%`，小于单层记忆 Arm B 的 `21.0%`；因此能确认的只是条目数有上限，P3 本身的成本收益尚未得到证实）。
- **P4 —— Harness² 运行时与离线进化双层 `Propose–Probe–Compose` 闭环（[`orchestrator.rs`](rust/crates/ncx-core/src/orchestrator.rs)、[`train/forge.py`](train/forge.py)、[`train/lesson_lib.py`](train/lesson_lib.py)）**：
  * **运行时多 Worker 非冲突文件级组合（`compose_with_paths` / `COMPOSE:i,j`）与重试探测合成（`build_compose_feedback`）**：`VERIFY_SYS` 告诉 Verifier：当其他 Worker 在不同文件上做了互补修改时，在 `BEST:i` 之后再输出 `COMPOSE:i,j`。组合结果（与前面 Worker 改了同一文件且内容不同的次级 Worker 整个剔除）先在工作区的一次性副本中暂存，由可调用工具的验证者（`COMPOSE_VERIFY_SYS`）复验；只有 `PASS` 才通过 `HarnessAgentRunner::compose_workers` 提升，否则只提升最佳 Worker 并在结果中说明原因；重试用尽仍未通过的轮次不会组合；当首轮 $N \ge 2$ 个 Worker 未通过验证进入重试时，自动将 Verifier 审查意见与所有 Worker 的探测结论摘要（`[Propose-Probe-Compose Peer Digest]`）合成为下一轮输入。
  * **离线 `ncx-forge` PPC（`--h2-k`、`--h2-mode parallel|sequential`、`--h2-blind`）与跨轮改进库（[`train/lesson_lib.py`](train/lesson_lib.py)）**：对单一失败任务在同条件下探测 incumbent 对照 + $K$ 个通用/领域特定候选（`keep_trajectory=True`），确定性生成隐藏裁判结果的纯行为对比（`build_contrast`），由 Teacher 合成 1 个 `composed` Genome 与 1 条元经验草案（越界时自动回退至最佳合法 Probe 候选），再进入外层全量 `train + holdout` 双通道门。`LessonLibrary`（`lessons.jsonl` + `adopted_harnesses.jsonl`）按症状重叠度 $\times$ Beta 后验 `(support+1)/(support+refute+2)` 检索经验，在外层门通过/被拒时，只对 compose 在 `applied_lessons` 中声明实际采用的 lesson 更新 `support`/`refute`（`--prune` 时只追责被剪掉的那次采纳所归功的 lesson），并在 `refute >= support + 2` 时自动退役为证伪禁令；[`replay.py`](evals/compounding_replay/replay.py) 新增 `--genome PATH` 支持直接回放评测进化产物（详见 [`docs/harness2-forge-integration.zh-CN.md`](docs/harness2-forge-integration.zh-CN.md)）。

## 2026-10-19 —— 第三阶段底座：智能复利闭环、无人工介入完成率（ACR & 60 分奇点）+ 跨 Agent 平台升级

- **无人工介入完成率（ACR）与 60 分奇点门禁**：在单 Agent 与多智能体编排主控出口统一采集隐私脱敏的 `TaskOutcome`，收敛为 **4 个核心工程任务类别**（`code_fix_test`、`compile_repair`、`feature_impl`、`doc_refactor`；纯问答 `chat` 与子任务 `OrchestratorWorker` 排除出主分母）。当某类别在滚动 30 天内已定论样本 `n >= 30`、`ACR-strict` 的 Wilson 95% 置信下界 `>= 60%` 且返工率 `<= 15%` 时，正式判定该类别跨过 **60 分奇点**。
- **零守护进程全链路后续信号（`I1–I5` + `L`）**：自动归因 `I1` 显式阻断/失败、`I2` 审批/提问介入、`I3` Checkpoint `/restore` 回滚、`I4` 中/英/日多语言纠正与同文件返工推断、`I5` **零常驻 `LazyGitUndoDetector`**（在新会话启动及下一轮开始前通过 `git merge-base --is-ancestor` 与 `git status --porcelain` 检测跨会话 `git reset` / `git revert` / 手工覆写 Agent 修改过的文件），以及 `L` 人工显式标签（CLI/GUI `/outcome good|bad` + GUI 消息级 `♡`/`♧` 精准按轮次绑定）。
- **带 14 天基线门禁的在线回流（`shadow` → `on`）**：`[feedback] memory / routing = "off" | "shadow" | "on"`（默认 `"shadow"`）。从 `shadow` 切换至 `on` 受 **14 天 Shadow 基线门禁** 保护（需满足至少 14 天 Shadow 观测期且至少 2 个核心类别 `n >= 30`，支持 `--force` 强制覆写）。开启后执行有界 Beta 后验记忆调权（`±20%`，每个已定论任务仅计分一次）、低效记忆自动降级（`injected >= 8 && p < 0.25`）与编排器路由单调升级（`Simple -> Medium -> Complex`）。
- **复合恢复链沉淀、Worker 隔离脱敏与频次优先晋升**：自动提取多步 `tool_failed -> diagnostic/read -> tool_succeeded` 复合恢复链（`CompoundRecoveryChain`）为 `Candidate` 候选记忆；子任务隔离副本（`.buglecat-subruns`）不直接写主库，仅在胜出 Worker 被主控 `promote` 且回合成功时，将临时路径脱敏还原为真实工作区路径后落盘；支持跨会话复现计数（`occurrences`，按 `命中 N 次` 降序排列）、零介入高置信度自动晋升（`memory_auto_promote_enabled`）与人工审阅（`ncx --memory-review` / GUI 审阅面板）。
- **跨 Agent 核心机制吸收（Pi / OpenCode / Codex）**：
  * **Pi 风格会话内建树状链路追踪（`_trace`）**：每条 `session.jsonl` 消息内嵌 `MessageTrace`（`id`/`parent_id` 树指针、轮次号、SHA-256 摘要、Git revision、生效模型与单步 `usage`、工具耗时与退出码、压缩位点 `first_kept_entry_id`），发往模型前自动剥离。
  * **OpenCode 风格编辑后自动诊断回写**：`apply_patch` 与 `str_replace_editor` 修改文件后自动触发增量 LSP 诊断（`textDocument/didChange`）、JSON/TOML 语法校验与 Git 冲突标记检测，直接追加至工具返回值。
  * **Codex 风格超长工具输出自动落盘卸载**：单次工具输出超 16,000 字符时完整落盘，内联返回前 8,000 + 后 8,000 字符预览及 `[tool_output_offloaded: path=... offset=...]` 句柄供 `read_file` 按需分段精读。
- **常用命令与桌面端入口**：
  * 查看工作区奇点与 ACR 报告：`ncx --outcomes report --since 30d --by category,model,policy`（或 REPL / GUI 输入 `/outcome report`）
  * 跨项目历史会话冷启动提取：`ncx --outcomes bootstrap <仓库路径1> <仓库路径2> ...`
  * 候选与降级记忆审阅：`ncx --memory-review [list | confirm <id> | reject <id> | restore <id>]`
  * 桌面端可视化奇点仪表盘：打开 Tauri 桌面端右侧 **「记忆」** 面板，即可实时查看 **ACR & 60 分奇点仪表盘**、分类样本进度与 14 天基线状态。
- **`v0.1.2` 预编译发行包与声明级核验基线（`host-rules-v3.4`）**：
  * [GitHub Release `v0.1.2`](https://github.com/dgy-github/BugleCat/releases/tag/v0.1.2)（`BugleCat_0.1.0_x64-setup.exe`、`buglecat-20261014-final.zip`、macOS `arm64`/`x64` DMG）· [三模式验收收据](reports/buglecat-delivery-20261014.json) · [175 例声明漂移基准报告](reports/claim_drift_eval_report_v1.md)
