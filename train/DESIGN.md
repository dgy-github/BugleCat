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

### 训练器 / 被训者边界（务必看清）

仓库里有两套实现：遗留 Python agent `nanocodex/*.py`（已弃）和 Rust 重写 `rust/crates/...`。
**本框架只训 Rust 版**：

- **被训者（trainee）= Rust `ncx.exe`**。可进化的 genome 字段全部指向 **Rust 源**：
  `SYSTEM_PROMPT`（`rust/crates/ncx-cli/src/main.rs`）、各工具 `description()`
  （`rust/crates/ncx-core/src/tools.rs`）、orchestrator `*_SYS`（`orchestrator.rs`）。
  `genome.py` 的 `extract_current_genome()` 取的是这些 **Rust 默认值**，与 `nanocodex/*.py` 无关。
- **训练器（trainer）= `train/` 纯 Python + 复用 `bench/`**。它只通过 **subprocess**
  驱动 `ncx.exe`（注入 `NCX_GENOME`）和教师 CLI（codex/claude）。**不 import、不依赖**
  遗留 Python agent 包 `nanocodex/*.py`（genome TOML 用 Python 标准库 `tomllib` 读、
  自带小 writer 或 `tomli_w` 写，保持 train/ 自包含）。
- **唯一的 Rust 改动** = §2.1 的 P1 genome 注入。其余全是 train/ 侧 Python，零 Rust 冲突。

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

### 2.1 硬前置条件 P1：genome 注入 ✅已实现（`f1af9ce`）

> ✅ **已落地并 live 验证**：`NCX_GENOME` 注入已在 `ncx-core::genome` 实现并接入
> `main.rs`。最初对抗评审正确指出它"完全不存在"——若不先建，forge 写的每个候选都被
> 忽略、所有候选同分、整轮"绿着"空跑。现已建好：实测 self-destruct 系统提示让 agent
> 拒绝建文件、sentinel 工具描述被模型逐字回显、空 genome 与默认字节等价。

新增一个**仅在训练时生效**的注入点：环境变量 `NCX_GENOME=<path.toml>`。
启动时若设置，则用文件里的同名键覆盖 prompt/desc/config 默认值；未设置则行为完全不变。

- 落点：`main.rs`（`SYSTEM_PROMPT` 用 `genome.system_prompt` 覆盖；tool 注册时按工具名
  覆盖描述）。M0 不碰 orchestrator 基因。
- 形态：`ncx-core::genome::Genome { system_prompt: Option<String>, tool_desc: HashMap<String,String> }`
  + `load_from(path)`（用 `toml` crate 解析）。`Tool::description` 需从 `&str` 改成可被
  owned override 覆盖（trait 改 owned String，或每个工具包一层 `Option<String>` 覆盖，
  由 genome 按工具名解析）。
- 实现（与原计划略有取舍）：覆盖在**注册层**应用 —— `register()` 建 catalog 条目 +
  新增 `schema_for()` 用 genome 覆盖后的描述建模型可见 schema；`Tool::description()` trait
  默认不变（改动小、空 genome 可证字节等价）。空/空白/格式错的 genome → 空 genome（no-op）。
  单测覆盖：① 空 genome 时 schema+catalog 字节等价；② 覆盖到达 schema+catalog；③ 空白/
  格式错处理；④ 多行 prompt。
- **forge 侧自检（gate）✅已实现并 live PASS**：
  > 实测教训：最初的"自毁 genome（refuse all）→ 通过率下降"**不可靠** —— 强模型常无视
  > "拒绝"指令、照样完成任务（任务指令与系统提示竞争，模型合规是噪声；实测 t1 仍 1/1）。
  改用**确定性 sentinel 注入**：往 `system_prompt` 塞唯一码字 `NCXFORGE_SENTINEL_4242`
  + "被问到只回该码字"，跑一次 read-only 提问，**断言带 genome 时输出含码字、baseline 不含**
  （两条都满足才 PASS，否则报 `NCX_GENOME not honored` 中止）。确定性、便宜（2 次 read-only），
  不依赖模型合规。实现 `train/forge.py:self_check`，`python train/forge.py --self-check` 已 PASS。

### 2.2 硬前置条件 P2：失败轨迹采集 ✅已实现（`train/evaluator.py`）

> ⚠️ **已实测**：`bench/run.py` **不保留** agent 输出 —— `run_once` 里 agent 的
> `subprocess.run` 结果都没绑定变量，只留 grader 最后一行 70 字符（`note`），且 `finally`
> 里 `rmtree` 了工作区。**教师赖以诊断失败的"轨迹"目前根本没有数据源。** 只喂任务名 +
> 70 字符 grader 尾巴（而且按反作弊该 grader 输出本就不该给教师看）→ 三个强模型在"靠文件名
> 猜"。

对策：扩展 evaluator/run.py，让每次 ncx 运行写**独立 session log**（ncx 已有
`Session::with_log`，见 `main.rs:168`），从日志里抽 agent 最后的 assistant 消息 + 工具调用
作为失败轨迹；**显式剔除 grader 输出**（保住 check.py 对教师不可见）。先定死轨迹 schema，
再写任何教师后端。这是 P2 硬前置。

> ✅ **已实现**：`train/evaluator.py` 在临时工作区跑 ncx（注入 `NCX_GENOME`），在 `grade()`
> 复制 `_check.py` 之前从 `<ws>/.nanocodex/session.jsonl` 抽 agent 最后消息 + 工具调用作失败
> 轨迹，并**剔除含 `check.py/_check.py/grader/hidden test` 的行**（grader 输出永不外泄给教师）。
> 已对真实 session log 验证解析正确；5 个 agent-free 单测（`train/test_evaluator.py`）覆盖
> 解析 + 脱敏 + 截断。

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
- **打分噪声 → 噪声被当成"提升"**（对抗评审高危项）：agent 随机，`repeats=3` + 严格 `>`
  门 + champion 只评一次 ⇒ 纯方差的 +1 会被提拔进 `current_best` 且因"平手保留 incumbent"
  再也踢不掉，报告显示"单调提升"实为采样噪声。对策：**每代都重评 incumbent**（同 repeats）；
  接受需 `candidate − incumbent ≥ 噪声带`（或接受判定专门加大 repeats）；**接受打分用 held-out
  split**（教师的 note 信号没见过的任务）；定期在全集重评 champion，**退化就降级**。
- **奖励黑客 / 过拟合 note 尾巴**：grader 尾巴常泄露期望答案形状（"expected 15 got 0"），
  教师可能往 system_prompt 塞任务专用提示骗过这些 bench 任务而非真提升 → 故 grader 输出对教师
  **全程屏蔽**（见 §2.2），且 train/val/test 旋转切分让"只过见过的任务"不成立。
- **`_check.py` 落在工作区**：`bench/run.py` grade() 把 `check.py` 复制成工作区里的 `_check.py`
  再评分——agent 在 workspace-write 下**能读到它**。语义门要扫描候选 genome 文本，拒绝任何提及
  `check.py/_check.py/grader/hidden test` 的 prompt；并考虑给 grader 副本改名/隔离，使 agent
  即便被教唆也读不到。
- **诚实记录**：任何子采样/截断（每代只跑部分任务、top_k 失败）都在报告里 `log`，
  避免"看起来覆盖全了"的假象。

## 8. 成本模型与预算

> ⚠️ **真正的成本炸弹是内层 bench 循环，不是教师调用**（对抗评审高危）：一轮 ≈
> `候选数 × 任务数 × repeats × 每个 ncx 跑最多 60 个模型轮次(max_iterations)`，每任务 180s 超时。
> 跨多轮 + patience，是**数千次付费 DeepSeek ncx 调用** + N 次强教师调用。教师调用反而少。

控制手段：
- **全局预算/时钟 governor**（在后端之外）：累计 token 估算（codex 往 stderr 打 `tokens used`；
  DeepSeek 在 `ModelResponse.usage` 给 token——**注意是 dict，预算要按 token 求和、不是 usd**，
  DeepSeek provider 没有 cost_usd）+ wall-clock 上限；每轮、每候选评测前检查，超了就**干净中止、
  保留当前 champion**。不要依赖 claude 的 `--max-budget-usd`（它只覆盖本机已死的那个教师）。
- **便宜预筛**：候选排序用小 fast 任务子集 + `repeats=1` + 更短超时；只对胜出者上全 repeats/全集。
- 每代**限候选数**（如 panel 取 top-1/2 再全评，不是所有后端都全评）。
- 缓存 `(genome_hash, task, seed)` —— 但**只在 P1 落地后**才有意义（否则不同 genome 哈希相同行为，
  缓存会用 champion 的旧分掩盖"注入是死的"这个 bug）。
- 预算单位用**所有后端都能报的量（token / eval 次数）**，不用 usd。

## 9. 风险与对策（汇总）

| 风险 | 对策 |
|---|---|
| **P1 未落地 → 静默空跑**（最高危） | genome 注入先行 + forge 自毁-genome 自检 gate（§2.1） |
| **P2 缺失 → 教师瞎猜**（最高危） | 先从 session log 采集失败轨迹、屏蔽 grader 输出（§2.2） |
| 注入攻击：不可信轨迹 → 教师 → 恶意 genome（含真实 shell 工具，workspace-write） | 轨迹当**数据**定界、剥 fence、硬截断；genome 与 incumbent diff、拒绝含 shell 元字符/URL/`curl\|sh` 的描述；语义门拒提及 grader 工件；**genome 仅改描述不改行为，沙箱仍管执行**——这是结构性保护 |
| 打分噪声当成提升 | 每代重评 incumbent + 噪声带阈值 + held-out 接受 + 退化降级（§7） |
| 过拟合 / 奖励黑客 | grader 输出对教师屏蔽 + train/val/test 旋转 + `_check.py` 隔离（§7） |
| 成本失控（内层 bench 循环为主） | 全局 token/时钟 governor + 便宜预筛 + 限候选数（§8） |
| codex 模型名硬编码失败 | 从 `~/.codex/config.toml` 解析 model；探测断言 `-o` 非空 |
| claude 401 被误判可用 | 按 `is_error` 结构化字段判，非退出码/子串 |
| size cap 误杀基线 apply_patch | cap 从实测基线取 + round-trip 测试（§10.5） |
| 教师后端中途 502/限流抖动 | 区分瞬时(重试1次)/永久(401，缓存不可用)；K 次连败后再探测 |
| 有效 system prompt 是拼接的（base+instructions+recall+skills，`main.rs:150`） | 要么把**完整拼接后**的 prompt 作只读上下文喂教师、标出可进化的 base 段；要么训练时把后缀源固定/置空并记录 |

## 10. 预留：通往权重训练（未来，需 GPU）

Trajectory Store 从第一天就按 **SFT/RL 友好 schema** 落盘：
`{system_prompt, task, messages[], tool_calls[], final, reward(0/1), tokens}`。
- 这正是 SFT（取 reward=1 的轨迹做模仿）或 RL（GRPO/PPO，reward=bench 通过）的数据。
- 届时只需加一个 `export.py` 转成 trl/verl 的数据格式，骨架进化的产物（更好的 prompt）
  还能当 RL 的系统提示初值。**本设计让 option B 变成"加载数据"而非"重起炉灶"。**

## 10.5 教师后端：可插拔多模型 panel（Codex/GPT + Claude/Opus + API）

用户决策：把**最强外部 agent**当教师 —— Codex（GPT 系）+ Claude Code（Opus 系）。
设计成可插拔多后端 + panel，强模型可用就上、不可用优雅降级。

### 统一抽象
`train/teacher.py` 一个 `TeacherBackend` 基类，三个实现，对 forge **同形**：
```
class TeacherBackend:
    name: str
    def available(self) -> bool   # 启动探测一次，结果缓存
    def propose(self, prompt: str) -> str | None   # 纯文本进 / 文本出；None=失败
```
forge 负责拼 prompt、解析 genome、选择、记 lineage —— 后端只管"把 prompt 送给某模型、
把文本拿回来"。

### 各后端调用契约（全部已实测）

| 后端 | 调用 | 模型 | 可用性判据 | 本机实测 |
|---|---|---|---|---|
| `codex`（GPT） | `codex exec -m <model> -s read-only --skip-git-repo-check -o <f>`，prompt 走 stdin | **从 `~/.codex/config.toml` 的 `model` 解析**，勿硬编码 | `rc==0` **且** `-o` 文件非空 | 本机 codex 走 CLIProxyAPI 代理(`127.0.0.1:8317`)，`model=gpt-5.4`；`-m gpt-5` 报 502 + 空文件 |
| `claude`（Opus） | `claude -p --model opus --output-format json`，prompt 走 stdin | `opus` | **解析 JSON 取 `is_error==false`**（不是退出码、不是子串扫描） | 本机**未认证**：rc=0 但 `is_error:true`（401）→ 探测应判不可用并缓存、不重试 |
| `api`（DeepSeek） | 复用现有 provider（`cfg.model`） | `cfg.model` | endpoint 通即可 | 始终可用，作 panel 地板 |

> 关键纠错（实测）：① codex 模型名**必须从环境解析**，硬编码 `gpt-5` 在本机 100% 失败；
> ② claude 的 401 是 `rc=0 + is_error:true`，**只能靠结构化字段判**，子串扫描会漏判
> （限流/过载/本地化文案）；③ `-o` 文件只含最终消息，`rstrip()` 即可，别臆造"剥掉
> `Shell cwd was reset` 末行"那种脆弱逻辑。

### panel 策略
每代：可用后端**并行各提一个候选 genome** → 各自评分 → 留最优 → lineage 记**解析出的真实
模型 id**（不是只记 `backend.name`，否则"Opus 教的"无从验证）。启动横幅打印：
`live teachers: api(deepseek-…), codex(model=gpt-5.4 via cliproxyapi); SKIPPED: claude(401)`，
让操作者清楚本机其实是"单教师"还是真 panel。

### prompt / 解析契约（含安全）
- 教师 prompt 要求只输出**一个 fenced TOML 块**。解析取**最后/最大**的块（最终答案通常在最后），
  必须能 `tomllib` 解析成预期 key 形状，否则判失败。
- 校验：拒绝空 `system_prompt`、拒绝任何空 `tool_desc` 值（`apply_patch` 描述是 load-bearing，
  被清空会让 agent 退化）；只接受真实注册工具名（从 ncx 实际工具集派生，**不要硬编码 4 个**，
  现已含 web_search/web_fetch/mcp）。
- **size cap 从实测基线取**：`apply_patch` 默认描述 ≈8125 字符，所以 cap = `max(基线长, …)+余量`
  （如 12k），并加 round-trip 测试：`parse(serialize(extract_current())) == extract_current()`，
  保证基线永远能过自己的校验（否则会把优化器逼向"裁剪 apply_patch"——已知会引发 git-diff 回退死循环）。

## 11. 路线图（里程碑）

- **M0a｜前置（必须先做，对抗评审定为硬 gate）**：
  ① §2.1 P1 genome 注入（Rust + 字节等价单测 + forge 自毁-genome 自检）；
  ② §2.2 P2 失败轨迹采集（run.py 写 session log、屏蔽 grader 输出）。
  这两件不过，**整个 M0 是静默空跑**，所以先于教师工作。
- **M0b｜最小闭环**：`genome.py`（extract-current + round-trip 测试，size cap 从基线取）
  + `evaluator.py`（包 run.py、注入 `NCX_GENOME`、回收轨迹）+ `teacher.py`（codex 后端先行，
  模型从 config 解析；claude 探测可用才上；api 地板）+ `forge.py` 单 champion 爬山 + 全局
  token/时钟 governor。验收：自检 gate 通过 + 跑 ≥5 代 + champion 在 **held-out** 不低于
  gen0 baseline + 全程报告/lineage（记真实模型 id）。
- **M1｜抗过拟合**：train/val/test 切分 + TaskGen（自校验）+ 保守接受准则。
- **M2｜搜索增强**：小种群 / Pareto（通过率×token）/ lineage 可视化。
- **M3｜数据导出**：Trajectory Store 成型 + `export.py`，对接未来 GPU 训练。

## 12. 决策记录（评审已定 2026-06-26）

1. **教师 = 可插拔多模型 panel**（用户追加决策）：Codex（GPT 系，`codex exec`）+
   Claude Code（Opus 系，`claude -p`）+ DeepSeek（`cfg.model`，地板）。可用就上、不可用
   优雅降级。详见 §10.5。本机现状：codex 可用（经 CLIProxyAPI 代理，model=gpt-5.4）、
   claude 未认证(401) 跳过、api 始终可用 → 实际先以 codex+api 跑。
2. **M0 先优化"直跑骨架"**：只进化 `system_prompt` + 工具描述，评测跑 `nanocodex`
   直跑 arm。变量少、归因清。编排策略（orchestrator 基因）推到 M1+。
3. **genome 注入 = 独立 `NCX_GENOME` TOML 文件**（env 指定），不污染用户 config；
   解析复用 `ncx-config` 的 TOML 能力。default genome 与现有 const 字节级等价。
4. （未定，TaskGen 阶段再议）能力维度清单：先人工列 6~8 个种子维度。

### M0 范围冻结（据上述决策 + 对抗评审修正）

可进化字段仅：`system_prompt`、`tool_desc.*`（allow-set 从 ncx 实际工具集派生）。
固定不动：所有 orchestrator 基因、memory_seed。
评测 arm：`nanocodex`（非 `-o`）。教师：codex+claude+api panel（§10.5）。注入：`NCX_GENOME` 文件。
**硬前置：P1 genome 注入 + P2 轨迹采集必须先落地并自检通过**（§2.1/§2.2），否则整轮空跑。
验收：自检 gate 通过 + ≥5 代 + champion 在 **held-out** 不低于 gen0 baseline + 报告/lineage(真实模型 id)。

### 对抗评审（workflow，6 agent）已确认的关键事实
- `NCX_GENOME` / genome 注入**当前不存在**（grep 零命中）→ 必须先建（P1）。
- `bench/run.py` **不留 agent 轨迹**（弃 stdout、rmtree 工作区）→ 必须先建（P2）。
- codex 本机经 **CLIProxyAPI 代理**，model=`gpt-5.4`；硬编码 `-m gpt-5` 报 502 → 模型从 config 解析。
- claude 本机 **401**，但 `rc=0 + is_error:true` → 按结构化字段判可用性。
- `apply_patch` 描述 ≈8125 字符且 load-bearing → size cap 从基线取，加 round-trip 测试。
- 真正成本在**内层 bench 循环**（候选×任务×repeats×≤60 轮）→ 需全局 token/时钟 governor。
```
