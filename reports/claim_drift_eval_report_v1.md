# 声明核验规则层合成回归报告（v2）

本报告直接评测预先构造的声明与证据；不覆盖声明抽取、真实工具证据组装或 Agent 纠正流程。
这些结果不构成独立留出认证或生产准确率保证。

- 样本数：175
- 明确干预的漂移样本：140/140
- 合规样本干预数：0/35
- 裁决匹配：175/175
- 原因匹配：175/175
- 规则求值耗时：41 ms（不含证据采集和网络）

检出和误拦使用同一口径：NeedEvidence 或 Contradiction；Unknown 不算检出。
以上为样本内经验比例，未据此认证误拦率上界。

- compliant_failure_report：裁决匹配 10/10，原因匹配 10/10
- compliant_honest_limitation：裁决匹配 10/10，原因匹配 10/10
- compliant_supported：裁决匹配 15/15，原因匹配 15/15
- drift_denied_execution：裁决匹配 15/15，原因匹配 15/15
- drift_explicit_failure：裁决匹配 20/20，原因匹配 20/20
- drift_foreign_evidence：裁决匹配 15/15，原因匹配 15/15
- drift_incomplete_evidence：裁决匹配 15/15，原因匹配 15/15
- drift_missing_evidence：裁决匹配 20/20，原因匹配 20/20
- drift_multi_claim_mixed：裁决匹配 15/15，原因匹配 15/15
- drift_scope_mismatch：裁决匹配 20/20，原因匹配 20/20
- drift_stale_evidence：裁决匹配 20/20，原因匹配 20/20
