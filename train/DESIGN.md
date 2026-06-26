# ncx-forge — nanocodex agent 骨架训练框架（设计文档 v0.1）

> 状态：设计稿，待评审。纯 API（无 GPU），复用现有 `bench/`。零权重训练。
> 目标读者：接手实现的 agent / 作者本人。

## 1. 目标与非目标

**目标**：自动提升 nanocodex agent 的**工程化能力**（完成率、可靠性、少跑偏），
方法是让一个强"教师"模型迭代优化 agent 的**可配置骨架**，用 `bench/` 的可验证
通过率当 fitness 做闭环进化。

**非目标（本版不做）**：
- 不训练/微调模型权重（无 GPU；另起路线，见 §10 预留）。
- 不改 agent 的核心架构（loop/sandbox/tools 实现），只调它们的**文本与策略参数**。
- 不追求刷高单一 bench 分数 → 用 train/val/test 切分 + 任务生成对抗过拟合（§7）。

**一句话定位**：这是 agent 的**骨架进化器（prompt/scaffold optimizer）**，
类比 GEPA / DSPy-optimizer / AlphaEvolve 的"反思式变异 + 可验证打分"，但作用对象是
一个完整的 coding agent 的 harness，而非单条 prompt。

**期望与边界（诚实）**：harness 优化能稳定提升**可靠性/工程贴合度**，但**受限于底座
模型的能力天花板**（plan/verify 跑在主模型上）。这不是"提高智商"，是"把现有智商用满、
少犯工程错误"。memory 里已记过结论：*model is the real lever*——本框架是第二根杠杆。

## 2. 核心抽象：基因组（Genome）

把当前**硬编码/散落**的可调骨架，外置成一份带版本号的 genome 文件
（`train/genomes/<gen>-<id>.toml`），agent 启动时加载覆盖。Genome 字段全部是
**文本或标量参数**，可被教师模型读写：

| 基因 | 现在在哪 | 说明 |
|---|---|---|
| `system_prompt` | `rust/crates/ncx-cli/src/main.rs` 的 `SYSTEM_PROMPT` const | agent 主指令 |
| `tool_desc.apply_patch` 等 | `rust/crates/ncx-core/src/tools.rs` 各 `description()` | apply_patch 描述是 load-bearing（memory 有记） |
| `orch.classify_sys` / `plan_sys` / `decompose_sys` / `worker_sys` / `verify_sys` | `orchestrator.rs` 的 `*_SYS` const | 编排器各节点 prompt |
| `orch.workers` / `high_workers` / `max_depth` / `max_verify_retries` | `OrchestratorConfig` | 标量策略旋钮 |
| `memory_seed` | `.ncx/memory/LEARNINGS.md` | 预置的项目经验（少量、通用的工程戒律） |

> **不可进化的东西**：任务的 `check.py`（奖励函数）对 agent 与教师**全程不可见**，
> 防奖励黑客（§9）。教师只能改 prompt/策略，不能碰评分。

### 2.1 唯一需要的 Rust 改动（最小、零风险）

新增一个**仅在训练时生效**的注入点：环境变量 `NCX_GENOME=<path.toml>`。
启动时若设置，则用文件里的同名键覆盖上述 prompt/config 默认值；未设置则行为完全不变。

- 落点：`main.rs`（system prompt + tool 注册时的描述覆盖）、orchestrator 构造处
  （`OrchestratorConfig` + `*_SYS` 注入）。
- 形态：一个 `ncx-core::genome::Genome { ... }` 结构 + `load_from(path)`，
  各 prompt/desc 改为"有覆盖用覆盖、否则用 const 默认"。
- 约束：default genome == 现有硬编码值，保证 `NCX_GENOME` 不设时**字节级等价**。
  这条改动单独一个 commit，配单测（覆盖生效 / 不设时回落默认）。

## 3. 组件总览

```
            ┌─────────────┐   genome.toml    ┌──────────────┐
            │  Optimizer  │ ───────────────▶ │  Evaluator   │
            │ (search 循环)│                  │ (bench wrap) │
            └─────┬───────┘ ◀─────────────── └──────┬───────┘
                  │            score + 失败轨迹        │ 调 ncx (NCX_GENOME)
            mutate│                                   ▼
            ┌─────▼───────┐                    ┌──────────────┐
            │   Teacher   │◀── 失败 transcript ─│ Trajectory   │
            │ (强模型变异) │                    │   Store      │ ── 未来 SFT/RL 备料
            └─────────────┘                    └──────────────┘
                  ▲
            ┌─────┴───────┐
            │ TaskGen     │ 生成新 bench 任务（扩覆盖、抗过拟合）
            └─────────────┘
```

1. **Evaluator** — 给定一个 genome，跑 `bench/run.py`（已支持 `--repeats`/`--tasks`/报告）
   在指定任务集上，产出结构化结果：每任务 `k/N` 通过率、均 token、均时延、**失败任务的
   最后一条 transcript**（给教师当信号）。≈90% 已就绪，只需让 run.py 接受 `NCX_GENOME`
   并把失败轨迹写进 JSON 报告。
2. **Teacher（变异算子）** — 强模型。输入：当前 genome + 失败轨迹（哪些任务挂了、
   agent 当时怎么想/调了什么工具/错在哪）。输出：一份**改进后的 genome**（reflective
   mutation：先诊断失败模式，再针对性改 prompt/策略）。一次只改少数字段，便于归因。
3. **Optimizer（搜索循环）** — 维护 champion（或小种群），每代：选当前最差失败 → 教师变异
   → Evaluator 在 train 集打分 → 若优于 champion，再上 val 集复核 → 接受/拒绝 → 记 lineage。
   终止：预算耗尽 / 连续 K 代无提升（plateau）。
4. **TaskGen（任务生成器）** — 另一个教师，按"难度/能力维度"生成新 bench 任务
   （`prompt.txt` + 隐藏 `check.py` + 可选 seed）。**必须自校验**：参考解通过 check、
   且对"空实现/seed-buggy"失败，才入库（这条纪律在 stream E 已验证过）。
5. **Trajectory Store** — 记录每次 `(genome_id, task, run_idx) → 完整消息/工具轨迹 +
   pass/fail + token`。既是调试依据，也是**未来微调/RL 的数据集**（§10）。
6. **Reporting** — champion 随代数的通过率曲线、genome diff 血缘、最终 held-out test 分数。

## 4. 主循环（伪码）

```python
champion = extract_current_genome()          # = 现有硬编码值，作为 gen0 基线
champion.score = evaluate(champion, TRAIN, repeats=R)
log(champion)

for gen in range(max_gens):
    if budget.exhausted(): break
    failures = champion.score.failing_transcripts(top_k=3)   # 最该修的
    if not failures: break                                    # train 全过 → 提难度(TaskGen)
    candidate = teacher.mutate(champion.genome, failures)     # 反思式变异
    cand_train = evaluate(candidate, TRAIN, repeats=R)
    if cand_train.passrate <= champion.score.passrate: 
        log_reject(candidate, cand_train); continue
    cand_val = evaluate(candidate, VAL, repeats=R)            # 过拟合闸门
    if accept(cand_train, cand_val, champion):                # 见 §7 的接受准则
        champion = candidate; log_accept(champion)
    plateau.update(...)
    if plateau.stuck(): TaskGen.expand(); plateau.reset()     # 没进展就加难度

final = evaluate(champion, TEST, repeats=R_high)              # 一次性 held-out
report(lineage, curves, final)
```

## 5. Fitness 定义

主指标：**val/test 任务集上的平均通过率**（`Σ passes / Σ runs`）。
平手时的次级目标（lexicographic）：① 更少 token、② 更少时延、③ genome 更短（奥卡姆）。
- 用 `nanocodex`（直跑）和 `nanocodex-orch`（编排）两种 arm 分别评，因为 genome 同时含
  orchestrator 基因；可分阶段：先优化直跑骨架，再优化编排策略。
- 评测用**便宜的 fast 模型**跑 worker，教师用强模型——成本错配，省钱。

## 6. 目录布局（纯 Python，零 Rust 冲突，除 §2.1 那一处）

```
train/
  DESIGN.md            # 本文件
  forge.py             # Optimizer 主循环 (CLI: --gens --budget --arm ...)
  genome.py            # Genome 读写/校验/diff/extract-current
  evaluator.py         # 包 bench/run.py，注入 NCX_GENOME，回收失败轨迹
  teacher.py           # 教师变异 + 任务生成（调 DeepSeek/强模型）
  store.py             # Trajectory/结果 JSONL 存档
  genomes/             # 每代 genome.toml（gitignore，除 baseline 与 best）
  runs/                # 评测结果/报告/曲线（gitignore）
  tasks_gen/           # TaskGen 产出的任务（人工 review 后并入 bench/tasks）
```
（`bench/` 复用不动；`train/genomes/`、`train/runs/` 进 .gitignore，只留 baseline 和
当选 champion。）

## 7. 过拟合与噪声（本框架最大风险，重点对待）

- **任务太少**：现有 8 个 bench 任务远不够，直接进化必然过拟合。对策：
  - train/val/test **三切分**（任务级，不是 run 级），test 只在最后碰一次。
  - **TaskGen 持续扩库**：plateau 时自动加任务；目标先到 ~40+ 任务、覆盖多能力维度
    （文件编辑/多文件重构/读栈跟踪 debug/算法/IO/并发/CLI 解析…）。
- **打分噪声**：agent 随机。`repeats≥3` 仅够弱信号。接受准则用**保守下界**：
  candidate 需在 train 上明确高于 champion（差值 > 噪声带，或重复更多次复核），
  且 val 不退化。不接受"1 次跑赢"的偶然。
- **诚实记录**：任何子采样/截断（每代只跑部分任务、top_k 失败）都在报告里 `log`，
  避免"看起来覆盖全了"的假象。

## 8. 成本模型与预算

单次 evaluate 成本 ≈ `任务数 × repeats × 每任务 agent 调用数`。控制手段：
- 每代只在 **train 子集**（含当前失败任务 + 随机若干）评，全量评仅在接受复核时。
- worker 用 fast 模型；教师强模型每代仅 1~N 次调用（贵但少）。
- `forge.py --budget <tokens>`：复用主框架的预算思路，到顶即停并出当前 champion。
- 缓存：相同 `(genome_hash, task, seed)` 不重复跑。

## 9. 风险与对策（汇总）

| 风险 | 对策 |
|---|---|
| 奖励黑客（教师/agent 迎合 check.py） | check.py 对两者全程不可见；教师只改 prompt/策略 |
| 过拟合小任务集 | train/val/test 切分 + TaskGen 扩库 + 保守接受准则 |
| 打分噪声导致假提升 | repeats + 噪声带阈值 + val 闸门 |
| 成本失控 | 子采样 + fast/强模型错配 + token 预算硬上限 + 结果缓存 |
| genome 注入引入回归 | default genome 与现有 const 字节级等价 + 单测 + 不设 env 时零影响 |
| 教师改坏 orchestrator 策略卡死 | 标量旋钮加合法区间钳制；每代有 wall-clock/调用上限 |

## 10. 预留：通往权重训练（未来，需 GPU）

Trajectory Store 从第一天就按 **SFT/RL 友好 schema** 落盘：
`{system_prompt, task, messages[], tool_calls[], final, reward(0/1), tokens}`。
- 这正是 SFT（取 reward=1 的轨迹做模仿）或 RL（GRPO/PPO，reward=bench 通过）的数据。
- 届时只需加一个 `export.py` 转成 trl/verl 的数据格式，骨架进化的产物（更好的 prompt）
  还能当 RL 的系统提示初值。**本设计让 option B 变成"加载数据"而非"重起炉灶"。**

## 11. 路线图（里程碑）

- **M0｜最小闭环**（先做）：§2.1 genome 注入（Rust）+ `genome.py` extract-current +
  `evaluator.py` 包 run.py + `teacher.py` 单次变异 + `forge.py` 单 champion 爬山。
  验收：跑 ≥5 代，champion 在 val 上不低于 gen0 baseline，全程有报告。
- **M1｜抗过拟合**：train/val/test 切分 + TaskGen（自校验）+ 保守接受准则。
- **M2｜搜索增强**：小种群 / Pareto（通过率×token）/ lineage 可视化。
- **M3｜数据导出**：Trajectory Store 成型 + `export.py`，对接未来 GPU 训练。

## 12. 开放问题（评审时定）

1. 教师模型用哪个？（建议：主模型的最强档；与被训 agent 解耦，避免自我打分偏置）
2. 第一阶段先优化"直跑骨架"还是"编排策略"？（建议直跑骨架先，变量少、归因清）
3. genome 注入用 env+TOML 文件，还是复用现有 config profile 机制？（倾向独立文件，
   不污染用户 config；但可共用 `ncx-config` 的 TOML 解析）
4. TaskGen 的任务难度分布与"能力维度"清单由谁定义？（建议先人工列 6~8 个维度种子）
```
