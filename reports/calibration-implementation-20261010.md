# 零网络阈值校准与一次性留出工具

执行者：GPT-6.1-Sol；2026-10-10。代码为 `evals/verification/calibration.py`，测试为 `tests/test_verification_calibration.py`。本轮没有读取凭证、调用 Jev、构建 Rust 或修改生产配置。

## 可运行入口

```powershell
python -m evals.verification.calibration register --dataset dataset.json --protocol protocol.json --output registration.json
python -m evals.verification.calibration calibrate --dataset dataset.json --observations calibration.jsonl --registration registration.json --output calibration-artifact.json
python -m evals.verification.calibration calibrate --dataset development.json --observations development.jsonl --registration development-registration.json --output development-artifact.json --development
python -m evals.verification.calibration holdout --dataset dataset.json --observations holdout.jsonl --calibration calibration-artifact.json --receipt holdout-receipt.json
```

输出目录应已存在；注册、校准产物和留出收据使用 create-new，已有文件不会覆盖。命令失败退出码 2，输出明确 `not run` 原因。开发校准不能打开留出，未达到校准门槛也不会读取留出概率。

协议必需字段：

```json
{
  "model": "jev-1.13.0",
  "policy_version": "host-rules-v3.1",
  "template_version": "jev-single-claim-v2",
  "categories": ["semantic"],
  "threshold_grid": [0.9, 0.95, 0.99],
  "max_false_block_rate": 0.05,
  "confidence": 0.95,
  "min_supported": 60,
  "min_unsupported": 30,
  "min_paired_improvements": 5
}
```

该例是接口演示，不是已认证生产门槛。网格必须升序唯一，范围 [0.5,1]；最小样本按独立 group 计算，工具硬下限为30个 supported组、20个 unsupported组，协议可以要求更多。实际5%误拦上界/95%置信要求即使零误拦也需要至少59个独立 supported组。

dataset 使用 `buglecat.verification.review-dataset.v1`，每例必需：id/group_id/split/category/label/label_basis/review_status/source_kind/author_exposed。非 unlabeled 标签必须有非空依据。认证门槛另需 case `input_hash`、`candidate_hash`，与观察逐字段一致，以及 `context=narrative`、`polarity=positive`。可选 `original_verdict` 用于原框架A；缺任一例的原框架判定，A整体标 unavailable，不能默认全部通过。输入绑定 hash 是声明与观察的关联，不是工具对原始请求真实性的独立证明。

observations 是 JSONL，字段为 id/rules_verdict/call_status/model/policy_version/template_version/input_hash/candidate_hash/questions。每条成功观察必须包含 `contradiction` 和 `unsupported_success`，kind=noul，noul为有限 [0,1] 数值。`incomplete` 可保留，但不进入阈值策略。校准观察文件必须只含本次选中 split/category；不能包含留出观察。模型版本、策略、模板全体一致并匹配注册协议。缺标签、重复ID、缺观察、无成功响应、非法概率、分组跨split、混版本和 hash不一致拒绝运行。失败请求不携带概率，保留为失败记录并阻止门槛通过。

## 与 Rust 策略一致

已核对 `verification.rs::apply_experimental_policy`：仅语义类别、肯定 narrative、原判定 Unknown 可改变。依次检查 contradiction.noul≥threshold → contradiction；否则 unsupported_success.noul≥threshold → need_evidence。原有 Pass/矛盾/缺证据不被概率覆盖；低分和失败保持 Unknown。负向、代码和引用不应用拒绝信号。

事实四态独立计数。错误放行定义为 unsupported 且事实 Pass，其分母是有 supported/unsupported 标签中全部 unsupported例；C仅改变 Unknown，因此它不可能改善B的 false_pass。误拦定义为 supported 且最终 need_evidence/contradiction，其分母为全部 supported例，Unknown不算误拦。

同时报告固定候选宿主动作：Pass=deliver_verified，Unknown=deliver_unverified，矛盾/缺证据=correction_or_block。`unsupported_delivery_rate` 单独统计 unsupported 的 Pass+Unknown交付，不能与已验证准确率混称。unknown真值标签独立列出，不混入 supported/unsupported分母。

误拦单侧精确 Clopper-Pearson 上界按独立组计算，一个组内任一 supported误拦即该组误拦。配对 C>B 用新增 unsupported拒绝组与新增 supported误拦组的单侧 sign test；同组同时受益受损按受损，避免变体复制扩大增益。它仅表明固定候选拒绝判别，不能证明纠正后的任务成功、延迟、成本或闭环收益。

网格选择顺序预定：满足校准门槛的网格优先；新增 unsupported拒绝独立组最多者优先，平局取最高阈值。无合格网格时只给未认证建议，`calibration_gate_met=false`、`certified_threshold=null`、`threshold_status=candidate_not_calibrated`；开发输入始终false。合格时状态为 `calibrated_for_holdout_only`，仍不表示生产认证。全部输出始终 `production_authorized=false`，不会启用策略。

`input_binding_status=derived_local_fixture_not_recorded_request` 以及未记录的旧模板/旧Python夹具策略始终使门槛不合格；改 review_status 或暴露声明也无法使这些旧记录成为认证输入。

## 留出隔离与证据绑定

- 注册绑定整个 dataset、模型、模板、策略和预注册门槛 SHA256；校准产物另绑定观察、选择split、网格结果和阈值。数据变动、产物散列不符拒绝。
- 校准仅选 calibration，或显式选择 development；不使用 holdout概率选阈值。校准不得包含 holdout观察。
- 留出只测试固定阈值，不重新搜索网格。必须有人类/可执行断言复核声明，author_exposed=false，且输入/候选绑定完整。
- 在读取留出概率之前，在注册目录创建 `holdout-consumed-<dataset_hash>.json`。同注册目录、同数据即使换输出路径或改校准产物也不能再次读取。解析失败也消耗，并写 `failed_consumed` 收据；失败分母不被静默丢弃。
- SHA256和本地 create-new不能证明注册时间、独立标注或此前无人访问。复制整个目录、删除锁或虚假暴露声明超出本工具信任边界。已看过的数据不能称盲测；工具收据明确标为“声明未暴露，工具不能证明”。

## 验证结果与当前限制

- `python -m pytest tests/test_verification_calibration.py -q`：**41 passed in 0.34s**。
- `python -m ruff check evals/verification/calibration.py tests/test_verification_calibration.py`：**All checks passed**。
- CLI help与 subprocess校准路径通过。测试覆盖阈值优先序/不升Pass、Unknown动作、缺A、小样本置信界、同组变体、跨split、混版本/hash漂移、非法概率、无观察、未知/缺失标签、旧合成开发、非肯定引用代码、一轮留出及失败消耗。
- 测试中的大样本均为人工构造的算法夹具，用于验证门槛和收据流程，不是独立效果评测；测试 gate_met不代表真实标注或认证。
- 旧18例只能保留development，pending/author_exposed/synthetic均阻止认证。全部18例仍可保留在dataset；语义阈值只取semantic子集的观察。旧数据采用原有模型观察，只能演练接口。父代理已完成 register 和显式 development calibrate，旧18例中14 tests_passed、1 execution_success、1 limitation、2 semantic（其中1非断言）。语义子集两例未达到门槛；threshold=1.0仅同分取最高值的开发建议，不能称校准完成。新字段明确 certified_threshold=null。父代理拥有适配/实际演练产物，本执行者不覆盖这些文件。
- 目前没有独立人工完成标签、合格新校准样本、真实一次性留出结果或闭环任务收益认证。没有生成可用于开启生产的结论。

本工具实施结束；根代理继续标注队列、开发18例适配演练和交接文档。
