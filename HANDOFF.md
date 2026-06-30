# HANDOFF — nanocodex (Rust 线)

> 新接手的 agent：先读完再动手。与上一级 `D:\agent_prac\HANDOFF.md`（面试准备）是两条独立线。
> Python 时代历史在 git 历史 + SESSION_MEMORY.md。

## 元信息
- 最后更新：2026-06-29
- 分支：**`rust-capability`**（整合线，推 **`origin/gui-merge-featgui`**；`origin/rust-capability` = codex
  的独立 GUI 线，**勿覆盖**）。Python 树 `nanocodex/*.py` 不动。
- remote：`origin` → https://github.com/dgy-github/nanocodex.git（凭据已配）
- 路径：crates `rust/crates/`，GUI `rust/gui/`，基准 `bench/`。
- 工具链：无 MSVC，用 `x86_64-pc-windows-gnu`；每条 cargo 前 `export PATH="$HOME/.cargo/bin:$PATH"`。
- ✅ **`feat/train` 已并入 rust-capability**（merge `a26793b`）：ncx-forge 训练框架全部回灌 —— `genome.rs` 读 `NCX_GENOME` 覆盖 prompt/工具描述、`--dump-genome`/`--from-genome` CLI、`train/` 纯 Python 框架。详见下节 + `train/DESIGN.md`。

## 最近改动（2026-06-29，已测，已推 `origin/gui-merge-featgui`）
> ⚠️ push 坑：`rust-capability` 默认 upstream 误指 `origin/rust-capability`（codex 线）。**推必须显式**
> `git push origin rust-capability:gui-merge-featgui`，别裸 `git push`。

- **合入 feat/gui 完整前端**（merge `9d623cc`）：feat/gui 的 1490 行 GUI（侧栏会话列表/最近会话、resume+fork、
  git·diff·记忆·文件·checkpoints 面板、slash 面板、token 流式+用量、4 模式权限、中文化）并入整合线。
  **之前给精简 GUI 加的 token 条（b78dfba）被 feat/gui 自带 usage 取代**。6 文件冲突已解（GUI 文件取 feat/gui，
  lib.rs/main.rs/.gitignore 取并集）；`.gitignore` 现忽略 `.nanocodex/`、`.ncx/`。
- **CLI slash 扩展**（早于合并）：`/export`（会话→md，动态围栏、拒覆盖/拒目录）、`/review`·`/security-review`·
  `/verify`、`/docx·pdf·pptx·xlsx`（prompt+shell 调后端，未装给 pip 并征询）、别名 `/update-config`·`/usage-credits`。
- **eval 数据持续更新机制**（`26b9b76`）：`templates/eval-data-pipeline/`——可移植模板（自定义命令 + analyze
  skill + 日报 + gate 文档）+ `example/` 自包含可跑参考（`run_pipeline.py --self-check` + `eval.py`，纯 stdlib，
  13 行合成快照→4 候选）。原则：生产只采集、本地 agent 分析提案、CI/人审做 gate；daily loop 对 eval 集**只读**。
- **与 codex 线对齐**（codex 独有项全部摘到整合线）：
  - `c167348` **per-prompt 记忆召回**（agent_loop 按 prompt 召回为 per-turn note；去掉 main/runner/bridge 的启动静态召回）+ **`--mcp` 启动门**（默认不起 MCP，加 `--mcp` 才连）。
  - `20cd53b` GUI **打开会话日志/快照/记忆文件**（`open_session_log`/`open_session_snapshot`/`open_memory_file`）。
  - `056a622` GUI **自定义命令面板**：把自定义命令引擎抽到 **`ncx-core::custom_commands`**（CLI+GUI 共享、去重）+ `get_custom_commands`/`expand_custom_command` + App.svelte slash 面板并入自定义命令（选中展开进输入框；`runSlash` 清空前先捕获尾随参数）。
  - 跳过 codex 的 `remember_note`（本线 `memory_add` 已覆盖）。
- 测试：全 rust 工作区 **273 绿**；GUI 后端 `cargo check` + 前端 `vite build` 均过；GUI 已实跑（含自定义命令面板）。
- 坑：`tauri dev` 报「Port 5179 already in use」= 上次残留 vite 孤儿占端口；`taskkill //F //PID <node>` 后重启即可。

## ncx-forge 训练框架（分支 `feat/train`，已推 origin）— 当前活跃工作线
目标：让强模型当"教师"迭代优化 agent 骨架（system_prompt + 工具描述），用 bench 通过率当
fitness 做闭环进化。**只训 Rust 版 `ncx.exe`**；权重不动，纯 API。完整设计见 `train/DESIGN.md`。

- **隔离开发**：在独立 worktree `D:/agent_prac/ncx-train` 上做（主 worktree 有并行会话在
  thrash + 一个 Codex agent 重置 cwd）。接手请 `git worktree add <dir> feat/train` 后在其中干，
  **用绝对路径 / `git -C` / `--manifest-path`**，别依赖 cwd。
- **M0a ✅（地基）**：
  - P1 `NCX_GENOME` 注入（`f1af9ce`）：`ncx-core/src/genome.rs` 读 TOML 覆盖 system_prompt +
    工具描述；覆盖在注册层应用（`schema_for`/catalog），空 genome **字节等价**。
  - P2 失败轨迹采集（`train/evaluator.py`）：跑 ncx 注入 genome，从 `<ws>/.nanocodex/session.jsonl`
    抽 agent 末条消息+工具调用，**剔除 grader 行**（check.py 不外泄）。
- **M0b ✅（最小闭环）**：
  - `ncx --dump-genome`（`90d0a20`）吐默认 genome → `train/genome.py` extract-current + 校验
    (size cap 从基线取) + round-trip。
  - `train/teacher.py` 可插拔 panel：**codex(GPT，模型从 `~/.codex/config.toml` 解析) + claude
    (Opus，按 `is_error` 判) + api(DeepSeek 地板)**。npm shim 用 `shutil.which` 解析 `.CMD`。
  - `train/forge.py`：`--self-check`（sentinel 注入门，确定性）/`--baseline`/`--train`（gen0→
    每代教师提议→评测→**接受门:train升+holdout不退**→JSON lineage + wall-clock governor）。
  - **live 验证**：codex(gpt-5.4) 与 api(deepseek) 都真实产出合法候选 genome（动 prompt/澄清
    shell，**不动 apply_patch**）；forge --train 端到端跑通；接受门 monkeypatch 单测 3/3；P2 单测 5/5。
- **M1 ✅（抗过拟合，`4e36738`）**：`splits.py`(task 级 train/val/test) + `taskgen.py`(教师造题，
  **自校验**：参考解过 check×2 + seed 态失败才入库，→ `bench/tasks/gen_*` gitignore) +
  forge 噪声感知接受(每代重评 incumbent + `--accept-margin` + test 末尾打无偏分)。
  live：api 造出 Unicode/ZWJ 重叠子串难任务并入库；trivial 任务被正确拒。6+3+5 单测全过。
- **临门一脚已做（真能训验证）**：workflow 12 个 Opus 并行造题 → 自校验门 **9/12 入库**
  （3 个"参考解过不了自己的 check"被正确拒）→ bench 现有 10 个 gen_* 难任务（gitignore）。
  baseline 扫：deepseek-v4-pro **9/10 全过**（仅 stable_topo 失败）→ 强基线，harness 余量薄。
  `forge --train`（train=stable_topo）**全闭环跑通**：gen0 0/1 → 教师(api)真提出合法变异
  (system_prompt 192→748 + web_fetch 描述) → 评测仍 0/1 → 噪声接受门**正确拒绝**(+0<margin) →
  无回归。**结论：框架真能训**（propose→validate→evaluate→accept 全活、不伪造提升）；本轮教师
  没抬升，因 codex/claude 当时不可用、教师=agent 同模型 + 硬推理任务 prompt 改不动（印证
  *model is the lever*）。
- **修了个 live bug**（`21400af`）：失败任务若 timeout→空轨迹，旧逻辑误判"train 全过"停在 gen0；
  现 evaluator 给无轨迹失败合成信号（"timed out"），forge 区分"全过"与"有失败但无信号"。
- **codex(gpt-5.4) 教师重跑已做**：codex 恢复可用，`forge --train --teacher codex`
  (train=stable_topo+csv) 全闭环跑通：gen0 1/2 → codex **两轮都提出实质合法变异**
  (R1 system_prompt 192→663 + read_file/shell/update_plan 扩写；R2 192→866 不同改法) →
  两轮评测都 **1/2 无提升** → 接受门**均正确拒绝**(+0<margin) → 无回归。耗时 1321s。
  **结论：即便上 gpt-5.4 强教师，也没抬升 deepseek agent 在这些算法任务上的通过率** ——
  因为这些 task 的失败是底层推理/效率所致、非 prompt 可修；强力印证 *model is the lever*。
  框架本身完全正确：强教师真engaged、提出高质量候选、噪声门顶住不伪造提升。
- **骨架敏感任务 + 逼出 lift（已做，capstone）**：workflow 造 8 个"prompt-可修习惯"任务
  （exact ValueError 契约/无 stdout/输入不可变/精确公共 API/精确返回类型/最小编辑…），自校验
  8/8 入库。但 **baseline 全过 16/16** —— 强 agent + nanocodex 默认骨架已经不踩这些坑，
  说明**真实默认骨架的 harness 余量也很薄**（model 与默认 prompt 都已够好）。
  于是做**诚实的优化器能力测试**：新增 `forge --train --from-genome <degraded.toml>` 从
  人为劣化的骨架起训（system_prompt 诱发 print/原地改/加 helper）。结果（codex gpt-5.4 教师）：
  **gen0 train 1/2 → R1 codex 重写 system_prompt(351→1345) → train 2/2 被接受**（margin≥1、
  holdout 1/1 不退、test 无回归）。**结论：headroom 存在时，优化器能真产出经噪声门+holdout
  验证的 lift**（`889078f`）；但默认骨架上余量薄 → 真实增益靠更强 model / prompt-可修的失败。
- **M2 ✅（搜索增强，`a6a47d2`）**：`pareto.py`（多目标 pass↑/cost↓ dominance+front+NSGA-II
  crowding，6 单测）+ `forge.py --population/--pop-cap`（`evolve()` 小种群，保 trade-off，空 eval
  →cost=inf 防误配夺冠）+ `viz.py`（lineage→自包含 HTML：Pareto 散点+血缘表）。3 population 单测；
  对抗复审判 pareto CORRECT(2万随机 0 违例)、evolve substantially correct（其 1 medium 已修）。
- **M2+ 收尾 ✅（`b88e023`+`8786cbb`）**：① promote 5 难任务进 committed bench（t14_overlap/
  t15_base_n/t16_csv/t17_running_stats/t18_rank_purity，均验 seed 失败+无泄漏解+baseline 可解）；
  ② `evolve` 加 `reeval_parents`（默认开，每代重评存活成员，防 lucky 早抽钉死 front）；
  ③ **ncx 一次性模式 stderr 吐 `[ncx-usage] total_tokens=N`**（唯一新增 Rust 改动，`main.rs`
  emit_usage_line）→ evaluator 解析进 `mean_tokens` → **Pareto cost 优先用真 token、无则回退 mean_s**
  （live：cost=33515 tokens）。26 Python 单测 + ncx-cli 全绿。
- **M3 + 弱base + 大种群 ✅（`3056b29`）**：① `train/export.py`——跑 genome×任务抓**完整轨迹**+
  reward+tokens 写 SFT/RL JSONL（`--reward-pass-only`=SFT 集；schema ncx-forge-trajectory/v1），
  live 验(reward=1/14 轮轨迹/真 token)；② `--base-model`（evaluator/forge 透传 `-m`）训**更弱 base**
  （deepseek-chat 余量更大）；③ `forge --population --base-model deepseek-chat --pop-cap 4` 大种群跑
  （结果见 train/runs/lineage_*.{json,html}）。28 Python 单测全绿。
- **🎯 弱 base 真 lift（默认骨架，已复现）**：`forge --population --base-model deepseek-chat`
  （codex gpt-5.4 当教师，train=t14/t16/t18）：**gen0 默认骨架 0.67 → gen1 codex 重写
  system_prompt(192→852)+read_file/shell/update_plan 描述 → 1.00**，Pareto cost 用真 token，
  lineage+viz HTML 已出。证明：**base 够弱（默认骨架有真 headroom）+ 教师够强时，框架能在
  默认骨架上真抬升**（不再需要人为劣化）。修了 cp1252 `→` 崩溃（UTF-8 reconfigure）。
- **gate 已加重试 ✅**：sentinel 自检对 with-genome 探测重试 ≤3 次（模型偶尔不回显码字是噪声、非
  注入失败），单次 miss 不再 block 训练；2 个新单测。
- **export system_prompt = genome base（有意，非缺陷）**：完整拼接 prompt 含 workspace 专属的
  项目指令/memory/skills，会污染可移植 SFT 数据；且把 system 写进 session.jsonl 会让 resume 重复。
  故 export 取**进化的 genome base**（更干净的训练信号）。
- **权重训练脚手架 ✅（`train/finetune.py`）**：`--mode sft`（export reward=1 → chat → trl
  SFTTrainer，trl/torch 懒加载）+ `bench_reward()` RL 奖励 + `rl_design()`（诚实：agentic RL 需
  GPU 侧 rollout collector，非 vanilla GRPO）。数据转换在本机可跑+5 单测；`--mode prep` 预览+打印
  GPU 运行命令。**真正训练只差一台 GPU**：`pip install trl transformers torch peft datasets` →
  `python train/finetune.py --mode sft --data <export.jsonl> --model <hf-model>`。
- **agentic-RL rollout collector ✅（`train/rollout.py`，分支 `feat/train-rl`）**：`collect_rollout`
  (注入 policy chat_fn + tool_exec 的 model↔tools episode，回合末 `bench_reward` 0/1) +
  `ncx_episode`(复用 ncx 真 loop，指向 vLLM-served policy，读 session.jsonl，**推荐生产路径**) +
  `grpo_advantages`(组内归一) + `collect_group`(N episode→优势)。纯逻辑本机可跑+5 单测；
  `run_grpo` 的 token 级 `policy_update` 是 GPU/torch 部分(懒加载+契约)。`finetune.py --mode grpo` 指到它。
- **下一步（仅剩需 GPU / 大算力）**：① 在 GPU 上把 `rollout.run_grpo` 的 `policy_update` 接上
  (vLLM 服 policy + ncx_episode 收 rollout + trl/自写 PG step)；② 跑 finetune.py SFT；③ 扩 corpus。
  **本机功能面 100% 闭环**（含 SFT 数据/脚手架 + RL rollout 收集器 + 验证奖励，只差 GPU 跑权重更新）。
- **diff() 小瑕疵**：champion 的 tool_desc 显示 "→0 chars" 是因 genome 未指定该键（=用默认），
  非真清空；注入对缺失键正确回落默认。diff 显示未区分"缺失"与"清空"，纯展示问题。
- **已知限制**：强基线 + 算法任务 = harness 余量薄；harness 优化对"模型能力门"无效，只对
  "工程习惯门"有效。教师必须比 agent 强，且任务失败须 prompt-可修，才可能抬升。
- **forge Do-Not**：① 别硬编码 codex 模型名（本机经 CLIProxyAPI 代理=gpt-5.4，`-m gpt-5`→502）；
  ② claude 401 是 rc=0+`is_error:true`，只能按字段判；③ api 地板优先用 `$DEEPSEEK_API_KEY`
  （config 里是 `ark_api_key`，未必对）；④ 自检别用"refuse genome→通过率降"（模型常无视，不可靠），
  用 sentinel 注入。

## 当前状态（已完成，约 225 测试全绿）
- 6 核心 crate + CLI(`ncx`) + Tauri GUI + **`ncx-mcp`**（MCP stdio 客户端，已接进 agent：McpTool + mcp.toml loader + 启动注册）
- 工具：read_file·apply_patch·shell·update_plan·grep·glob·web_search·web_fetch·tool_search·remember·skill
- **Skills（已并入 rust-capability）**：SKILL.md 发现 + 渐进披露注入 + `skill` 工具 + builtin（`commit-message`，include_str! 编入二进制，FS 同名可覆盖）+ `/skills` 命令。stream C vision 基础（`7de2235`）也随 FF 一起进了 rust-capability。
- 分层 flash/pro 编排器（`-o`，verifier 选 BEST worker + promote）；memory 自进化 + 启发式/LLM consolidate（`--memory-merge`）；keyed 搜索(Tavily/DDG)
- 已并入并行会话 18 commit：session 持久化/resume、checkpoints、hooks、project_instructions、富 slash、compact、token usage、release 脚本

## 并行拆分（多会话同时做）——接手按此认领
**硬约束**：① 每会话**独立 git worktree**（别共用工作目录）：`git worktree add ../ncx-A -b feat/mcp rust-capability`；
② 从已推的 `rust-capability` 分叉；③ push 前 `cargo test` 全绿；④ 频繁 `git pull --rebase`；⑤ 一个会话当 integrator 合并。

| 流 | 任务 | 拥有/新建文件（低冲突） | 依赖 |
|---|---|---|---|
| **A 分支 feat/mcp** ✅完成(`dc56233`，已并入) | ncx-mcp crate(stdio JSON-RPC client) + McpTool(`Rc<Mutex<McpClient>>`，非只读走审批) + `~/.nanocodex/mcp.toml` loader + main.rs 启动注册。mock server live 测过。⚠️ 之前这些文件未入库导致 HEAD 干净 checkout 编不过，已修复 | `ncx-core/src/mcp_tool.rs`、`crates/ncx-mcp/`、`ncx-config` servers 字段 | 无 |
| **B 分支 feat/skills** ✅完成(`b70907b`) | SKILL.md 发现 + 渐进披露注入 + `skill` 工具(已 live 验) | `ncx-core/src/skills.rs`(新)；tools/lib/cli/runner/gui 各加几行 | 无 |
| **C 分支 feat/vision** ✅完成(已并入) | VL 视觉分流：`with_vision_provider` + `has_image_block` 路由；CLI `--image`(可重复)/REPL 内联 `--image`；base64 多模态 content；`vl_base_url/vl_api_key/vl_model` 配置；含测试 | `agent_loop`、`cli/main.rs`、`ncx-config` vl 字段 | 无 |
| **D 分支 feat/orch** ✅完成(`3207b43`+`3090436`+`23c993a`) | high 任务递归分解：plan→decompose→每子任务 recurse(顺序、各自 promote)→main verify；atomic/depth 耗尽回退 best-of-N(`high_workers`=3)。旋钮 `high_workers`/`max_depth`(0=关)/`max_subtasks`(默认6，防过度拆分)。reasoning 节点(classify/plan/decompose/verify)**无工具**(`reason()`，否则强模型边分类边执行)。`parse_subtasks` 容错(SUBTASK:→编号/项目符号回退，live 模型常不守格式)。`LocalBoxFuture` 保 ?Send。13 测试。`NCX_TRACE` 有 `[orch]` 行。**live 验证**：classify High→decompose→recurse 已触发；但分类器保守(小任务判 Medium)+全 pro 慢，整条 High 递归未跑到完成 | **独占 `ncx-core/src/orchestrator.rs`** | 无 |
| **E 分支 feat/bench** ✅完成(`b175a74`+`96730f0`) | bench：`--repeats`(默认3)通过率 + md/json 报告 + `--tasks` 过滤 + Claude 臂。任务 t1–t13：**新增 5 个难任务** t9_expr_eval(递归下降+优先级)/t10_intervals/t11_wildcard(DP)/t12_toposort(环检测)/t13_jsonpath(嵌套+falsy 边界)，grader 均经参考解验证 well-formed + live 5/5 | **整个 `bench/`（纯 Python，零 Rust 冲突）** | 无 |

**冲突热点（只有这几处，纪律）**：`tools.rs`(register 行)、`lib.rs`(mod/export)、`Cargo.toml`(deps)、`cli/main.rs`(接线)。
**约定**：每条流对这些共享文件只加 **1–2 行**、加在末尾/固定锚点 → 合并是 trivial。
**建议并行度**：A/B/E 最独立（新文件为主），先开这三条；C/D 第二批。
之后 ROI 顺序若还要扩：③ skill(=B) → ④ image(=C) → ⑤ orch(=D)。鲁棒性不单独做，靠以上 + 真实使用磨。

## 基准（bench/，自动评分）
`python bench/run.py --agent <nanocodex|nanocodex-orch|opencode|claude|all>`。同模型 deepseek-chat：nanocodex 4/4、opencode 3/4
（**N=4 单跑、在噪声内，不能断言优势**）。Claude 臂 `claude -p` 报 401，需 `ANTHROPIC_API_KEY`。

## 流 A 完成情况（feat/mcp）
- `ncx-config`：`McpServerConfig` 结构体 + `load_mcp_servers()`/`load_mcp_servers_at()` 解析 `~/.nanocodex/mcp.toml`
- `ncx-core/src/mcp_tool.rs`：`McpTool`（`Rc<tokio::sync::Mutex<McpClient>>` + 审批）+ `register_mcp_server()` 启动帮助函数
- `ncx-cli/src/main.rs`：`ToolRegistry::new` 后自动加载并注册所有 MCP server 工具
- Live 验证：`everything` server 注册 13 个工具，模型成功 `tool_search` + `echo` 调用

## Do-Not（踩过的坑）
- tauri lib 用 `crate-type=["lib"]`（cdylib → gnu ld `export ordinal too large`）；GUI crate 须自列 `async-trait`。
- svelte-plugin `^5` 配 vite `^6`。工具描述**逐字照搬**（含示例），否则模型发 git-diff 死循环；调试 `NCX_TRACE=1`，别用 `| head`（SIGPIPE 打断进程，重定向到文件）。
- opencode：`npm i -g opencode-ai` 后若 "postinstall not run"，手动 `cd node_modules/opencode-ai && node postinstall.mjs`；bin 在 `~/AppData/Roaming/npm/node_modules/opencode-ai/bin/opencode.exe`；DeepSeek 配 `~/.config/opencode/opencode.json`。
- 预期校准：这些抬完成率/触达面，**不抬硬推理天花板**（封顶在 deepseek-v4-pro < Fable）。真正上限杠杆=main 换强模型（`DeepSeekProvider` 已 OpenAI 兼容，改 base_url/key/model 零代码）。
- 残留：`git stash list` 的 `stash@{0}`=会话前 Python 时代 README/config.example 旧改动（已被远程取代，可丢）。
- MCP on Windows：`Command::new("npx")` 找不到 `.cmd` 脚本；`mcp.toml` 里用 `command="cmd"` + `args=["/c","npx",...]` 才能启动。
- 编排器 live 坑：`run_in` 给**所有**节点挂全部工具时，强模型在 classify 回合就 apply_patch 把活干了（classify 永不快速返回）→ 已用 `reason()` 无工具修。子任务隐患：分类器保守 + 无 fast_model 时全 pro，high 递归子任务多→跑不完；用 `max_subtasks` 限。要确定性验 high 递归，需 fast_model 或一个 `-o` 强制 complexity 的开关（尚无）。

## 记忆指针（auto-memory）
rust-rewrite-setup · rust-rewrite-rationale · rust-apply-patch-tool-desc · rust-tauri-gui-gotchas · rust-orchestrator-capability
