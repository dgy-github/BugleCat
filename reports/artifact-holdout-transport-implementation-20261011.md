# 独立留出 main / Jev 入口实施记录（2026-10-11）

执行模型：GPT-6.1-Sol。执行结束，两个新入口和原始源码/二进制已单独封存。本轮 **HTTP 0、付费 0、实际全局预约/启动 0、真实留出打开 0**。Root 是实际登记与收费操作者。

## 实现与门禁

- 新 `evals/verification/artifact_holdout_transport_guard.py` 只读组合既有 `artifact_holdout.validate`、`artifact_formal.revalidate_calibration`、原始 main/Oracle/transport 验真、固定数值规则；没有另造校准统计、标签、注册或预算。缓存仅在单个 `source_transaction` 内复用不可变 source/catalog 证明，每次命中仍重新 hash 源绑定，不缓存 ledger、候选或概率。
- 新 main `verification_artifact_holdout_collect.rs` 及独立 engine/support 验证真实 `split=holdout`，复用封存的 high-profile、原 SYSTEM/public user 模板和 SQLite Ledger。prepare/live 初始检查固定下一未启动批次（最多 8）；每次发送前重算门禁并要求当前组是下一未启动源，前面同批成员已启动。重试/旧组、已冻结标签后的追加 main、被重设的旧 ledger 均拒绝。
- 新 Jev `verification_artifact_holdout_formal.rs` 及独立 engine/support/formal 在 key/HTTP/启动 marker 前要求原始一次性 lock、合格 calibration、原始 registration/observations raw 与 canonical hashes、同 profile 三字段、原始 SQLite start/finish、候选/Oracle/实际主模型源图、150 supported + 50 unsupported 标签配额。Unknown 保留独立全分母；不充负例，不晋升 Pass；阈值不可重选，`.evaluation.started` 后拒绝再送。
- 共享 proof subprocess 300 秒 / 128 KB 上限，清除继承的 API 凭证，丢弃 stderr；超时/非零/源码变更/无效封存 proof 均在 HTTP 前失败关闭。300 秒仅是只读证明预算；主模型 high/16384/120 秒、Jev 8 秒 / 48 KB 输入 / 32 KB 响应保持不变，无自动重试。
- main 继续写原 `prepared-main` / `collected-main` / session/sources/native packet/events；Jev 继续单 `acceptance_violation` NOUL、实际提交 body hash、2xx 原 bytes create-new/fsync 后 parse、model/usage 校验和共享 500 上限/唯一 group 预约。非 2xx 不保存 body，取消/崩溃保留已占位且不可重试。
- 两入口 `build_info` 记录新 wrapper/engine/validator 和真实 exe，追加 8 个 guard/owner 字段（含 `source_binding_owner_source_sha256`）及 `profile_collector_validator_source_sha256`。Jev 另外记录新 holdout main wrapper/engine，旧 profile main wrapper/engine 保留。Root exact filename dispatch 已对应这些字段。

## 可运行入口

从仓库根目录使用 `rust/target/debug/examples/` 下的两个 **实际主二进制**（不是 Rust test exe）。`--live` 只供 Root 在真实资格成立后显式调用；当前没有创建合格留出输入。

```powershell
# 只读原资格证明；没有注册、预约或 HTTP
verification_artifact_holdout_collect.exe check-gate <holdout-manifest.json> [selected-id]
verification_artifact_holdout_formal.exe check-gate <prepared.jsonl> <id> <holdout-registration.json>

# main 默认 prepare 零网络；真实 source cohort 必须已由 Root 合格登记
verification_artifact_holdout_collect.exe prepare <holdout-manifest.json> <new-prepared-dir>
verification_artifact_holdout_collect.exe live <prepared-dir> --live [--cancel-file path]

# Jev 每次一个原始 prepared，不批量造样本
verification_artifact_holdout_formal.exe check-source <prepared.jsonl> <id>
verification_artifact_holdout_formal.exe check-registration <prepared.jsonl> <id> <registration.json>
verification_artifact_holdout_formal.exe validate <prepared.jsonl> <id> <registration.json> <new-output.json>
verification_artifact_holdout_formal.exe live <prepared.jsonl> <id> <registration.json> <reservation.json> <newdir> --live [--cancel-file path]

# 默认无需 key 的实际 main fake transport
verification_artifact_holdout_collect.exe offline-test <newdir> success
verification_artifact_holdout_formal.exe offline-transport-test <newdir> success
```

Python 只读 guard 独立接口：`python -m evals.verification.artifact_holdout_transport_guard main <manifest> [id]` / `jev <prepared> <id> <registration>`。失败只返回 `holdout_transport_gate_rejected`，不输出原异常内容。

## 直接验证与结果

全部来自这两个新 actual-main；未重建旧付费 collector/replay 或 CLI/GUI/NSIS 包。

- `python -m pytest tests/test_verification_artifact_holdout_transport_guard.py -q`：**8 passed**。前缀/同批后续组、乱序、旧组、原 owner 失败、标签已注册后关闭、旧真实 calibration 拒绝、150/50 配额、原 Oracle 标签与消费锁。正向组合测试明确 mock 原 owner，仅代码夹具，不是假造真实认证。
- `python -m pytest tests/test_verification_artifact_holdout_binary.py -q`：**22 passed in 7.65s**。main 7 类成功/失败/格式/model/usage/timeout/中途取消；主模型预启动取消、高 profile body；Jev 9 类 fake 成功/格式/model/usage/多答案/HTTP500/前后取消/timeout；原 bytes/request/exe/source/usage join、不可重复 marker、旧 calibration 与 resealed holdout 拒绝、标签门槛不足先拒、缺显式 live 拒绝。所有测试进程 key 为空，fake ledger 为隔离目录。
- `cargo test -p ncx-core --example verification_artifact_holdout_collect --example verification_artifact_holdout_formal`：**7 + 10 passed**，主要复用既有 prompt/ledger/body/response 不变量；没有把这些 unit tests 冒充 GUI 或真实合格校准。
- `cargo clippy -p ncx-core --example verification_artifact_holdout_collect --example verification_artifact_holdout_formal -- -D warnings`：通过。
- `cargo build -p ncx-core --example verification_artifact_holdout_collect --example verification_artifact_holdout_formal`：通过，之后运行上述 actual-main binary tests。
- 新 8 个 Rust 文件 `rustfmt --edition 2021 --config skip_children=true --check ...`：通过；guard 与两新 Python 测试文件 `python -m ruff check ...`：通过。

持久零网络样例：

- `reports/artifact-holdout-main-binary-success-20261011/`：原 session/sources/native packet/request/response/SQLite/events/receipt；**development fixture** 的 35 旧槽 → 36 / 剩 464 是 fake ledger，不是当前真实账户/授权台账。
- `reports/artifact-holdout-jev-binary-success-20261011/attempt/`：原 bytes/events/receipt；`offline=true`、`formal_registration_hash=offline-fixture-not-formal`；一次 fake transport、付费 0。

## 封存

目录 `reports/artifact-holdout-frozen-20261011/` 保存两个实际 exe、所有编译/owner 源字段副本、测试与 `manifest.json`。

- manifest raw SHA256：`d0b5e6826ed45180f1f471798260153aa6a69903e0a8fb61427323bd8166b578`
- manifest canonical seal：`c5b2f7269b48350a61b6d14fbbeb4d214a97169084177c6840754fc0d5433cff`
- main exe：**9,314,304 bytes**，SHA256 `cc89f2d91aea2d72b2f8b847d6e183b3cc49eafacec1a897dac72df857fc6c2a`；wrapper SHA256 `f16fba233f9369fe9aca395e58b259883bc66c1519a4846a5c2b6912aeead284`。
- Jev exe：**8,258,048 bytes**，SHA256 `de3de56c17efb166ad5396a28578e299e12034041508216f3d61969aab72e62a`；wrapper SHA256 `7f62bb383a3d82e3db258e51e7b0d5de9110e78a134192280b0c48a07c556852`。
- Python guard：`40796e400aea2ffa40b17dfe71411d549789704561532bea339e61b619b30f24`；Rust guard：`873b104a819ec2067b0beb31487223e5bdad0f8a84b56da2acd10a5c7e47d44a`。
- source binding owner：`6f79873d37947adb6de2ba7bff5d24d0eafc2d2814c027f0e919365fb1dea4f0`。其余精确 compiled/source hashes 在封存 manifest，已逐个对 actual fake receipt 与当前文件核对。
- 原 **7 个付费/开发例子 exe**、`provider/request.rs`、旧封存 profile wrapper/engine/validator 和 v3.3 发布包保持原字节；归档脚本对全部旧 exe 原 hash 做实际断言。

## 尚未验证 / 认证边界

真实合格 calibration / 150 正 50 负 / 一次性 holdout 输入尚未成立，因此 **真实正向 qualification + 全量 proof 性能 + 正式 holdout main/Jev transport 尚未运行**。300 秒是有界实现上限，不是完成 200 组正路径性能承诺。Root 将在真资格成立后先零费重算预检，再决定独立登记/收费。

本交付只准备被冻结 benchmark 的代码产物轨道；不认证原生声明、一般算法正确性、部署泛化或 AgentLoop 闭环收益。Unknown 单列、不算 Pass。生产 off / categories 空 / threshold null 保持；新 sidecars 没有被冒充包含在先前已 seal 的 v3.3 CLI/GUI/NSIS 源图。无 commit/push/publish。
