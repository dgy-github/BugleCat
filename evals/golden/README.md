# evals/golden — 金标回归评测集（buglecat.golden.v1）

按现代 LLM 评测治理标准为 BugleCat 定制的评测套件，与既有资产的关系：

| 既有资产 | 本套件的角色 |
|---|---|
| `bench/`（18+2 道编码任务） | 被 G-BENCH-001 做有效性门禁（见下），agent 层跑分 |
| `evals/self_repair/`（6 案例故障注入） | 被 G-SR-001 做锚点有效性门禁，agent 层全量跑 |
| `evals/harness.py` / `metrics.py` | 确定性指标层；本套件负责"哪些必须跑、跑不过算不算失败" |
| `tests/`（629 pytest） | 金标用例直接引用对应测试文件作为回归断言 |

## 三层结构（tier）

| tier | 内容 | 默认 |
|---|---|---|
| `unit` | 24 个 critical 用例：pytest 直引 51 个测试文件中的核心面 + 3 个内建校验器。**离线可跑、确定性、零外部依赖** | 默认执行 |
| `cargo` | 11 个 Rust 侧关键行为（补丁引擎/编译门禁/退出码/脱敏 + 事实存储/分层召回/记忆索引/分块压缩/工具恢复/状态机/指令冲突）；过滤器匹配 0 个测试即判失败 | `--tier unit,cargo`；CI 的 `rust-gate` job 用 `--tier cargo --scope-to-tiers` |
| `agent` | 2 个真实 Agent 跑分（需 `ncx.exe` + 已配置模型路由） | `--tier unit,cargo,agent` |

## 门禁语义（不可弱化）

1. **SKIP ≠ PASS**：任何 tier 未选中/工具缺失导致的 skip 只是不计入通过；**critical 用例被 skip = 门禁直接失败**。cargo/agent 用例定为 high（可离线豁免），unit 层全部 critical（不可豁免）。
2. **缺失证据即关闭（fail closed）**：一次都没执行（如过滤器配错）不可能得出通过；空执行的 pass_rate 为 `null` 而非 0/100。
3. **基础设施错误与真实失败分开**：runner 内部异常记 `error`，同样失败门禁，但报告里可与断言失败区分。
4. **基准确有效才算数**：G-BENCH-001 对 `bench/tasks/*` 逐题校验——**评分器非空洞**（agent 动手前 check 必须失败）、**种子缺陷真实存在**（修好 t2_median "不改就通过"的问题）、**参考解答必须通过**（`reference/files.json`，纯数据、不动态执行，且 `bench/run.py` 已将 `reference/` 列入 HIDDEN 不会泄漏进 Agent 工作区）、**故意写错的答案必须被拒**（`reference/mutants.json`，每题必备；每个 mutant 是对参考解的一处精确替换并附原因，锚点必须唯一；任何 mutant 能通过 check 即判 grader 太弱。参考解也是每题必备）。校验时用 `python -B` 并清理 `__pycache__`，避免同长度改动复用旧字节码造成假通过。
5. **judge 永不冒充**：`judge.py` 仅用于确定性断言覆盖不了的开放输出（如压缩摘要忠实度）；判定需 `method=llm 且 score>=4`，端点未配置记 `skip` 且**退出码非 0**——"没评"永远不等于"评过"。

## 用法

```cmd
:: 日常离线门禁（CI 就跑这个）
python -m evals.golden.runner

:: 单能力面 / 单用例
python -m evals.golden.runner --capability bench_task
python -m evals.golden.runner --case G-PATCH-001

:: 全层（发版前）
python -m evals.golden.runner --tier unit,cargo,agent

:: judge（可选，配置 NCX_JUDGE_BASE_URL / NCX_JUDGE_MODEL / NCX_JUDGE_API_KEY 后可用）
python -m evals.golden.judge --text-file <待评文本> --criteria-file <评分标准>
```

报告落 `evals/golden/reports/golden_<stamp>.json|.md`，含逐用例结果与能力面覆盖矩阵。

## 覆盖矩阵（24 个 unit 用例的能力面分布）

agent_loop(4) · sandbox_guard(4) · patch_engine(1) · tools(1) · mcp(1) ·
memory_context(3) · session(2) · provider(2) · config(1) · orchestrator(1) ·
bench_task(1) · self_repair(1) · compaction(2) · metrics_honesty(2)

**已知未覆盖面**（后续扩展方向，不虚报已覆盖）：deliverable 交付门禁（PDF 头校验）、hooks、
slash commands、MCP 并发许可、负载/稳定性（pass^k：同任务重复 k 次全成才算通过——
agent 层跑分建议逐步引入 repeats>=3）。

## 与 eval-gate CI 的关系

`.github/workflows/eval-gate.yml` 在 push/PR 时跑两个 job：`golden-gate`（`pytest tests -q` +
unit 层）和 `rust-gate`（rustfmt + `cargo test --workspace --lib` + cargo 层）。agent 层需要
模型密钥，仅人工触发；`benchmark` 低于 `--min-pass-rate` 时以非 0 退出。
