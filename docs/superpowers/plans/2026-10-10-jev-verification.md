# Jev 决策校验层开发计划与记录

日期：2026-10-10（Asia/Tokyo）。状态：Rust v2 完成前核验、有限纠正机制、GUI 与可运行旁路已实现；完整语义覆盖和真实任务效果门槛未完成，生产 enforce 类别为空。

当前执行记录：GPT-6.1-Sol 本轮执行已结束（16:00），完成 P0/P2/P4/P5 的可运行旁路交付，P1 保守支持窄执行/测试声明；P3 未达真实三组效果门槛。CLI/GUI 新版已构建并实际启动，结束记录见 HANDOFF 当前执行节；整体未知不能宣称 Verified。

## Rust v2 定稿与兼容

Python v1 夹具不覆盖 Rust 宿主 provenance 契约。Rust `verification/types.rs` 是 v2 类型事实源：Claim 增加候选原文 UTF-8 byte span，Evidence 增加实际 command/source_ref/timestamp/session/turn/workspace 和本地 before/after/current SHA-256。全文句段均有条目，但要求满足/语义覆盖无法认证时整体 coverage=unknown；单个 narrow claim supported 不表示任务完成通过。

模式装配所有者为 `AgentLoop.verification: Config`，从进程环境读取、默认 off；shadow 保持放行，enforce 仅对宿主显式提供的已审查确定性类别补正。生产类别为空，未校准 Jev 分数不参与拦截。关闭为 `NCX_JEV_MODE=off` 后重启；启动脚本继承用户环境 key，不把 key 写仓库。

独立 `session.verification.jsonl` 用 v2 record，旧 shadow v1 仍是诊断，拒绝升格；旧 `jev_shadow` SourceRecord 不自动召回。错误代码包括 missing_key、cancelled、timeout、input_budget、response_budget、call_budget、http_400/401/429/5xx、invalid_json/model/answer/usage、persistence_failed/no_durable_log。调用成功、事实核验、任务交付分别记录。缺 token 计数或非法计数拒绝响应，不伪造费用。

限制：保留全部可用 user text 与多段文本而不自称正确完成历史要求撤销；复杂命令 scope、并行工具批次、超量文件树、symlink 或变化均不认证；运行时 Python syntax evidence 不可代替测试全量通过。

## 目标和第一期范围

降低无证据完成声明和证据范围误用，提高最终任务成功率。第一期实现完成前的证据校验、有限补证据及旁路评测；行动选择和摘要/记忆校验留待后续评估。

方案入口：仓库根目录 `HANDOFF.md` 的 2026-10-10 记录。开发沿用 Rust 主运行链；Python 评测代码用于离线评测，不能替代 Rust 集成验证。

## 已核对的复用位置

- `rust/crates/ncx-core/src/agent_loop/turn.rs`：已有候选完成回答、Python 复检、运行时证据与最终完成处理。接入点在既有必要检查完成后、最终答案确认前。
- `rust/crates/ncx-core/src/context_store.rs::tool_call_detail`：工具调用状态、退出码、来源、工作区版本；无结果为 Unknown，退出码缺失不能推断为 0。
- `rust/crates/ncx-core/src/context_trace.rs`：来源 ID、调用关联、时间、原始内容哈希与 RuntimeVerification 类型。
- `evals/golden/judge.py`：已有可选裁判和 SKIP 不计通过语义，但调用协议为 Chat Completions，Jev 需要独立适配。

## 契约草案（实施时定稿）

`VerificationInput`：schema_version、session_id、turn_id、候选回答哈希、当前有效要求、候选结论、来源证据、未决调用、证据截断/缺失标记、工作区状态。

每条 `Claim`：claim_id、候选回答中的原文位置、结论类别、所引用 evidence_ids。主模型提出的引用必须由框架核验；检查声明抽取的覆盖率，防止漏抽关键结论导致假通过。

每条 `Evidence`：source_id、tool_call_id、工具和参数摘要、真实状态与退出码、时间、结果片段与原文引用、文件内容哈希及测试范围。Git revision 不能单独代表未提交文件内容；摘要和候选回答不能作为自身事实证明。

`VerificationResult`：verdict（Pass / NeedEvidence / Contradiction / Unknown）、逐结论判断、概率或评分、evidence_ids、固定 reason_code、实际模型标识、问题模板版本、策略版本、耗时与用量。不得假设 Jev 能生成自由文本解释；面向用户的提示由 reason_code 和证据模板生成。

运行模式：off / shadow / enforce。shadow 记录判断而不影响现有完成流程；enforce 仅对通过验证的类别生效。调用失败必须和模型判断 Unknown 分开记录，均不能算校验通过。

## 不变量与失败处理

- 确定性失败优先于模型概率；权限拒绝、取消、工具参数错误均不可由 Jev 放行。
- 校验输入由框架从可信事件记录组装；外部文本只作为证据，不允许改写校验规则。发送前脱敏，密钥和敏感配置不进入输入及日志。
- 证据按会话、任务、工作区隔离；截断或缺失明确标记，并允许补取原始记录。
- 检查后文件再次变化时，相关验证失效；缓存键包含候选回答、证据内容哈希、模型和模板/策略版本。
- 补证据或改写最多两轮，共用原任务截止时间、工具和调用预算。重复问题不无限重试；修订答案必须重新校验。
- shadow 调用失败只记录未校验；enforce 调用失败不得标记 Verified，可输出明确未完成/未验证结果。两种模式都保留现有确定性检查。
- 流式候选答案在确认前必须区分草稿与已确认结果，失败后不得在历史、轨迹或 UI 中残留已确认成功状态。
- 阈值、超时和成本上限通过实测定稿；不把供应商宣传数据或未经校准的概率当作生产保证。

## 开发任务

### Task 1：契约与策略设计

- [x] 定稿 VerificationInput / Claim / Evidence / VerificationResult v1；可执行契约见 `evals/verification/contract.py`，语义见同目录 README。
- [x] 明确证据范围、文件新鲜度、声明抽取和完整性检查规则；抽取实现和准确率验证仍属后续开发。
- [x] 定稿 off / shadow / enforce、取消、超时、重试预算与降级语义。
- [x] 落实 Rust 模块和配置接线；core verification + provider Jev 客户端，Agent 装配时读取环境模式。

验收：每条失败路径有明确状态；Jev 判断不能覆盖确定性失败；不会新增平行权限系统。

### Task 2：评测样本与基线

- [ ] 从历史失败轨迹提取脱敏样本：DSML 未执行、局部/全量测试混淆、超时、旧结果、执行结果矛盾。
- [ ] 加入正确完成、合理不确定、证据缺失、证据含恶意指令和关键结论漏抽的对照样本。
- [ ] 人工或可执行断言确定标签；按会话/任务分组划分校准集与独立留出集，避免近重复泄漏。
- [ ] 跑原框架和仅确定性规则基线，保存版本、数据范围及错误明细。

验收：标签可复核；缺失/跳过不算成功；评测覆盖失败和正确放行，不能只测能拦截多少错误。

### Task 3：证据组装和确定性检查

- [x] 从 ContextStore / SourceTraceStore 组装有预算限制的证据包；无法精确关联的来源不认证。
- [x] 校验来源引用、调用终态、退出码、实际窄测试命令及 bounded 文件变化；复杂范围/并行批次保留 Unknown。
- [x] 对全文句段候选关联显式命令或 `[tool_call_id]` 引用，识别缺失或过期证据，保留原文 source_ref；语义覆盖仍未认证。
- [x] 增加跨会话隔离、截断、Unknown、文件改动和引用伪造的回归测试。

验收：无结果不算成功；检查不能仅依赖模型自述或 Git HEAD；日志脱敏。

### Task 4：Jev 客户端和旁路接入

- [x] 核对官方 schema，通过 `/v1/models` 获取账号 `jev-latest/jev-preview`，实现 `/v1/systemone` 独立客户端；实际响应 jev-1.13.0。
- [x] 单 claim 的三项窄问题提交，校验 question ID/概率/必需 usage 计数；缺项拒绝响应。
- [x] 实现超时、取消、限额；当前不重试；复用用户环境凭证，禁止输出密钥。
- [x] 完成前挂接 shadow 模式，独立持久化结果和证据引用；配置关闭时行为兼容。
- [x] 替身覆盖 400/401/403/429/5xx/redirect、超时、畸形响应、缺项及取消；显式真实调用与实际 GUI smoke 有记录。

验收：协议测试与 Rust 集成测试通过；真实调用未做时明确保留未验证状态，不能用替身替代证明。

### Task 5：有限纠正和答案确认

- [x] 根据宿主批准的确定性类别触发补证据/修订，最多两轮，共用任务预算；生产类别空。
- [x] 修改完成状态和 GUI 流式展示，区分草稿、核验中、通过和未验证。
- [x] Rust 覆盖取消、耗尽预算、文件变化与 turn ID 恢复；GUI 实际验证正常完成与恢复，不补造 Pass。
- [x] 配置仅允许宿主显式批准的类别 enforce，保留 shadow/off；尚无生产启用类别。

验收：不存在无限校验循环、失败伪成功或修订答案沿用旧校验结果；UI/会话/轨迹状态一致。

### Task 6：效果验证与上线门槛

- [ ] 对比原框架、确定性规则、规则 + Jev 三组；主模型、任务和预算保持可比。
- [ ] 报告错误放行率、正确答案误拦截率、任务成功率、校验覆盖率、Unknown 比例、P50/P95 延迟与每任务增量成本。
- [ ] 按错误类别校准阈值并冻结策略；在独立留出集验证，报告样本量及不确定性。
- [x] 执行启用门槛：独立样本/真实任务对照不足，生产继续 shadow。阈值/误拦与成本上限仍未冻结，不能开启类别。
- [x] Rust/core/provider/GUI 回归、严格 fmt/clippy、Python baseline、前端 typecheck/build 与隔离 GUI 实际启动通过；详细次数与产物见结束记录。

验收：有对照证据证明效果，未达门槛不宣称减少幻觉；提交、构建和运行版本可追溯。

## 后续候选（未排入第一期）

- 工具多次失败、重复行动或方案切换时，校验候选行动与目标的一致性。
- 压缩提交和长期记忆写入前，校验事实/推断边界、撤销信息与最新约束。
- 根据实际收益决定是否扩展，不默认每个工具调用都增加模型请求。

## 开发记录

### 2026-10-10：GPT-6.1-Sol 可运行旁路交付与效果门槛

- 当前执行结果：候选提交前的 Rust v2 核验、来源/范围/快照规则、注入 transport、有限纠正、隔离持久化、GUI 实时状态/恢复已实现。P1 对窄声明可认证，任意语义和当前有效要求归并未认证；P3 真三组任务评测仍 not_run，生产 enforce 类别为空。
- 验证：core 398 passed / 1 ignored、provider 68 passed / 1 ignored；GUI 138 passed；相关 core/provider/CLI 与 GUI strict `-D warnings` Clippy、两工作区 fmt 通过；前端 protocol v3/70、tsc、155 modules build 通过；Python baseline 23 passed，新增 summarizer Ruff 通过。命令、失败修正与日志见 HANDOFF。
- 真实调用：18 条作者标注合成夹具 18/18 返回 jev-1.13.0，12,341 input / 1,026 output tokens，P50 852ms / P95 1001ms；原框架 not_run、任务成功率/费用 null。实际 GUI 两次各一请求（首轮恢复 selector 随自动标题变化失效，第二轮全断言通过），另 924 input / 114 output。当前批次共 20，请求台账 `jev_call_ledger_20261010.json`；此前阶段 2 smoke 另记，不重复刷分。
- 产物：新 CLI/GUI 在 `releases/jev-shadow-20261010-155325/`，manifest 记录 HEAD、原分支、工作树及 source/exe hashes；旧安装包保留，NSIS 未重建。runtime/restore smoke 以 localhost 主模型夹具运行，最终 verdict unknown；重启恢复不发送新 Jev 请求。
- 已知失败均修正：GUI 旧 noteTone 源码断言漂移；catalog_layers 复杂返回类型提取为类型别名；E2E 等待设置弹层关闭与去掉自动重命名后的旧标题 selector。费用重试计数如实保留，没有自动 Jev 重试。
- 未完成：真实轨迹样本与新独立留出集/标签复核、同主模型同预算原框架/规则/规则+Jev 对照、阈值与误拦收益认证、历史要求撤销/归并与完整语义覆盖。独立聊天的 trace_replay 是另行离线审计，不在本执行记录中假称已完成，更不替代真实任务三组对照。
- 配置与回滚：见 README 当前 v2。默认 off，显式 shadow；`NCX_JEV_MODE=off` 重启关闭，旧安装包回滚。仅返回模型概率不会使任务成为 Verified。

### 2026-10-10：GPT-6.1-Sol P0–P4 实施进展

- 起点 HEAD `17530133e49241664ed4d665252e9ac24282ea5b`，分支 `feat/context-association-memory`，63 dirty paths；原 WIP 保留，无提交推送发布。
- 已把核验前移到最终 commit 之前；候选失败最多两轮反馈，不持久化被拒绝草稿，重复问题停止；取消进入 Cancelled，预算/校验耗尽不进入 Completed。生产 enforce 尚未启用。
- 新增 host v2 contract、来源精确关联、32 条失败优先证据、bounded SHA-256 新鲜度、独立持久化、injectable transport 故障矩阵；Jev 逐 claim 请求，48KB/32KB、8s、每轮 3 次、无重试，沿用原任务 deadline/model budget。
- 已通过 core 398 / provider 68（各 1 个显式收费测试 ignored），相关严格 Clippy，前端 build 155 modules；GUI 初轮 135/136，有一个旧源码断言漂移，已修后复验中。精确命令/路径与 P0–P5 checkbox 见 HANDOFF 当前执行节。
- 18 条合成 Jev 回放执行中，尚无完成证据；真实任务三组对照/独立人工复核未做，不宣称减少幻觉。新版 CLI/GUI 尚未构建。

### 2026-10-10：Rust Jev 旁路首版

- 新增 provider `jev.rs`（官方接口、3 个窄问题、返回校验、8 秒超时、禁止重定向、安全错误）；core `verification.rs`（当轮有限证据、常见密钥脱敏、取消检查、观察落盘）；turn completed 分支接线。
- 配置采用现有进程环境入口，新增启动脚本 `scripts/with-jev-shadow.ps1`；第一把 key 保存于当前用户环境，不在仓库。默认 off，仅 shadow 有效。
- 新增协议拒绝畸形概率、当轮隔离/脱敏测试，以及两个默认 ignored 的真实 API 测试。provider/core lib 回归通过；两项真实测试显式运行通过。原会话消息未被校验修改。
- 范围收敛：使用独立 shadow schema，coverage、verdict 均为 unknown；没有实现逐结论抽取、文件新鲜度、完整 scope、阈值或 enforce。不能把本轮标成 Task 3/4 全部完成。
- 待完成：HTTP 失败矩阵、取消/预算全路径集成测试、真实任务对照及 GUI；旧安装包未更新。下一步把完整证据组装接入该旁路，再做三组基线。

### 2026-10-10：v1 契约与合成样本规则基线

- 新增 `evals/verification/contract.py`、`baseline.py`、`cases.json`、`README.md` 和报告 `reports/baseline_20261010.json`；新增 `tests/test_verification_baseline.py`。
- 已定稿：v1 数据契约、裁决优先级、缺失/失败/未知语义、模式和有限纠正策略。Rust 所有权已规划，实际接线未做。
- 数据：18 条作者标注的合成样本，8 条 calibration / 10 条 holdout；来源和标签未独立复核状态逐条记录。没有冒充真实会话回放，holdout 也不宣称独立盲测。
- 验证：`python -m pytest tests/test_verification_baseline.py -q`：23 passed；基线命令见 README，18/18 标签一致，11 个明确负例错误放行 0、3 个正例误拦截 0、4 个 unknown。
- 限制：基线假设宿主已正确生成 scope/coverage；全文抽取、Rust 端到端、真实 Jev 及成本/延迟尚未验证。Task 2 尚未整体完成，不能据此宣称幻觉下降。
- 下一步：标签独立复核与脱敏真实轨迹，再做原框架真实基线及 Task 3 证据组装。

### 2026-10-10：方案归档

- 完成：记录架构、第一期范围、现有接入点、契约草案、六项开发任务及验收标准。
- 证据：本次已读取 `turn.rs` 完成分支、`tool_call_detail`、`SourceRecord` 和现有评测 judge 协议。
- 未完成：所有开发 checkbox；无 API 调用、无模型校准、无功能测试、无运行逻辑变更。
- 下一步：Task 1 契约定稿，然后 Task 2 样本与基线。

后续每轮记录：日期、完成 checkbox、改动文件、验证命令与结果、未验证项/阻塞、下一步；仅在有证据时勾选完成，不用预计结果替代实测。

## 官方参考

- https://api.typesafe.ai/redoc
- https://typesafe.ai/blog/introducing-system-one-models-and-jev
- https://evals.typesafe.ai/

查询日期：2026-10-10。结构化类型保证不等于语义正确率保证；本项目效果待实测。
