# MBPP 单请求采集入口与原始收据审计

执行模型：GPT-6.1-Sol。开发、prepare 与测试阶段由本代理执行，HTTP / 付费调用为 0。真实首批请求由主代理统一协调，不计作此阶段的离线验证。没有修改生产开关、阈值、HANDOFF 顶节、冻结的 source/catalog/authorization/protocol、既有付费收据或 trace 保留路径。

## 本次实现

- `rust/crates/ncx-core/examples/verification_collect.rs` 与 `verification_collect/support.rs`：真实 OpenAI-compatible 单请求采集，不执行 AgentLoop 或工具，不调用 Jev。
- Core 的 dev-dependencies 增加已在 Provider 使用的 reqwest；对应 Cargo.lock 只增加 Core 的依赖边，不修改生产逻辑。
- `tests/test_verification_collect_binary.py`：实际已构建 main binary 的离线入口测试，避免 test/check executable 与 main binary 不同导致陈旧收据。
- `verification_replay.rs` 与原 binary 回归：2xx 原始 Jev response bytes 以 create-new / fsync 保存，供独立检查 parsed probabilities 与原始 transport 收据一致。

## CLI 与冻结输入

```powershell
cargo build -p ncx-core --example verification_collect --manifest-path rust/Cargo.toml
rust/target/debug/examples/verification_collect.exe prepare <manifest.json> <new-directory>
# 以下仅主代理在已授权且准备完成时显式调用；本代理未执行
rust/target/debug/examples/verification_collect.exe live <prepared-directory> --live
# 可选 --cancel-file <path>；取消或超时不重试
rust/target/debug/examples/verification_collect.exe offline-receipt-test <new-directory>
rust/target/debug/examples/verification_collect.exe offline-failure-test <new-directory>
```

输入 schema：`buglecat.verification.mbpp-collection.v1`。字段为 source_file/catalog_file/authorization_file/protocol_file 的绝对路径及各对应 SHA256，model/reasoning_effort/max_tokens/timeout_s，cases 包含 id/task_id/group_id/split/public_interface。来源固定到官方 MBPP commit 与文件 hash，验证 catalog 的 group、split 与 prompt hash。公共接口只发送函数签名，不发送 reference body、test_list、test_imports 或预期输出。

prepare 复用 ncx-config 的实际选定 provider/model 设置，冻结现有 request builder 的真实 JSON body；只支持 OpenAI-compatible 协议。ultra 在 DeepSeek 请求中实际映射 `thinking.enabled` 与 `reasoning_effort=max`。默认路径不构造 HTTP client，live 必须显式 `--live`。

当前入口只接受 calibration；holdout 入口需要在合格 calibration gate 后协调扩展，不提前开放。当前公共签名验证是无默认表达式的子集，遇有 literal defaults 的后续 manifest 会在请求前拒绝，需要协调正式接口扩展后才能采集，不能忽略该限制或强行改模型输出。

## 预算与失败状态

授权文件 SHA256 `d1c5a4ccf695dbccc1777af5123441bbd19aebe39da2a4d3e848abf30f6af0f2` 编译绑定。SQLite 位于授权的 main_model.ledger_dir；全局最多 400 started reservations，每 split 最多 200，group_id UNIQUE，首次批量最多 8、后续单批最多 25。多个新的 prepared batch 共享该数据库。

`BEGIN IMMEDIATE` + synchronous FULL 的占位在 HTTP 之前提交，started / submitted / finished JSONL 收据 fsync；失败、超时、取消与不完整输出仍占位并锁定该组。没有自动重试。每个 prepared directory 的 live 输出 create-new、不 resume。SQLite 同时锁定实际主模型设置、来源/协议、endpoint hash 与编译源码/executable hash；若批间设置或构建改变，拒绝继续混入同一采集批次。

首次 HTTP 故障立即停止未开始的余组；连续两次相同非成功状态也停止。正常返回的代码是否通过断言由独立 Oracle 判定，不等于 provider 故障。取消后保留已经开始的占位。

## 输出契约

- `prepared.jsonl`：`buglecat.verification.prepared-main.v1`，包括 source/catalog/authorization/protocol/manifest hashes、真实 prompt、body/request_hash、settings、compiled source/exe hashes、绝对 workspace/session_log、不可变 row seal。
- `attempt-events.jsonl`：每例 attempt_started → request_submitted → attempt_finished；SQLite 可能保留 started 而无 finished 的中断记录，必须计入预算。submitted 的 actual_request_hash 来自实际发送的序列化 bytes。
- 每例目录有 `request.json`、2xx `raw-response.json`、原文 `candidate.txt`、`receipt.json`、原生 packet/host-B。
- `receipt.json` schema `buglecat.verification.collected-main.v1`：顶层身份、request/actual/response hash、settings/build_info、call_status、attempt_ordinal；`result` 对象保存 original candidate/candidate_sha256、confirmed_model/原始 usage、session_log/session/source SHA、events 文件、workspace、claims（精确 spans）与 host_B。
- 会话在 HTTP 前通过真实 Session 保存 user、SystemPrompt source 和 UserMessage event；成功响应追加真实 assistant，未请求的工具调用保留为未执行调用，不补造 tool result。
- tools_executed=0、agent_loop_executed=false、assertions_executed=false、label=unlabeled。失败不制造候选或成功标签；2xx 非法 JSON 可留原始 bytes，non2xx body 不保存。

单请求只生成代码与模型自己的声明。prompt 允许诚实不确定，不强制肯定，也不由宿主后补肯定句。原生提取可能产生 TestsPassed、Limitation 或非断言；没有 eligible positive narrative Semantic 时不调用 Jev。独立冻结的少量 acceptance assertions 只能认证明确限定的 benchmark scope，不证明完整算法正确、部署泛化或 AgentLoop 闭环收益。

## 实际构建与离线验证

Collector：8 Rust tests passed，strict Clippy passed，main binary 实际 build 完成；实际 binary Python 回归 3 passed / Ruff passed。覆盖请求不含 hidden source/tests、精确 request/raw/candidate/source/exe hash、真实会话和 Unknown packet、失败占位持久化、重复 group/预算、无重试、timeout/cancel、create-new、缺显式 live 不运行。

Collector main SHA256：`8a1612e43893deea875bc9a94eb8a7ad1e084f38b912b82753792a49e8627ce9`。

Collector 源码 SHA256：`0ad55c036256468e9a36e1edbb01cd3767fc9cd93ed302177525dc51e8ae64e1`；support SHA256：`656cc541eaac8b5aa8d57c47df226aa093563109613212953c6aa5a3b5bad371`。

实际离线 main 收据：`reports/collector-diagnostic-20261010-success-r2/receipt.json`。此为明确 development fake transport fixture，不能用于正式样本标签或认证。

正式首8零费 prepare：`reports/benchmark-collection-batch01-20261010-r2/prepared.jsonl`，SHA256 `ad29af09076e27bbf21099a497ec0cc2d81e87bc6fbd9bbb991e831bdc901407`。首版 prepared 与 fixture 保留为未付费诊断，只有 r2 给主代理 green。

Replay：8 Rust tests / strict Clippy passed，实际 main 已重新 build；collector + replay 实际 binary pytest 共 5 passed，Ruff passed。Replay 源码 SHA256 `044443747479d834970b16e72d8f673f8b4b947f0fb03d18a43ae772de01f3b1`，exe SHA256 `a7a81112a2acdb51fca6428a6d7e5ec2a44e69ae3d3c2edf09aa3c4f1b755a7c`。

Replay raw 路径为 ledger_dir/attempt-01.raw-response.json；submitted 保存 planned_raw_response_file，finished 保存实际 raw_response_file/response_sha256。已有 raw 文件在请求前拒绝，取消/non2xx 不保存 body。实际零费入口收据 `reports/replay-binary-diagnostic-20261010/offline-main-receipt-raw2xx.jsonl` SHA256 `ca06ec6bfc1496c16da42080f60300c4d2df2d7e9c484dfc59bf6bcbf09591b8`；独立 raw SHA256 `5611dc7c92c97cc7d2b8f1971d791c19fa6f66df39ca068a38b8f25ea8328b3c`。

## 运行中发现的策略问题

主代理报告首真实响应中的“No tests were executed”被 v3.2 原生提取错误判为正向 TestsPassed。此处只记录缺陷，保持当前采集器、生产策略与既有 v3.2 原始 packet 冻结。之后如需修复，必须另版本保存并用原始响应重新 source-bound replay；不能覆盖旧记录、强迫 positive 或按已看概率补齐负例。

独立原始 transport 文件与本地 compiled hash 可验证落盘一致性，不能证明远端 provider 身份，也不能将作者暴露的 fixture 转成盲测。正式 labeling / intake / calibration / Jev 全局授权与收费仍由主代理负责。
