# ncx-forge — 训练框架清单（inventory）

> 让强模型当"教师"迭代优化 agent 骨架（system_prompt + 工具描述），用 bench 通过率当 fitness 做
> 闭环进化；并把成功轨迹导出为 SFT/RL 数据 + agentic-RL rollout 收集器，作为通往权重训练的桥。
> **被训对象 = Rust `ncx.exe`**；训练器 = `train/` 纯 Python。设计详见 [`DESIGN.md`](DESIGN.md)。
> 分支 `feat/train-rl`（RL 续做）；主体已并入 `rust-capability`。

## 1. 组件清单

### Rust 侧（被训 agent 的可调骨架 + 钩子）— 已并入 rust-capability
| 文件 | 作用 |
|---|---|
| `rust/crates/ncx-core/src/genome.rs` | `Genome`：读 `NCX_GENOME` TOML 覆盖 base system_prompt + 每工具描述；空/缺失=字节等价 no-op |
| `ncx-cli/src/main.rs` | 启动加载 genome 注入；`--dump-genome`（吐默认 genome）；一次性模式 stderr 打 `[ncx-usage] total_tokens=N` |
| `ncx-cli/src/args.rs` | `--dump-genome` flag |
| `ncx-core/src/tools.rs` | 注册层按 genome 覆盖工具描述（`schema_for` + catalog） |

### Python 训练器 `train/`（19 文件，~3.4k 行）
| 文件 | 作用 | 入口/关键 API |
|---|---|---|
| `genome.py` | Genome 读写/校验(size cap 从基线取)/diff；`extract_current()` 经 `ncx --dump-genome` | `python train/genome.py`（round-trip 自检） |
| `evaluator.py` | 跑 ncx(注入 genome+`-m` 模型)评测；从 session.jsonl 抽失败轨迹(剔 grader 行)；token 解析 | `evaluate()` / `--genome --tasks` |
| `teacher.py` | 教师 panel：codex(GPT,模型从 config 解析)/claude(Opus,按 is_error 判)/api(DeepSeek 地板) | `build_panel()` |
| `splits.py` | 任务级 train/val/test 切分（`splits.json` 真相源，确定性派生） | `python train/splits.py` |
| `taskgen.py` | 教师造题 + **自校验门**（参考解过 check×2 + seed 态失败才入库） | `python train/taskgen.py -n N` |
| `admit_batch.py` | 把一批候选任务 JSON 过自校验门入库 | `python train/admit_batch.py tasks.json` |
| `pareto.py` | 多目标：dominance / Pareto front / NSGA-II crowding trim | — |
| `viz.py` | lineage JSON → 自包含 HTML（Pareto 散点 + 血缘表） | `python train/viz.py lineage.json` |
| `forge.py` | **主控**：`--self-check`(sentinel 门,重试) / `--baseline` / `--train`(单 champion 爬山) / `--population`(Pareto 种群) | 见 §3 |
| `export.py` | 跑 genome×任务 → 完整轨迹+reward+tokens → SFT/RL JSONL | `python train/export.py` |
| `finetune.py` | 权重训练脚手架：SFT(trl SFTTrainer,懒加载) + RL 奖励/契约 | `python train/finetune.py --mode sft\|grpo\|prep` |
| `rollout.py` | **agentic-RL rollout 收集器**：episode(model↔tools)+`bench_reward`+`grpo_advantages`+`ncx_episode`(复用 ncx) | `python train/rollout.py`(优势 demo) |
| `test_*.py` (8) | 单测：pareto6/population4/forge5/evaluator7/taskgen6/export2/finetune5/rollout5 = **40** | `python train/test_X.py` |
| `DESIGN.md` / `INVENTORY.md` | 设计文档 / 本清单 | — |

### bench/（复用，评测奖励源）
- 18 个 committed 任务 `t1`–`t18`（含 promote 的 5 个难任务 t14–t18）；`bench/tasks/gen_*` 机器造题(gitignore，审查后 promote)。

## 2. 能力清单（✅ 已落地）

- [x] **骨架注入** `NCX_GENOME`（system_prompt + 工具描述），空=字节等价
- [x] **失败轨迹采集**（session.jsonl，剔除 grader 输出）
- [x] **教师 panel**（codex/claude/api，探测可用、优雅降级）
- [x] **训练闭环**：gen0→教师提议→评测→接受门→lineage
- [x] **自检门**（sentinel 注入确定性验证，含重试防噪声）
- [x] **抗过拟合**：train/val/test 切分 + 噪声感知接受（重评 incumbent + margin）
- [x] **自校验造题**（TaskGen：参考解过 check + seed 态失败才入库）
- [x] **多目标搜索**：Pareto 小种群（pass↑/cost↓，NSGA-II crowding，逐代重评）
- [x] **可视化**：lineage + Pareto 散点 HTML
- [x] **真 token 成本**（ncx 吐 `[ncx-usage]` → Pareto cost）
- [x] **弱 base 真 lift**（deepseek-chat 默认骨架 0.67→1.00，codex 教师）
- [x] **SFT/RL 数据导出**（reward 标注的完整轨迹 JSONL）
- [x] **权重训练脚手架**（SFT trainer + agentic-RL rollout 收集器 + 验证奖励）
- [ ] **GPU 权重训练**（需 GPU：跑 finetune SFT / 接 `run_grpo` 的 `policy_update`）— 唯一未做

## 3. 命令清单

```bash
# 验证注入活着（确定性门，重试防噪声）
python train/forge.py --self-check
# 基线通过率
python train/forge.py --baseline --tasks t1_mathutils,t3_fizzbuzz
# 单 champion 爬山训练（切分自 splits.json；--teacher panel|codex|api）
python train/forge.py --train --rounds 3 --teacher codex
# Pareto 小种群（多目标 + 可视化）；--base-model 训更弱 base
python train/forge.py --population --pop-cap 4 --base-model deepseek-chat --teacher codex
# 造题（自校验入库 bench/tasks/gen_*）
python train/taskgen.py -n 8
# 导出 SFT/RL 数据
python train/export.py --tasks t1_mathutils --reward-pass-only --out train/data/sft.jsonl
# 权重训练（GPU 机）：pip install 'trl>=0.9' transformers torch peft datasets
python train/finetune.py --mode sft --data train/data/sft.jsonl --model <hf-model>
python train/finetune.py --mode grpo   # 指向 rollout.py 的 agentic 收集器
```

## 4. 数据流

```
extract_current()──genome──▶ Evaluator(跑 ncx，-m/NCX_GENOME) ──分数+失败轨迹──▶ Teacher(codex/claude/api)
        ▲                                  │                                          │
   接受门(噪声感知/Pareto)◀── 评测候选 ◀────┴──候选 genome◀── reflective mutation ◀──┘
        │                          TaskGen(自校验)──新任务──▶ bench/tasks
   champion + lineage(JSON/HTML)
        │
   export.py──(system_prompt,messages,reward,tokens)──▶ SFT/RL JSONL ──▶ finetune.py(SFT) / rollout.py(GRPO episode) ──[GPU]──▶ 权重
```

## 5. 配置与约定
- **注入**：env `NCX_GENOME=<toml>`（训练态专用，不设=默认）。
- **教师**：codex 模型从 `~/.codex/config.toml` 解析（本机经 CLIProxyAPI=gpt-5.4）；claude 401 则跳过；api 用 `$DEEPSEEK_API_KEY`。
- **弱 base**：`--base-model deepseek-chat`（默认骨架 headroom 更大，更易看出教师抬升）。
- **gitignore**：`train/genomes/`、`train/runs/`、`train/splits.json`、`train/data/`、`bench/tasks/gen_*`（本地产物/可复现）。
- **隔离开发**：在独立 `git worktree` 上做（主 checkout 有并行会话）；用绝对路径/`git -C`/`--manifest-path`。

## 6. 测试与状态
- **40 Python 单测** + Rust genome 单测（7）全绿；多处 live 验证（注入/造题/弱base lift/导出/rollout 循环）。
- 主体已并入 `rust-capability` 并推 origin；RL 续做在 `feat/train-rl`。
- **本机功能面 100% 闭环**，唯一剩项 = 需 GPU 的权重更新（数据/脚手架/奖励/rollout 均已就绪）。
