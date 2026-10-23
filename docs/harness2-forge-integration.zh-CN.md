# 技术说明：Harness² Propose–Probe–Compose 接入 `train/forge.py`

> 基线：commit `0294692`（RRSI 正则化）。
> 文档状态：**已实现（2026-10-23）** —— 包含 `train/evaluator.py`（`keep_trajectory`）、`train/lesson_lib.py`（跨轮 `lessons.jsonl` + `adopted_harnesses.jsonl` 检索/更新/退役/消融反写）、`train/teacher.py`（`build_h2_propose_prompt` / `build_h2_compose_prompt` / `parse_lesson`）、`train/forge.py`（`Probe` / `H2Round` / `build_contrast` / `_run_probes` / `_h2_round` 及 `--h2-k` 等 CLI 参数）、以及 `evals/compounding_replay/replay.py`（`--genome PATH`）。
> 说明：Harness² 机制描述依据论文要点（arXiv 2610.14224），并与 Rust 运行时 L0/L1 Harness² 闭环（[`docs/self-evolving-harness-rrsi-harness2.zh-CN.md`](self-evolving-harness-rrsi-harness2.zh-CN.md)）协同工作。

## 1. forge.py 现状

### 1.1 本地实际运行方式（只读调查结果）
在 Windows 机器 `D:\github_dgy\BugleCat`（HEAD 同为 `0294692`）上：
- **`train/runs/` 不存在**——本地从未产生 ledger/lineage，即 RRSI 版 forge **尚未在本地跑过**。
- 没有调用 forge 的 `.ps1/.cmd`（`scripts/run-eval.ps1` 只构建 `ncx.exe` 并跑 `bench`），PSReadLine 历史中无 `forge` 记录。
- 因此**未发现实际本地运行命令**。按 HANDOFF.md 推荐与 argparse（L654–701）推断的等价命令：
```powershell
$env:NCX_FORGE_NCX_BIN="D:\github_dgy\BugleCat\rust\target\release\ncx.exe"
python train/forge.py --train --rounds 3 --repeats 3 --timeout 120 --budget-s 1800 `
  --teacher panel --initial-max-surfaces 2 --max-changed-lines 40 --stagnation-limit 2 --prune
```
（`--max-surfaces` 现为 `--initial-max-surfaces` 的别名；HANDOFF/README 已统一为 `--initial-max-surfaces`。）

### 1.2 循环结构（`train()`，L208–507）
| 步骤 | 代码 |
|---|---|
| teacher 面板 | `teacher.build_panel()`（teacher.py L186），按 `--teacher` 过滤 |
| gen0 | `genome.extract_current()` → 保存 `genomes/<stamp>_gen0_start.toml` → `ev.evaluate()` train/holdout |
| 每轮 | 预算检查 → 可选重评 incumbent → `champ_train.failing_trajectories(top_k=3)`（L279） |
| 正则 | 退火 `max_surfaces = max(1, initial-(rnd-1))`；停滞时 `_pick_stagnation_surface()`（L122） |
| 提议 | `teacher.build_teacher_prompt(champion, failures, rejected_history, focus_surface, max_surfaces)`（teacher.py L203） |
| 每个 backend（L314） | `backend.propose()` → `parse_candidate()` → `genome.validate()`（尺寸/面数/行数）→ `teacher.critic_check()`（反记忆）→ 存 TOML → `ev.evaluate(train)` |
| 验收（L377–） | 取 passes 最高/token 最低者；双通道门：pass-gain(`margin>=accept_margin` 且非 token-bloat) 或 token-saving(`delta_tokens`)；再要求 holdout 不退步 |
| 记录 | 每个候选 `append_ledger()`（L113）→ `runs/ledger_<stamp>.jsonl`；拒绝项进 `rejected_history` |
| 结束 | `--prune` → `prune_genome()`（L133）；保存 champion；可选 test；写 `lineage_<stamp>.json` |
`evolve()`（L524）是 Pareto 种群版本，同样的 panel 循环（L588）。

### 1.3 隔离机制（重要事实）
forge **不使用 git worktree**。隔离单位是 genome TOML 文件：`evaluator._run_task_once()`（L161）为每次 run 建 `tempfile.mkdtemp` 工作区、`bench.seed()`、通过环境变量 `NCX_GENOME=<toml>` 让 Rust 运行时（`ncx-core/src/genome.rs`）覆盖 system_prompt / tool_desc / orchestrator_prompts / memory_config 四类 surface，结束 `rmtree`。因此 Probe 天然可并行、条件对齐，无需 worktree。

## 2. Propose–Probe–Compose 接入点

### 2.1 映射
| Harness² | forge 对应 |
|---|---|
| Harness | `genome.Genome`（4 类 surface） |
| Task | 一个失败的 train 任务（`failing_trajectories` 中的一项） |
| Propose K | 对**单一任务**生成 K 个 genome 编辑：一半"通用"、一半"领域特定"（针对该任务失败模式但仍需过 critic） |
| Probe | 在**同一任务**、相同 timeout/model/repeats/seed 下各跑 K 个 genome（+ 当前 champion 作为对照 probe 0） |
| Compose | teacher 读取 K+1 条执行轨迹的**行为对比**（不看 grader 结果——见 2.5），产出 1 个合成 genome |
| 外层验收 | 合成 genome 进入**现有** validate → critic → train eval → 双通道门 → holdout 流程，不变 |

插入位置：替换 L299–375 的 "build prompt + for backend in panel" 段为 `if h2_k>0: candidates = _h2_round(...) else: <原逻辑>`。

### 2.2 新 CLI 参数 [新]
```
--h2-k K                 每个任务 probe 的候选数（0=关闭，保持原行为）
--h2-mode parallel|sequential
                         parallel: K 个同时提议并 probe；sequential: 第 i 个提议看到前 i-1 个 probe 的对比再递归
--h2-tasks N             每轮挑几个失败任务做 PPC（默认 1，按失败率排序）
--h2-probe-repeats R     probe 重复次数（默认 = --repeats，建议 2 以压噪声）
--h2-workers W           probe 并发进程数（默认 min(K+1, 4)）
--h2-blind               compose 时隐藏 pass/fail（论文的"无 ground-truth verifier"模式，默认开）
--lessons PATH           改进库路径（默认 train/runs/lessons.jsonl）；--no-lessons 关闭
```

### 2.3 数据结构 [新]
```python
@dataclass
class Probe:
    idx: int                 # 0 = champion 对照
    kind: str                # "control" | "general" | "domain"
    genome: genome.Genome
    path: Path               # genomes/<stamp>_r{rnd}_t{task}_p{idx}.toml
    surfaces: list[str]
    diff: str
    trajectory: str          # 由 evaluator 产出（需新增 keep_trajectory_on_pass）
    passed: int; runs: int; mean_tokens: float; mean_s: float
    rationale: str           # teacher 给出的假设

@dataclass
class H2Round:
    task: str
    probes: list[Probe]
    contrast: str            # 结构化对比文本
    composed: genome.Genome | None
    lessons_used: list[str]  # lesson id
```

### 2.4 核心骨架 [新]
```python
def _h2_round(rnd, task, traj, champion, baseline, panel, *, k, mode, max_surfaces,
              focus_surface, max_changed_lines, forbidden_terms, rejected_history,
              lessons, timeout, repeats, stamp):
    hint = lesson_lib.retrieve(task_signature(traj), k=5)          # §3
    probes = [_make_control_probe(champion, task, timeout, repeats)]
    for i in range(k):
        ctx = probes if mode == "sequential" else probes[:1]
        prompt = teacher.build_h2_propose_prompt(                    # [新]
            champion, task, traj, kind=("general" if i % 2 == 0 else "domain"),
            prior_probes=ctx, lessons=hint, rejected_history=rejected_history,
            focus_surface=focus_surface, max_surfaces=max_surfaces)
        cand, why = teacher.parse_candidate(panel[i % len(panel)].propose(prompt) or "", champion)
        if not cand or genome.validate(cand, baseline, parent=champion,
                max_surfaces=max_surfaces, max_changed_lines=max_changed_lines) \
                or teacher.critic_check(cand, champion, forbidden_terms=forbidden_terms):
            _ledger_reject(...); continue                            # 复用原拒绝路径
        probes.append(_pending_probe(i + 1, cand, ...))
        if mode == "sequential":
            _run_probes(probes[-1:], task, timeout, repeats)        # 立即跑，供下一提议看到
    if mode == "parallel":
        _run_probes(probes[1:], task, timeout, repeats, workers=h2_workers)
    contrast = build_contrast(probes, blind=h2_blind)
    composed, why = teacher.parse_candidate(
        panel[0].propose(teacher.build_h2_compose_prompt(champion, contrast, hint)) or "", champion)
    return composed, probes, contrast

def _run_probes(probes, task, timeout, repeats, workers=1):
    with ThreadPoolExecutor(workers) as ex:   # 每个 run 自带 mkdtemp 工作区，可安全并发
        for p, r in zip(probes, ex.map(lambda p: ev.evaluate(str(p.path), [task], repeats, timeout,
                                                             keep_trajectory=True), probes)):
            tr = r.tasks[task]; p.passed, p.runs, p.mean_tokens = tr.passes, tr.runs, tr.mean_tokens
            p.trajectory = tr.trajectory   # [新字段]
```
需要的小改动：
- `evaluator._run_task_once`/`evaluate` 增加 `keep_trajectory: bool=False`，通过时也保留轨迹（对比需要成功轨迹）；`TaskResult` 增加 `trajectory` 字段。默认 False 保证兼容。
- `evaluate` 只是串行循环；并发放在 forge 侧线程池（子进程由 `run_owned` 管理，需确认 `process_control.py` 线程安全）。

### 2.5 对比分析 → teacher Compose
`build_contrast(probes, blind)` [新]，确定性生成，不调 LLM：
- 每个 probe：diff 摘要、工具调用序列（从 `extract_trajectory` 文本中抽取 tool 名序列）、步数、token、耗时、是否超时、末尾 300 字符。
- 两两差异：首个分歧步、工具使用差异（如 A 多跑了 targeted pytest）、token 差。
- `blind=True` 时**不暴露 pass/fail**，只给行为；这对齐论文"无 verifier compose"，同时降低对 grader 过拟合风险。`blind=False` 作为消融。
- 轨迹统一包在 `<<<UNTRUSTED ... >>>UNTRUSTED` 中，复用 `UNTRUSTED_PREAMBLE`。

`teacher.build_h2_compose_prompt` [新]：要求"从对比中提炼 1–2 条**通用**行为规则写入 genome，并输出一条 lesson（when/why）"，输出两个块：```toml（genome 补丁）和 ```json（lesson 草案）。`parse_candidate` 只取 toml 块，新增 `parse_lesson()` [新] 取 json。

### 2.6 与 RRSI 机制的交互
| 机制 | 处理 |
|---|---|
| 退火面预算 / `max_changed_lines` | 每个 probe **和** 合成 genome 都过 `genome.validate`；合成后 surface 数 ≤ max_surfaces（合成最易越界，必须再验） |
| `critic_check` | 对 probe 和合成都跑；"领域特定"提议是主要风险点，forbidden_terms 不变 |
| 双通道成本门 / holdout | 只对**合成 genome**执行，完全复用 L377–465；probe 结果**不能**直接晋级（probe 仅在单任务上跑，样本太小） |
| `--max-token-growth` | probe 记录 token；compose prompt 告知 token 预算 |
| 停滞转向 | `focus_surface` 传给 propose；仅当本轮**未接受**新 champion 时计为一次停滞（接受即清零，即使 probe 无行为差异；该信号记录在 `round_log.h2_behavioral_variation`） |
| ledger | 新增 `phase` 字段：`"h2-probe"`/`"h2-compose"`，含 `task`、`probe_idx`、`kind`、`passed`、`contrast_hash`；probe 的失败也进 `rejected_history`（标 `phase=h2-probe` 便于去重） |
| `--prune` | 不变，在最终 champion 上做消融；被剪掉的 surface 反写 lesson 的 `refuted`（见 §3） |
| `budget_s` | 每个 probe 前检查 `elapsed()`；PPC 成本 ≈ (K+1)·R 次单任务 run + K+1 次 teacher 调用 |
| `evolve()` | 第一阶段只接 `train()`；`evolve()` 可在 L588 用相同 `_h2_round` 替换，后续阶段 |

## 3. 改进库（harnessing-level recursion）

### 3.1 文件格式 [新] `train/runs/lessons.jsonl`（跨 stamp 持久，追加写+定期压实）
```json
{"id":"L-0007","created":"2026-10-23T15:00+09:00","stamp":"20261023_1500",
 "trigger":{"symptoms":["timeout","repeated full-suite pytest"],"surfaces":["system_prompt"],"task_family":"python-bugfix"},
 "edit_pattern":"要求只跑 targeted pytest 并在通过后立即结束",
 "why":"probe 对比：采纳该规则的执行少 6 步、未超时",
 "evidence":[{"stamp":"...","round":2,"task_sig":"sha1..","verdict":"accepted","margin":1,"token_saving":-120}],
 "support":2,"refute":0,"status":"active"}
```
同时维护 `adopted_harnesses.jsonl`：每次 ACCEPT 的 genome 路径 + 所用 lesson id（论文的"adopted harnesses"）。

### 3.2 检索
`lesson_lib.retrieve(sig, k)` [新]：`task_signature(traj)` 抽取症状标签（超时/无日志/patch 失败/重复工具调用等，规则匹配）+ 关键词；按 `symptom 交集 × (support+1)/(support+refute+2)` 打分，`status=active` 优先，返回 top-k。保持确定性、无 embedding，和 Rust `memory.rs::select_recall`/`semantic_score` 的关键词+别名思路一致（可移植其 CJK 分词逻辑）。
**禁止**把 task 名写入 lesson（critic 的 forbidden_terms 同样用于 lesson 文本校验）。

### 3.3 更新规则
- compose 产出 lesson 草案 → 仅当合成 genome **通过外层门 + holdout** 才写入，`support=1`。
- 已有 lesson 被检索并用于被接受的合成 → `support+=1`；用于被拒合成（below-margin / holdout-regressed / token-bloat）→ `refute+=1`。
- `--prune` 剪掉的 surface 若对应某 lesson → `refute+=1`。
- `refute >= support+2` → `status=retired`（不检索但保留，作为"falsified"注入 propose 提示的禁止清单，与 `rejected_history` 同构）。
- 压实：相同 `edit_pattern` 归一化后合并证据。

### 3.4 与现有记忆的关系
- `rejected_history` / `ledger_*.jsonl`：**单次 run 内**的证伪记录 → lessons 是**跨 run**、带 when/why 的正负证据汇总。可提供 `lesson_lib.bootstrap_from_ledgers()` 从历史 ledger 冷启动。
- Rust 运行时两层记忆（`memory.rs`，`.ncx/` 工作区状态）是**被优化 agent 的任务时记忆**，lessons 是**优化器（teacher）的元记忆**，二者不混用；`memory_config` surface 仍由 genome 控制。
- 不进入 `NCX_GENOME`，不影响 Rust 字节级 no-op 保证。

## 4. 评测计划（evals/compounding_replay）
`replay.py` 现有 Arm A（无跨任务记忆）/ B（状态延续），已有 P3 的 B'。PPC 产出的是 genome，因此评测方式是"用不同 genome 跑同一 replay"（需给 replay.py 加 `--genome PATH` [新]，设置 `NCX_GENOME`）。

| Arm | genome |
|---|---|
| B | 默认（无 genome） |
| B-RRSI | `forge --train` 现有 RRSI 流程产出 champion |
| B-H2p | `--h2-k 3 --h2-mode parallel` |
| B-H2s | `--h2-k 3 --h2-mode sequential` |
| B-H2p+L | 同 B-H2p + lessons 库（两次 forge：第一次积累，第二次用） |
| 消融 | `--no-h2-blind`；K=1 vs 3 |

- 进化数据：forge 用 `bench` 任务 train/holdout；**replay 课程（tabulate / sqlparse_split）只作 test**，避免泄漏。
- 指标：resolved 率（隐藏 judge）、平均 token 与美元成本（replay 已有 cache-aware 计价）、步数、超时率；进化侧：每次 ACCEPT 的 teacher 调用数与 probe 数、接受率、holdout 增益、lessons 的 support/refute 比。
- 统计：每 arm 3 个 seed-label（r1–r3），配对 McNemar / bootstrap，先测 δ（HANDOFF G1：同 genome 重跑噪声）再判定有效。
- 成本粗估（以 rounds=3、h2-tasks=1、K=3、probe repeats=2 计）：每轮 probe 8 次单任务 run + 4 次 teacher 调用；对比现有每轮 = |panel| 次 teacher + |panel|×|train|×repeats 次 run。若 train 有 6 任务、panel 2，原流程约 12 run/轮，PPC 额外约 +8 run/轮（~+65%）。replay 每 arm 每轮按 `--round-budget` 封顶，6 arm × 3 seed × 12 任务 ≈ 216 次 task run，按 replay 现有记录的单任务成本乘算，先跑 `--limit 2` smoke 获得单价再定预算。

## 5. 分阶段实施与风险
1. **P0 基础**：evaluator `keep_trajectory`；ledger 加 `phase` 字段；先在本地跑一次现有 `--train` 生成 `train/runs/` 作为基线（本地当前没有任何 run）。
2. **P1 Parallel PPC**：`Probe`、`_run_probes`、`build_contrast`、`build_h2_propose_prompt`/`build_h2_compose_prompt`、`--h2-k/--h2-mode parallel`；单测用 stub backend + stub evaluate（沿用 test_forge.py 风格）。
3. **P2 Sequential**：递归提议，按 `budget_s` 截断。
4. **P3 Lessons**：`train/lesson_lib.py` [新]：retrieve/update/compact/bootstrap_from_ledgers；单测更新规则。
5. **P4 评测**：replay `--genome`，跑 §4 arms；`evolve()` 接入可选。

风险：
- **噪声**：单任务 probe 样本小，对比可能是随机差异 → probe repeats≥2、合成必须过全量 train+holdout 门。
- **过拟合/泄漏**："领域特定"提议易写入任务特征 → critic 同时校验 probe、合成与 lesson 文本；replay 课程只作 test。
- **成本膨胀**：K+1 倍单任务运行 → `--h2-tasks` 默认 1，受 `budget_s` 与 token 门约束。
- **并发**：Windows 下并发 `ncx.exe` 子进程与 `run_owned` 进程组管理需验证；默认 workers 保守。
- **Compose 越界**：合成易超出退火面预算 → 强制再 validate，越界/解析失败/预算耗尽则回退到单个 probe 候选再走外层门（train + holdout）。回退选择遵守 blind：`blind=True` 时**不读取 probe 的 pass/fail**，按纯行为确定性排序（未超时优先 → 与 control 的行为差异最大 → token 更少 → probe 编号最小）；仅 `--no-h2-blind` 消融时按单任务 probe 通过数选择。
- **lesson 漂移**：错误 lesson 自我强化 → 只在外层门接受后写入，refute 自动退役。
- 文档一致性：已统一为 `--initial-max-surfaces`（`--max-surfaces` 保留为别名）。
- **lesson 库并发/持久化**：`lessons.jsonl` 读改写在跨进程锁（`lessons.jsonl.lock`，`O_CREAT|O_EXCL`，120s 视为陈旧锁）下进行，写入走临时文件 + `os.replace` 原子替换；新 lesson ID 取现有最大编号 +1；同 `edit_pattern` 仅在 symptom 有交集时合并（trigger.symptoms 上限 8 条），否则另建 lesson。
