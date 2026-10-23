# Jev 核验：当前交付、真实开发对照与离线基线

## 2026-10-23：工具执行前动作复核（shadow 先行）

模型仍自主决策；非只读工具调用（写文件、shell 等）在执行前多问 Jev 三个 Noul：`not_requested`（用户没要求/不会预期）、`exceeds_scope`（越出用户允许范围或违反明确限制）、`irreversible`（难以撤销，仅记录供校准）。只读调用不复核。实现见 `rust/crates/ncx-core/src/verification/action_review.rs`，模板 `jev-action-review-v1`，复用 `jev_mode` 与 `NCX_JEV_API_KEY`，单次 4s、无重试。

- `shadow`：只记录，不拦截。
- `enforce`：仅当设置了 `NCX_JEV_ACTION_THRESHOLD`（0.5–1.0）且 `not_requested` 或 `exceeds_scope` ≥ 阈值时，该调用不执行，返回 `Error: ... held by the pre-execution review` 给模型；未设阈值时与 shadow 相同。
- 缺 key、敏感输入、取消、超时或应答不合格一律放行（fail open），状态照实记录。
- `NCX_JEV_MODEL_ENABLED=0` 关闭全部 Jev 调用，包括本复核。

记录在 `.nanocodex/sessions/<session>.action_review.jsonl`（schema `buglecat.action_review.record.v1`），含工具名、参数哈希、调用状态、三个概率、决定与阈值；不存参数正文。阈值尚未校准，生产默认 off；需先在 shadow 积累并人工标注样本后再定阈值。

## 2026-10-11：新增代码产物轨道，原声明轨道保留

用户已选择独立 `artifact_acceptance` 轨道，判断模型原始代码能否满足冻结断言，宿主命题明确标记 `host_artifact_hypothesis`，不要求模型声称测试执行过。[契约](ARTIFACT_ACCEPTANCE_PROTOCOL.md) 已固定；`artifact_acceptance.py` 完成原始来源与独立Oracle绑定，`artifact_scoring.py` 完成预测前注册与仅开发评分，7条已查看代码保持development。参考解/标签/Oracle结果不发Jev；失败/未知保留，不凑正式负例。正式阈值仍null，holdout未运行，native A/B unavailable，生产不启用。

新sidecar真实开发请求已打通，`artifact_observations.py` 独立复核原始transport与收费reservation；1次成功、无重试、NOUL0.12，实际655/25 tokens。新增授权Jev已用1/400，主模型12/400；全套verification Python190 passed。[开发报告](../../reports/artifact-development-smoke-20261011.md)。旧来源只做development，正式intake/校准/留出仍待完成。

v3.3运行包已封存并实机验收，见 [新交付报告](../../reports/release-v33-20261010.md)。随后新增artifact example不在该包封存图内，sidecar另构建。调用入口先做零网络回归，收费仍与原轨道共享授权上限；新版包不意味着两个轨道已认证。

## 2026-10-10 恢复开发：v3.3 与来源审计（当前）

源码策略为 `host-rules-v3.3`：修复实际 `No tests were executed` 的肯定误抽；诚实未执行保持Unknown，权威运行记录可识别错误否认。新运行包正在构建，实际产物状态以 [HANDOFF](../../HANDOFF.md) 顶部和新交付报告为准；旧v3.2付费packet和运行包保留，不能混算版本。

真实来源复审新增 `source_bindings.py`（源文件/协议/授权/group/split/接口前后核对），原始收据额外核对model/usage/span/策略/resource cohort。Oracle升级另存内容寻址收据，旧事实不覆盖。`benchmark_readiness.py` 在扩大付费批次前核对原始审阅队列与全局台账，不生成新标签或概率。

当前12主模型启动/11完成/7有效候选/0合格肯定Semantic，新Jev0；正式阈值null、留出未运行。[当前入库检查](reports/benchmark_readiness_resume_20261010.json) 显示剩余预算即使全部成功也不足400有效组；未完成请求费用未知。接受断言通过只认证有界产物事实，不能改写自然语言声明或凑supported标签。当前生产默认off、类别空，源码修复不代表效果已认证。

零费用回归：`python -m pytest tests/test_verification_source_bindings.py tests/test_verification_benchmark_readiness.py tests/test_verification_benchmark_review.py tests/test_verification_collection_manifest.py tests/test_verification_benchmark_oracle.py tests/test_verification_transport_receipts.py tests/test_verification_paid_budget.py -q`。

## 当前评测阶段：独立标签与校准/留出工具

用户已批准最多50次新增Jev尝试，无自动重试；累计预算上限76。断言标注已运行：修正宿主提示/DSML/HTTP错误污染后，**7有效候选/3任务组，5条限定事实supported、2条Unknown、0负例**。范围复核为model_reviewed，真实人工标签0。原来源回放另发现 `not success` 被当肯定，已回归修复，策略源码升v3.2；旧发布exe仍v3.1。

实际新增**1次**source-bound开发探索，累计**27**，新增预算剩**49**。实际888输入/57输出token，Jev三项概率0.07/0.15/0.22，全部阈值仍Unknown；1正组/0负组，**certified_threshold=null**、真实新留出not_run、生产类别空。main example未在新增发送hash日志字段后重建，本次actual_request_hash未记录：原日志保留，development显式例外且正式门禁拒绝，未重复收费。详见[开发校准](reports/replay_development_calibration_20261010_v32.json)、[费用台账](reports/jev_call_ledger_20261010_r4.json)、[HANDOFF收尾](../../HANDOFF.md)。

使用流程、输入格式、标注准则与留出一次性锁见 [ANNOTATION_PROTOCOL.md](ANNOTATION_PROTOCOL.md)。当前工件：[有效队列](reports/review_queue_20261010_r3.json)、[已标注候选](reports/reviewed_candidates_20261010_r2.json)、[断言收据](reports/assertion_receipts_20261010_r2.json)、[原6回放manifest](reports/fixed_replay_manifest_20261010_r2.json)。旧开发校准结果中的1.0只是未达标网格候选；旧状态及付费报告保留，最新执行证据以HANDOFF顶部为准。

```text
python -m pytest tests/test_verification_baseline.py tests/test_verification_trials.py tests/test_verification_history_inventory.py tests/test_verification_review_queue.py tests/test_verification_calibration_inputs.py tests/test_verification_calibration.py -q
python -m evals.verification.calibration --help
```

新增审计/来源绑定/main入口测试：`tests/test_verification_annotation_audit.py`、`tests/test_verification_replay_inputs.py`、`tests/test_verification_replay_binary.py`；合计9文件120 passed，Ruff通过，Rust core412 passed/1 ignored。example main已真正重建并通过零网络发送收据回归；不替换旧paid缺字段收据。最终[执行状态](reports/annotation_calibration_status_20261010_r4.json)、[冻结manifest核对后的标注](reports/reviewed_candidates_20261010_r3.json)、[构建说明](../../reports/replay-implementation-20261010.md)。

## 2026-10-10 当前交付：host-rules-v3.1

本节覆盖下方历史 v2/v1 描述。R1/R2 的有界证据与判定、R4 本地 CLI/GUI 交付完成；R3 仅完成两题开发对照，独立效果认证未完成。当前运行时沿用 v2 类型并增加来源、覆盖与审计字段，策略版本为 `host-rules-v3.1`。

### 实现与限制

宿主依据原始 session/turn/workspace、事件序列、调用/返回 hash、实际 invocation identity 和持久前后文件快照关联证据。相同精确目标的新鲜成功可替代旧失败，显式引用旧失败仍矛盾；旧来源、重复 ID、越界、链接、不完整快照或 recovery 实际工具身份缺失不能认证。typed runtime provenance 当前只认证 Python 隔离编译的语法范围，不能冒充功能测试。

原用户事件与 InstructionLedger 支持精确主体的明确撤销/替换；附件内容、未解析要求和复杂授权归并仍 Unknown。声明区分肯定、否定、部分、条件、代码、引用与时间叙述；`claim_verdict` 与任务 `verdict` 分开，文本/事实覆盖和要求履行也分开。完整任务 Pass 目前仅覆盖有持久证据的简单直接 Run 目标，不能认证任意修复正确性。

Jev 只处理有界肯定语义，关键完成声明优先；尝试计数包含失败/取消，取消共享原 turn deadline。每轮最多3次调用、最多2次纠正，复用主模型预算。实验宿主策略必须指定类别、有限阈值和版本；概率只能用于实验拒绝，不能将 Unknown 提升为 Pass 或覆盖工具失败、权限。

### 启动、诊断与回滚

新产物在 `releases/jev-r1r2-20261010-164839/`，包括 `BugleCat.exe`、`ncx.exe` 与 `manifest.json`；默认 off，**生产 enforce categories 为空、实验策略 None、生产阈值 null**。进程可设置 `NCX_JEV_MODE=off|shadow|enforce`；仅设置 enforce 不会开放生产拦截。

```powershell
# 默认 off；直接启动新的本地程序
& .\releases\jev-r1r2-20261010-164839\BugleCat.exe
# 显式旁路：包装脚本从当前 Windows 用户环境读取 NCX_JEV_API_KEY
.\scripts\with-jev-shadow.ps1 -Executable 'D:\github_dgy\BugleCat\releases\jev-r1r2-20261010-164839\BugleCat.exe'
```

日志在实际工作区 `.nanocodex/sessions/<session>.verification.jsonl`，与 transcript/source trace 分离；哈希、来源引用、覆盖、策略、模型、尝试状态与用量均可审计。候选/文件改变使旧裁决失效；恢复只显示“有历史核验记录 · 当前未验证”。缺 key、取消、限额和持久化故障不能算通过。关闭时为启动进程设置 `NCX_JEV_MODE=off` 并重启；旧 `releases/jev-shadow-20261010-155325/` 和原安装包保留。新 NSIS 安装器未构建。

### 验证与真实结果

- Rust core 411 passed / 1 ignored、provider 68 passed / 1 ignored、GUI 138 passed；strict Clippy/fmt、前端 build/typecheck、protocol 5 tests 通过。Python 35 passed 与 Ruff 通过。ignored 为收费 opt-in，未运行；详见 [实施报告](../../reports/implementation-r1r2-20261010.md)。
- 新复制 exe 的 [GUI 生命周期](../../reports/gui-lifecycle-new-20261010.json) 与 [CLI/GUI/恢复检查](../../reports/runtime-smoke-new-20261010.json) 通过：三模式、取消、断网、日志故障、恢复均无伪造通过或重复写入，CLI exit0。使用15次 localhost 主模型请求，**0 Jev 请求**。GUI 取消位于主模型等待阶段，Jev checking 阶段取消由异步 transport 回归覆盖，未进行该阶段付费 GUI 实测。
- 同两题、同主模型/源码/runner/预算的真实开发对照：original **2/2**、rules **2/2**、jev **1/2**。Jev 组功能修复成功但旧失败规则误拦，已回归修复；不重跑付费组覆盖失败。付费数据对应 `host-rules-v3`，最终包为 v3.1；**不能据此宣称新版减少幻觉或效果已认证**。[R3 报告](R3_RESULT.md)、[三组汇总](reports/trial_summary_20261010_r3.json)。
- 本轮新增4次 Jev 尝试/响应，累计 **26/30**；实测 `jev-1.13.0`，新增2,807 input / 228 output tokens，金额未知。[累计台账](reports/jev_call_ledger_20261010_r3.json)。30是累计首批预算，后续不得自动扩大收费或默认重跑三组。
- 真实历史只读盘点93个工具结果：19成功退出、20失败、54未知；完成标签未独立复核。[盘点](reports/history_inventory_20261010_r2.json)、[禁止启用依据](policy_status.json)。独立聊天保留的 trace 文件未代写。

可重复的零付费回归：

```text
python -m pytest tests/test_verification_trials.py tests/test_verification_history_inventory.py tests/test_verification_baseline.py -q
cargo test -p ncx-core --lib --manifest-path rust/Cargo.toml
cargo test -p ncx-provider --lib --manifest-path rust/Cargo.toml
```

生命周期和正常 smoke 的 `NCX_JEV_GUI_EXE` / `NCX_JEV_CLI_EXE` 应指向上述新复制产物，报告路径使用新文件。默认不带 `--live`，子进程 key 显式为空；勿为重复检查运行收费命令。完整后续开发次序与剩余验收见 [剩余计划](../../docs/superpowers/plans/2026-10-10-jev-remaining.md) 和 [HANDOFF](../../HANDOFF.md) 顶部。

## 历史记录：2026-10-10 Rust v2（已由上节覆盖）

本节覆盖下方历史原型描述。类型在 `rust/crates/ncx-core/src/verification/types.rs`；`verification/{evidence,snapshot,policy}.rs` 负责宿主来源、实际命令范围、UTF-8 原文 spans、文件 SHA-256 与规则裁决。核验在最终答案提交前运行。未知声明覆盖不能标为 Pass；Jev 原始概率只作观察，不能覆盖工具失败或晋升 unknown。

`AgentLoop.verification` 从进程 `NCX_JEV_MODE=off|shadow|enforce` 初始化，默认 off。生产 enforce 类别为空，因为独立效果门槛未达；仅宿主显式配置的确定性类别可触发最多两轮纠正。无需重放写操作。`NCX_JEV_API_KEY` 从当前 Windows 用户环境进入启动子进程，不写配置或仓库。启用新产物：`scripts/with-jev-shadow.ps1 -Executable <new exe>`；关闭为进程 `NCX_JEV_MODE=off` 后重启程序。回滚使用保留的旧安装包，不混用新 exe 的核验能力。

记录在工作区 `.nanocodex/sessions/<session>.verification.jsonl`，与 `.jsonl` transcript 和 `.sources.jsonl` 分离。记录包含候选/输入哈希、版本、source refs、实际模型、概率、token 用量、耗时和调用状态；不存完整候选或证据正文。缺 key、取消、限额和写入失败均可观察。GUI 历史记录只显示“有历史核验记录 · 当前未验证”。旧 v1 概率记录不迁移成事实。

限制：语义覆盖与历史要求的撤销/归并未认证；复杂 shell、并行批次、symlink、文件读取失败、超量目录保留 unknown。完整快照上限 16MiB / 2048 files / 4096 entries / depth 24，排除当前诊断日志及生成目录。仅引用实际命令或 `[tool_call_id]` 的窄声明可以关联证据。已知密钥/账号字段脱敏不是任意秘密识别保证。每轮最多 3 次、每次 8s、48KB request / 32KB response、无重试，共享原任务 deadline 和 model-call 预算。

### 显式评测（不在默认收费 CI）

`jev_evaluate` 仅接受带合成标记的 v1 样本，最大 30 请求，报告必须为新路径；遇到 401/403/429 停止。18 条作者标注、作者已看过的样本回放见 `reports/jev_live_20261010.jsonl` 和 `jev_summary_20261010.json`。原框架真实任务对照为 not_run，费用无法确认写 null；不能据此声称独立盲测或减少幻觉。

```text
cargo run -p ncx-provider --example jev_evaluate --manifest-path rust/Cargo.toml -- evals/verification/cases.json <new-report.jsonl>
python -m evals.verification.summarize_live --cases evals/verification/cases.json --observations evals/verification/reports/jev_live_20261010.jsonl --output <new-summary.json>
node rust/gui/e2e/verification.mjs --live
```

最后一条仅用于隔离工作区：主模型为 localhost 固定夹具，CLI 不带 Jev key，GUI 恰好一次合成 Jev 请求；不等于真实主模型效果评测。先从用户环境显式加载 key；不要反复运行收费命令。最终测试/产物和剩余 checkbox 以 HANDOFF 当前执行节为准。

### 本轮可运行产物和无收费检查

交付目录 `releases/jev-shadow-20261010-155325/` 含 `ncx.exe`、`BugleCat.exe`、`manifest.json`；以当前未提交工作树构建，包含既有 WIP。manifest 保存源文件与 exe SHA-256，旧安装包没有更新或覆盖。最终复制产物已实际启动：`reports/staged_no_key_smoke_20261010.json` 的 CLI exit=0、GUI call_status=missing_key/verdict=unknown。

不带 `--live` 的 `verification.mjs` 不向 Jev 发请求。可用 `NCX_JEV_GUI_EXE`/`NCX_JEV_CLI_EXE` 指定复制产物；`NCX_JEV_SMOKE_REPORT` 指定报告路径。`NCX_JEV_RESTORE_ROOT` 指向此前脚本报告中保留的合成 fixture root 时，只启动/恢复 GUI，不创建新任务，不发主模型或 Jev 请求；`restore_smoke_20261010.json` 保留最终 exe 重启恢复断言。此模式修改的配置仅在该合成 fixture 内。

本轮收费台账 `jev_call_ledger_20261010.json` 为 20 请求（18 夹具 + 2 实际 GUI），不是每次 UI 自动化尝试都收费。首个 paid GUI 的后续 selector 失败仍计数；没有隐藏收费重试。新合成/真实任务评测应使用新报告路径并单独限定预算。

## v1 离线基线与历史记录

日期：2026-10-10。`contract.py` 是可执行的 v1 数据契约，使用现有 Pydantic 依赖。`baseline.py` 仅是离线规则参考实现，尚未接入 Rust Agent，也不调用 Jev。

## 所有权与边界

- Rust 运行时的 `ncx-core::verification`（拟建）负责证据组装、策略和裁决；provider crate 负责 Jev HTTP 协议。`turn.rs` 只编排调用。
- 输入由宿主构造。`coverage`、证据状态、scope 和文件哈希不能信任主模型提交的值；未来集成必须从工具原始结果、测试执行参数及文件系统生成或核验。
- 本基线假设输入的结构化 scope/coverage 已正确提取，尚不解决自由文本声明抽取、范围语义识别及恶意证据污染。相应能力需独立评测。
- 未提交变更由文件内容哈希识别；首次 v1 对测试使用完整相关文件哈希映射相等的保守策略。跨轮证据暂不直接放行，未来需显式重新验证。
- 原始结果保留 source_ref；日志中仅保留脱敏数据。候选答案不能作为自身证据，摘要不进入权威证据类型。

## 输入与输出

输入版本 `buglecat.verification.input.v1`：当前会话/轮次、有效要求、候选全文、原文结论片段、证据关联、当前文件哈希、未决调用、覆盖状态。重复 ID、未知字段及无法在候选全文定位的结论片段属于契约错误。

输出版本 `buglecat.verification.result.v1`：总体裁决、逐结论裁决、固定原因、关联证据、执行方式、调用状态、模型和策略版本。

- `pass`：当前结论均有支持，且无未知覆盖/未决调用。仅代表校验通过，不能自动授予权限。
- `need_evidence`：来源缺失、跨会话、截断、状态未知、范围不符、版本过期或未决调用。
- `contradiction`：明确失败、非零退出码、超时或取消与成功声明冲突。
- `unknown`：语义尚未判定、结论覆盖未确认或诚实表达限制。未知不是虚假回答；允许如实交付“未验证”的结果，但不得标记任务验证成功。

聚合优先级：contradiction > need_evidence > unknown > pass。覆盖不完整至少为 unknown，未决调用至少为 need_evidence；二者不能覆盖已经发现的 contradiction。空结论不能通过。缺失退出码不推断成 0；对 file_read 等非 shell 工具允许使用宿主权威结果状态。

Jev 调用失败（timeout/cancelled/error）必须输出 unknown，和正常响应里的语义 unknown 分开统计。确定性明确失败不能被 Jev 改为 pass；概率阈值在真实模型评测后单独版本化，本契约不预设阈值。

运行集成必须额外记录：输入内容哈希、问题模板版本、实际模型版本（服务若提供）、调用耗时、用量以及模式；这些属于调用审计信封，不伪造离线基线没有的数据。

## 运行策略定稿

- off：原有检查和行为。
- shadow：附加记录，不改变任务状态或触发纠正。
- enforce：仅启用通过评测的类别；补证据/改写总计最多两轮，共享原任务截止时间。已取消不再调用；预算耗尽或 Jev 故障交付明确未验证状态，不误记通过。
- 不重复自动执行写操作；补证据仍经既有权限与工具运行链。
- 修订答案或证据变化使旧裁决失效。最终确认之前的流式内容必须带草稿状态；会话恢复不能恢复成已验证。
- 先离线、后 shadow、最后按类别 enforce。首版默认 off，不在本次任务修改配置。

## 样本与标签

`cases.json` 共 18 条：8 条 calibration、10 条 holdout；包含成功对照、无执行记录、失败、超时、取消、范围误用、版本变化、来源错配、未决调用、漏抽结论、诚实限制和工具文本注入。

全部是根据已知故障类别人工编写的合成样本，不是原始日志回放。标签为作者标注，尚无独立复核。holdout 按 group 隔离，但同一作者看过全部样本，不能视为独立盲测；未来生产结论必须使用新的独立留出数据。

负例分母仅包含 need_evidence/contradiction；unknown 单独报告，不能当成负例或成功。正例分母为 pass。样本数很小，零错误不代表真实错误率为零。

## 可复现命令与初始结果

```text
python -m pytest tests/test_verification_baseline.py -q
python -m evals.verification.baseline --output evals/verification/reports/baseline_20261010.json
```

本轮：23 tests passed。规则基线 calibration 8/8、holdout 10/10 与标签一致。共 11 个明确负例中错误放行 0，3 个正例中误拦截 0，4 个 unknown 保留未知。报告保存数据 SHA-256 和 policy version。

这些数字仅验证结构化夹具上的规则行为。原 Rust Agent 的端到端基线与 Jev 对照均为 not_run；未测任务成功率、延迟及成本，也未证明实际减少幻觉。

## 下一步

1. 独立复核标签，加入脱敏真实轨迹及不同任务的成功样本。
2. 建立全文声明抽取/证据组装评测，验证本基线当前假设的 scope 与 coverage。
3. 固定主模型/任务预算，重跑 Rust 原框架基线，再实现 shadow 接入并比较三组。

## 2026-10-10：Rust shadow 已接入源码

`ncx-provider/src/jev.rs` 调用官方 `/v1/systemone`，校验三项 Noul 概率、禁止重定向、8 秒超时，不记录远端错误正文。`ncx-core/src/verification.rs` 在 turn 完成分支记录 shadow 观察，不改变回答、权限或任务结果。

当前使用独立 `buglecat.verification.shadow_input.v1` / `shadow.v1`，没有冒充完整逐声明 v1 裁决。输入仅当前轮最多 12 条工具结果，每段/候选/要求最多 2000 字符；coverage 始终 unknown，文件新鲜度未验证，最终 verdict 始终 unknown。三项概率为观察值，未校准，不用于拦截。常见 key、Bearer 和密码赋值字段会脱敏，但这不是任意秘密识别保证。不得用于包含未获准外发敏感内容的任务。

观察保存在原 session 的 `.sources.jsonl`，status=`jev_shadow`，含输入/候选哈希、模型、模板、概率、耗时与 token 用量；无 session 日志时不落盘。当前复用 DynamicContext，尚无专门 GUI 校验状态展示。

配置：环境变量 `NCX_JEV_API_KEY` 和 `NCX_JEV_MODE=shadow`；其他模式均不启用。用户提供的第一把 key 已保存到本机当前 Windows 用户环境，未写入仓库；第二把未保存。环境变量不是加密凭证库，同用户进程可读取。已有进程不会自动获得新配置。

启动新构建时可用 `scripts/with-jev-shadow.ps1 -Executable <新构建的程序路径>`，脚本从用户环境加载凭证，只为本次程序开启 shadow，并恢复调用环境。旧安装包不含本次源码，需重新构建；本轮未替换安装包。

验证：provider 和 core 的 lib 回归通过；两个显式 ignored 的真实调用测试分别执行通过（协议调用、日志落盘且会话不变）。真实协议样例返回 jev-1.13.0，矛盾概率 0.98、无证据成功 0.95、不完整 0.94；这是 smoke，不是准确率基线。默认测试不调用收费接口。

还缺：完整声明/证据组装、文件版本与范围验证、标签独立复核、三组真实任务基线、HTTP 故障注入矩阵、GUI 展示及 enforce。Task 3/4/5 不应因此整体勾选完成。
