# Fixed-candidate replay：开发与主入口构建修正

执行：GPT-6.1-Sol，2026-10-10。本执行者新增真实 Jev 调用 **0**；父代理已执行1次冻结开发请求，不能因收据缺字段重新收费。

## 回放实施

- `rust/crates/ncx-core/examples/verification_replay.rs` 复用 Session::resume、原 sources 日志、只读原 SQLite事件视图及临时 ContextStore；调用真实 packet/policy，不手填scope、当前文件hash或用标签制造来源。
- 原6候选从父代理正式manifest原样读取。初次来源校验过严，旧 ToolRecovery 的空 session_id导致全部not_run；保留初次诊断，修正为仅权威来源跨session拒绝，旧旁路元数据不认证，各例保留1条不归属记录。
- 原candidate需确切存在于durable transcript。只有明确标记 assertion_projection且有加工依据时可回放投影，始终不称独立盲测。
- 当前 v3.2 只窄修复 `not success` / `not a success` 否定；混合句仍保留肯定分句。原6 raw eligible由4降至2，为同修复组的original/rules两个arm，其余真实失败/诚实限制不调用。
- 所有 host整体verdict/requirements/facts为Unknown；original修复选中窄声明有 exit0与旧exit1证据、真实scope=python check.py，但当前文件freshness=false，无supersession。规则不认证完整目标。`run passed` 未命中当前有界tests分类，因此保持Semantic；未为增加可判定率扩大分类器。
- 原6最终packet报告 `reports/replay-original6-packets-20261010-v32.jsonl`，SHA256 `dd46e6b64ef6f504bc3647433223442c2bf47995fe5b8186e3c74319c91b6b65`。
- live显式参数、串行、不重试、≤25尝试、同group最多1例且选中claim B必须Unknown。create-new独立run claim和尝试账本，attempt_started先sync再调用；崩溃或取消不抹去预算。

## 收据遗漏原因与保留证据

我在最后一次 `cargo run ... prepare` 主入口构建后新增 actual_request_hash字段，随后只跑 cargo test/clippy。cargo test生成的是独立test harness，clippy只做静态编译检查，均未重建普通main exe。父代理直接运行了旧main，因此真实 request_submitted收据未独立记录 actual_request_hash。这是本执行者构建交接遗漏。

重建前已复制旧main至 `reports/replay-binary-diagnostic-20261010/verification_replay-before-rebuild.exe`，`before-rebuild.json`保留完整事实：

- 旧main SHA256 `b9ba99046970923ec95133b5f1cf9cf56c546511dacf45d8f8b7c72daedf948d`，8,098,816 bytes，mtime UTC `2026-10-10T08:58:26.0968438Z`。
- 当时源码 SHA256 `f8e466179d1e3f0a3b96e9ec53c2a5167f5f9933141d173362c47f05e29cb516`，mtime UTC `2026-10-10T08:58:54.3416149Z`。
- archive SHA与旧main完全一致。该时间与现存字节支持旧binary诊断；收费日志没有当场binary摘要，不能单凭源码hash声称收费进程已认证为某一binary字节。

父代理既有1次请求为 jev-1.13.0，888 input/57 output tokens，约1011ms；概率 contradiction=.07、unsupported_success=.15、incomplete=.22。旧binary已有request equality guard，但actual transport hash未独立落盘。父代理保持 `actual_request_hash=null` / `actual_transport_hash_unrecorded_development_only`，只能用于明确开发演练；正式校准拒绝此缺陷。未补写旧日志、未重跑收费、未把本次离线收据替换旧真实收据。

## 真正主入口回归与新构建

新增 `offline-receipt-test <new-jsonl>` 主入口：不读取密钥、不实例化HTTP，内置离线transport返回明确offline model与零usage。它调用同一provider request/check_with和RecordedTransport，生成真实主exe的 attempt_started → request_submitted → attempt_finished。submitted保存从实际body计算的actual_request_hash；收据另绑定编译时例子源码SHA和真实exe SHA。

新增 `tests/test_verification_replay_binary.py` 调用真正main：

1. 实际body SHA与prepared hash一致、编译时源码/当前源码一致、exe字节hash一致，actual_request_hash字段存在；重复输出create-new拒绝。
2. 将子进程key显式清空后运行live，not_run/missing_key、0attempt、没有账本目录。

构建和验证命令：

```powershell
cargo build -p ncx-core --example verification_replay --manifest-path rust/Cargo.toml
python -m pytest tests/test_verification_replay_binary.py -q
cargo test -p ncx-core --example verification_replay --manifest-path rust/Cargo.toml
cargo clippy -p ncx-core --example verification_replay --manifest-path rust/Cargo.toml -- -D warnings
rustfmt --edition 2021 --check rust/crates/ncx-core/examples/verification_replay.rs
python -m ruff check tests/test_verification_replay_binary.py
```

结果：main binary测试 **2 passed in1.49s**，此前Rust example **7 passed**，v3.2 verification **28 passed /1 ignored**（付费ignored未运行）；最终strict Clippy/fmt/Ruff通过。

最终debug main SHA256：`7537b1144f0241ecdadfa9091ab19e50f86cd830f7da6e229a3ae90d01c59c23`。

最终例子源码 SHA256：`6c65f2e7583e5c9ddb393103d0a4de8354c0fcfc9a4cdc4d8e2bb3ed5eaebe7f`。

实际main离线收据 `reports/replay-binary-diagnostic-20261010/offline-main-receipt-final.jsonl`，SHA256 `3c678f90e0908646f740fa0f4c8922a6057b011dc59416c145179e148d996e93`；actual_request_hash=`25d193c3152db4b1416328dd9e7e69024fe8861a1d9f6e3dadad28b64d1f3c46`。`after-rebuild.json`保存最终binary大小/mtime/hash。该收据仅offline夹具，0网络、0付费，不是新的真实观察。

## 边界

生产categories/阈值未开启；已发布v3.1 GUI/CLI及manifest字节未改变。本例独立持有CLI、只读快照、durable输送和内联测试，单文件已超过skill建议长度；本轮保留这一显式隔离例以完成可复验交付，没有扩散产品实现，后续若继续扩展应按加载/输送拆模块。新label队列、独立留出与正式阈值认证仍由父代理推进，不把开发请求或构造fixture当独立效果证明。

本执行者实施结束。后续真实请求须使用已构建main，并先通过实际入口回归；不会继续收费重试。
