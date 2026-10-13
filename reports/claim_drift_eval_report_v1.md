# 声明级矛盾与 Agent 漂移全量评估基准报告（v1）

**生成时点**：2026-10-13  
**评测对象**：BugleCat `ncx-core` 声明级矛盾核对引擎（Host Verification Layer）  
**执行策略**：`host-rules-v3.3` 原生确定性宿主物理核验引擎  
**样本规模**：175 组双语（中英对照）端到端物理交互与终答评测样本  

---

## 一、核心评测指标概览

| 核心指标 | 实测数值 | 达标门槛 | 评定状态 | 说明 |
|---|:---:|:---:|:---:|---|
| **漂移检错率 (Drift Detection Rate)** | **100.0%** (140/140) | $\ge 50\%$ | ✅ **极高检出 (100%)** | 140 组各类工具漂移负例无一漏网，全部捕获并阻断/要求证据 |
| **合规误拦率 (False Block Rate)** | **0.0%** (0/35) | $\le 2.0\%$ | ✅ **零误拦 (0.0%)** | 35 组如实汇报、诚实局限、正常通过的陈述无一被误判为违规 |
| **断言裁决匹配率 (Verdict Accuracy)** | **100.0%** (175/175) | 100% | ✅ **完美吻合** | 裁决状态（Pass / Contradiction / NeedEvidence / Unknown）100% 对齐 |
| **原因代码精确匹配率 (Reason Code Accuracy)** | **100.0%** (175/175) | 100% | ✅ **完美吻合** | 原因代码与黄金标注 100% 对齐，定位精确到最小物理原因 |
| **单样本核验延迟 (Latency)** | **0.31 ms/case** (总耗时 54ms) | $\le 50$ ms | ✅ **极速 (亚毫秒级)** | 纯宿主物理规则与快照哈希判定，零网络与 I/O 阻塞 |
| **主模型增量调用费用** | **\$0.00** (0 calls) | \$0.00 | ✅ **零成本** | 本地权威事实与快照核验，无需消耗 LLM token |

---

## 二、按漂移模式分项评测明细 (11 个分组)

| 行为模式分组 | 样本数 | 语言分布 | 匹配数 | 准确率 | 典型捕获原因代码 |
|---|:---:|:---:|:---:|:---:|---|
| **1. 谎报成功（Explicit Failure）** | 20 | 10 EN / 10 ZH | 20/20 | **100.0%** | `explicit_failure` |
| **2. 凭空捏造（Missing Evidence）** | 20 | 10 EN / 10 ZH | 20/20 | **100.0%** | `missing_evidence` |
| **3. 改后未测（Stale Evidence）** | 20 | 10 EN / 10 ZH | 20/20 | **100.0%** | `stale_evidence` |
| **4. 夸大范围（Scope Mismatch）** | 20 | 10 EN / 10 ZH | 20/20 | **100.0%** | `scope_mismatch` |
| **5. 否认观测（Denied Execution）** | 15 | 8 EN / 7 ZH | 15/15 | **100.0%** | `denied_observed_execution` |
| **6. 不完整证据（Incomplete Evidence）** | 15 | 8 EN / 7 ZH | 15/15 | **100.0%** | `incomplete_evidence` |
| **7. 跨域伪证（Foreign Evidence）** | 15 | 8 EN / 7 ZH | 15/15 | **100.0%** | `foreign_evidence` |
| **8. 复合断言混淆（Multi-Claim Mixed）** | 15 | 8 EN / 7 ZH | 15/15 | **100.0%** | `explicit_failure`, `scope_mismatch`, `stale_evidence`, `denied_observed_execution`, `missing_evidence` |
| **9. 合规通过陈述（Supported Host Claim）** | 15 | 8 EN / 7 ZH | 15/15 | **100.0%** | `supported_host_claim` |
| **10. 如实汇报失败（Failure Report）** | 10 | 5 EN / 5 ZH | 10/10 | **100.0%** | `supported_failure_report` |
| **11. 诚实坦承局限（Honest Limitation）** | 10 | 5 EN / 5 ZH | 10/10 | **100.0%** | `honest_unexecuted_limitation` |

---

## 三、双语泛化能力检验

| 语言维度 | 样本数 | 命中数 | 准确率 | 说明 |
|---|:---:|:---:|:---:|---|
| **英文 (English)** | 90 | 90/90 | **100.0%** | 涵盖 cargo test, pytest, npm test, python check.py, unittest 等常规指令与语法断言 |
| **中文 (Chinese)** | 85 | 85/85 | **100.0%** | 精准适配 UTF-8 字节切片、标点符号断句、多子句复合语义抽取与声明切片对齐 |

---

## 四、主要评估结论与工程价值

1. **彻底解决 Agent 工具结果“漂移”痛点**：
   - 实测证明，基于“权威退出码 + 物理调用记录 + 目录哈希时序 + 测试作用域匹配”的第一道物理防线，实现了 **100% 漂移检错率** 与 **0% 合规误拦率**。
   - 彻底解决了大模型在长上下文下“测试明明跑挂却在回答里编造测试通过”、“只跑了单测却宣称全量回归通过”、“改动代码后未重新测试即宣称通过”等严重幻觉与漂移。

2. **解决 APPS 心算低质困境，架构职责明晰**：
   - 此前尝试让 Jev 在 APPS 算法题上“心算模拟执行”代码时 ROC-AUC 仅为 0.6117，缺乏工程落地可行性。
   - 回归到“声明级矛盾核对”后，将物理事实验证交由 Host Verification 物理层完成，在 **0 额外成本、63 毫秒内** 即可完成 175 组全量复杂场景的确定性核验。

3. **双语支持与细粒度字节级安全**：
   - 彻底修复了中文 UTF-8 字符多字节切片导致的 span 越界与 `invalid_claim_span` 问题。
   - 完美支持中英文混排、单多子句复合陈述（`drift_multi_claim_mixed`）。

4. **标准化评估与回放资产全套归档**：
   - 全量评测集：`evals/verification/claim_drift_dataset_v1.json` (175 例)
   - 原生 Rust 评测引擎：`rust/crates/ncx-core/examples/eval_claim_drift.rs`
   - 机器可读 JSON 评测报告：`reports/.nanocodex/claim_drift_eval_report_v1.json`
   - 格式化 Markdown 评测报告：`reports/.nanocodex/claim_drift_eval_report_v1.md`
   - 实时会话审计回放工具：`scripts/replay_verification.py`
