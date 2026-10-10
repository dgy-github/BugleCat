# R1 / R2 / R4 实施记录（GPT-6.1-Sol）

日期：2026-10-10。本轮继承原有脏工作树；未 reset、commit、push、publish。R3 与 HANDOFF/计划由父代理记录。本报告只记录本执行者的 Rust 和 GUI 工作；本轮本执行者 Jev 收费调用数 **0**。

## 已实现的产品路径

- 新增 `verification/{assembly,provenance,requirements,extraction}.rs`，扩展既有 Session、ContextStore、SourceTraceStore、InstructionLedger，未另建存储系统。
- RuntimeVerification 包含实际校验工作区、session/turn、校验器、目标路径、完整 SHA256、状态、时间与来源。Python `-I -S` 编译校验记录真正送入编译器的字节 hash，并检测检查期间文件变化。当前只认可 `python.compile.-I.-S` 的 `python_syntax_only` 范围；它不认证功能测试。旧记录缺元数据仍供审计，不能变成权威通过。
- tool_call/result 以同 session、turn、workspace、事件序列、调用 hash、返回 hash 和 trace 关联。宿主保存实际 workdir、目标路径与 invocation identity（包含完整工具参数）。路径越界、symlink/junction、缺目录、裁剪、重复 call_id 均不能认证。重复 call_id 的调用/返回事件 ID 不再被去重丢弃；不完整协议维持不可信。
- 前后文件快照写入原事件库。成功认证只接受持久快照，未依靠易丢失的内存 snapshot 提升通过。快照排除当前会话与生成目录，包含未跟踪源文件；不完整读取、链接、竞态降级。
- 同一精确 invocation、workdir、scope 的后续成功，且成功检查的前后/当前版本一致，可以标记旧失败 `superseded_by`；旧失败保留审计。只引用旧失败仍否决。不同目标/范围不相互抵消；时间性“最初失败”目前保持 Unknown。
- 普通用户答案不需要复述完整命令，可由宿主在唯一目标组中关联。`[call:...]` / `[evidence:...]` 是待核验候选，伪造 ID 不能偷偷退回自动关联。跨工作区证据不能使用。
- 原用户事件保存脱敏文本、原内容 hash、多段文本与附件元数据，稳定 source ID 不随压缩消息下标复用。依据原事件重建 InstructionLedger 的 Active / Superseded / Revoked / Conflicted，仅相同明确主体的用户改口可以替换。工具内容、引用、代码块不撤销限制；附件内容、模糊或未解析要求均留原因与 Unknown，压缩视图不是完整来源证据。
- 声明按 UTF8 span 拆成正向、否定、限定、代码/引文部分。局部测试、非 shell read 成功、明确执行失败等按规则判断；不识别的语义保持 Unknown。`The final check passed` 可关联唯一实际 `python check.py` 范围，不能冒充全部测试。
- `claim_verdict` 与整个任务 `verdict` 分离。候选文本覆盖、事实分类覆盖、用户要求认证分开；`coverage.requirement_sources` 表示历史来源/解析完整度，输出 `coverage.requirements` 表示真实目标认证。当前完整 Pass 仅支持有持久证据的简单 `Run <直接测试命令>` 目标；广义修复正确性仍不认证。
- Jev 只尝试尚需语义判断的肯定性 narrative，确定性判断与诚实未知/代码无需付费请求。`attempted_calls` 包含失败、取消尝试，和 observations 分开。取消回调包含原 turn deadline；取消后仍保存尝试记录。
- 新增显式实验策略接口 `ExperimentalJevPolicy`，版本必须明确、阈值有限且在 [0.5,1]。只能将 Unknown 变成实验性拒绝信号，不能提升 Pass，不能覆盖确定性失败/权限。生产 `from_env()` 的策略 None、enforce categories 空。最多两次纠正仍共享 turn 的模型/工具预算和 deadline。

## 验证记录

1. 初次 `cargo check -p ncx-core --lib` 暴露 iterator 类型和 EventType exhaustive match 错误，已修正。
2. 原 verification 测试初次 12 passed / 4 failed / 1 ignored：旧断言仍假设不拆混合句、只最后一条要求、每个确定性声明都发 Jev，以及“已执行但回答未执行”不矛盾。更新为对应新语义，并新增实际行为回归。
3. Runtime 新回归发现测试夹具误将16位来源短 hash 当成完整文件 SHA256；修正夹具，保持产品完整 SHA256 条件。
4. Core 全套：`cargo test -p ncx-core --lib --manifest-path rust/Cargo.toml`，406 passed / 1 ignored（增加最后的重复ID回归前）。日志 `reports/r1r2-core-full.log`。
5. 最终窄回归：`cargo test -p ncx-core --lib verification --manifest-path rust/Cargo.toml`，25 passed / 1 ignored，包含重复ID、错误turn关闭pending、同目标修复、不同范围、普通声明、native无exitcode、runtime旧数据/新鲜度/工作区、要求压缩/改口、代码/否定、deadline实际尝试和实验策略。日志 `reports/r1r2-verification-final.log`。
6. `cargo check -p ncx-core --example verification_trial --manifest-path rust/Cargo.toml` 退出0；父代理评测适配器可编译，未编辑其文件。日志 `reports/r1r2-example-check.log`。
7. strict core Clippy 初次发现重复 if 分支、循环创建 regex，已修正；`cargo clippy -p ncx-core --lib ... -- -D warnings` 退出0。日志 `reports/r1r2-clippy.log`。修改文件已按 Rust2021 格式化。
8. GUI 实机脚本新增 `rust/gui/e2e/verification-lifecycle.mjs`；Node syntax check 退出0。首次探索使用此前 release 二进制，三种独立进程配置 off / shadow / enforce 均通过：一次真实 apply_patch 写入后本地主模型慢响应中途取消；恢复不重写；主模型断网；核验日志写入故障（shadow/enforce显示故障）；无“规则核验通过”，持久 trace 一次写调用。12次本地主模型请求，0 Jev。此结果是脚本/既有包探索证据，不能代替新构建复验。

## 评测前交接状态（历史）

Rust crate sources 已冻结供父代理三组开发集评测；最终全套回归、新 release 构建及新二进制 GUI 复验尚待该评测结束后执行。任何主模型或 Jev 效果数字、计费总账、阈值认证、独立留出状态由 R3 报告给出，不由以上 unit tests 推断。

## 评测反馈后修复（host-rules-v3 → v3.1）

父代理已完成 r2 开发集三组，发现 C 的修复任务虽然独立判分通过，但被规则误拦。实际初始失败 shell 与 apply_patch 在同一模型响应 batch，旧检查没有 snapshot；原 supersession 条件错误要求旧失败 snapshot 完整。当前移除该条件，仅认可**旧 invocation 来源权威 + 同一精确调用/workdir/scope + 新成功前后和当前文件版本完整一致**。这认证当前重跑，不能认证未知旧文件版本；显式旧失败引用仍否决。新增两调用batch回归同时验证修复可恢复与后续文件修改重新失效。没有收费重跑，r2 数据继续对应 v3。

- 不执行测试的 `--help`、`--list`、`--collect-only`、`--dry-run`、`--no-run` 等旗标不形成 TestsPassed 范围；保留正常 `pytest -v`。
- `Run ... and do not ...` 等内嵌要求增加 `embedded_requirement_unparsed`，不能声称已完整解析。
- 语义调用优先整项完成声明，再按明确宿主来源/原叙述顺序；预算为1的 fake transport 回归确认关键完成声明先被审核。
- 原工具 fallback 的 `[recovery:]` 返回不能假作原工具的真实执行；既有 recovery 记录缺调用身份时 Unknown/NeedEvidence。实际 substitute 的完整身份关联仍不认证。
- 策略版本升级 `host-rules-v3.1`，与付费 r2 的 `host-rules-v3` 明确分离。

最终完整回归（包括以上修复）：core **411 passed / 1 ignored**；provider **68 passed / 1 ignored**；GUI **138 passed**。两处 ignored 都是显式付费 live 请求，不在本轮运行。core/provider/CLI 及 GUI strict Clippy 退出0；两套 fmt check 退出0；前端 build/typecheck 退出0（155 modules），protocol checks70 methods / tests5 passed。

最终日志：`reports/r1r2-core-final.log`、`r1r2-provider-final.log`、`r1r2-clippy-final.log`、`r1r2-fmt-final.log`、`r4-gui-tests.log`、`r4-gui-clippy.log`、`r4-fmt-final.log`、`r4-frontend-build.log`、`r4-protocol.log`。CLI release 编译已完成，GUI release 进行中；实机新包验证将在构建结束后补记。

## 有界能力与未认证项

- 任意人类需求、附件内容、复杂许可归并、任意语义正确性没有完整认证；显式前缀和精确主体之外不猜授权。
- temporal 历史叙述的时间区间解析、跨 target 合并、多目标无显式引用、parallel batch 缺快照会保持 Unknown/NeedEvidence；未实现任意自然语言到验证计划的映射。
- 当前仅批准有限 checker/scope；没有由概率产生 Verified。生产默认 Off，Shadow/Enforce 环境模式仍没有可启用的生产类别与 Jev 阈值。
- 生命周期脚本取消的是工具写入后主模型等待阶段；Jev checking 中途取消由真实异步 transport 单元/集成路径验证，本轮不为GUI请求额外收费。
- 未构建安装器，未做安装/升级验收。不得将新exe本地构建替代安装器认证。

## 执行结束（本执行者）

本轮 R1/R2 的有界产品实现、对应回归、R4 本地 exe 构建与新包实机验收已完成；HANDOFF/计划 checkbox 和 R3认证状态由父代理统一更新。没有自动 commit/push/publish。本执行者本轮新增 Jev 收费调用 **0**。父代理给出的累计台账26/30只包含其4次新开发集调用，不是本实机检查产生。

最终新包：`D:\github_dgy\BugleCat\releases\jev-r1r2-20261010-164839`。

- `ncx.exe`：8,114,688 bytes；SHA256 `4c281475df12bf58d7c2640e644e227148d3f8a0488eebe7b6b19614504cec5e`。
- `BugleCat.exe`：14,637,056 bytes；SHA256 `416db3963b1b36a9e0f51341a2ee9ae3a222c792df7c12e7a3303ff36838c6bd`。
- `manifest.json`：策略 `host-rules-v3.1`、默认 Off、生产 enforce categories 空、生产 Jev threshold null；完整 source graph SHA256 `203b80518a859e9dad5a750682aeb5fdd96917759663051067ef4552d01ea205`（与评测 runner 相同 crates hash 算法），另含 GUI source SHA、dirty来源、HEAD、测试与smoke收据。旧 release 保留。
- 构建：CLI `cargo build -p ncx-cli --release --manifest-path rust/Cargo.toml`，退出0，1m34s；GUI `npm run tauri:build -- --no-bundle`，退出0，Rust release2m42s。日志 `reports/r4-cli-build.log`、`r4-gui-build.log`。打包脚本 `reports/stage_jev_r1r2_release.py` 与收据 `r4-stage.log` 可复核来源算法。
- 新 staged GUI 实机：`node rust/gui/e2e/verification-lifecycle.mjs`，退出0。`reports/gui-lifecycle-new-20261010.json` 包含实际 exe SHA，与 manifest 完全相同；3模式各4次 localhost 主模型请求、各1次真实 apply_patch，取消后恢复未重写，断网未通过，shadow/enforce持久化故障可见，最终写文件仍仅 `once`，Jev0。日志 `gui-lifecycle-new-run.log`，6张 `gui-cancel/fault-{off,shadow,enforce}-20261010.png` 是新包截图。
- 新 staged CLI/GUI 正常+恢复：`node rust/gui/e2e/verification.mjs` 无 `--live`，退出0；CLI退出0，GUI未验证标签可见，reload后“有历史核验记录 · 当前未验证”可见，Jev0。收据 `reports/runtime-smoke-new-20261010.json`、日志 `runtime-smoke-new-run.log` 与3张 `jev_gui*_20261010_r1r2.png`。
- 更新原 smoke 的默认报告/截图目录到根 `reports`，避免改父代理评测报告；未来显式 `--live` 改用一个未认证肯定语义声明，因为诚实未执行声明按新策略应跳过付费调用。本轮没有运行 `--live`。

仍未完成的是以上“有界能力与未认证项”以及独立冻结留出效果认证/生产类别开放/安装器。不得因为 unit tests 或新 exe 实机通过，将它们勾成效果已认证。修复后完整源码不同于 r2 三组付费评测；未重新付费运行三组，也没有宣称新版 C 优于 B。

### 最终元数据收尾

仅维护 `cfg(test)` 下的 ignored 付费 smoke：按新策略，一个纯确定性声明不应发 Jev，故改为 fixture 工作区直接组包、一个确定性失败声明加一个肯定语义声明，再保存独立记录。未执行该付费测试。之后 core 全套仍 **411 passed / 1 ignored**，格式检查退出0，CLI release cache复核退出0（0.25s）。额外 plain Cargo GUI cache检查未带 Tauri CLI 的生产 feature 配置，已取消，不作为构建收据；本次交付仍是此前正确 `tauri:build --no-bundle` 构建并实机验证的 staged 文件。

因测试源码变更，manifest 刷新 source graph SHA256 为 **`ae237f63f0a1348ed6cf417533a4bc697b571ee326bf67e53df98d8b3c0227a3`**，保留 `previous_source_sha256=203b80518a859e9dad5a750682aeb5fdd96917759663051067ef4552d01ea205` 和刷新原因。生产依赖源码未改变，两份 **staged exe 字节/hash未改变**，GUI报告仍与 manifest同一hash匹配。打包脚本在 refresh 时只核对既有文件，未覆盖已验证 exe。新增调用依然0；未扩大验证或重新付费回放。
