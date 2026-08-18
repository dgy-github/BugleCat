# HANDOFF — nanocodex (Rust 线)

> 新接手的 agent：先读完再动手。与上一级 `D:\agent_prac\HANDOFF.md`（面试准备）是两条独立线。
> Python 时代历史在 git 历史 + SESSION_MEMORY.md。

## 当前进度（2026-08-18）

### 当前工作线

- 工作树：`D:\github_dgy\nanocodex\.worktrees\model-provider-catalog`
- 分支：`feat/model-provider-catalog`
- 最新提交：`19e296b fix(agent): preserve long task deliverables`
- 本轮用户原有文件必须保留：`rust/Cargo.lock`（已修改）、`parse_xlsx.py`（未跟踪）。两者均未纳入本轮提交，也不得覆盖或清理。
- 主工作树 `D:\github_dgy\nanocodex` 另有用户未提交的 GUI 修改；继续开发应留在上述独立工作树，避免混入主工作树改动。

### 本轮问题与根因

- 相同标题的连续会话实际落在不同 session；程序启动时总建新会话，导致新会话无法继承旧会话的 PDF 任务状态。
- 长链路上下文裁剪只保留近期消息，较早的用户目标会被工具日志挤掉，模型因此持续研究或回答已有 PDF，而不是完成用户要求的新 PDF。
- Agent 以前没有交付物完成闸门：用户明确要求生成 PDF 时，即使没有创建或更新 PDF，也能用普通文本结束任务。
- 强制收敛逻辑可能过早移除工具，导致尚未完成的计划或 PDF 任务无法继续执行。

### 已完成修复

- `ncx-core/src/session_index.rs`：按规范化工作目录查找最近一个未归档、且存在快照的可恢复会话。
- `gui/src-tauri/src/bridge.rs`：启动时恢复当前工作目录最近会话；监听器就绪后发送 `Loaded` 事件，把恢复的历史同步到前端。
- `ncx-core/src/session.rs`：裁剪长上下文时，额外保留最多 8 条历史用户消息作为“任务历史锚点”，丢弃旧的助手和工具噪声。
- 新增 `ncx-core/src/agent_loop/deliverable.rs`：识别明确的 PDF 创建请求，记录执行前 PDF 快照，并检查本轮是否创建或更新了有效 PDF。
- PDF 有效性至少检查 `%PDF-` 文件头和尾部 `%%EOF`；只写文件头、沿用旧 PDF 或只回复路径均不能算完成。
- `ncx-core/src/agent_loop/turn.rs`：PDF 未交付时禁止文本提前结束，并明确要求模型停止继续研究、实际生成文件；未完成计划或交付物时，长链路收敛不能撤掉工具。
- PDF 只读、查找或询问类请求不会误触发“必须生成 PDF”的闸门。

### 验证证据

- `cargo test -p ncx-core`：**191 通过，0 失败**。
- `cargo test -p ncx-gui --manifest-path gui/src-tauri/Cargo.toml`：**25 通过，0 失败**。
- `npm run build`：成功，Vite 共处理 114 个模块。
- Windows GNU 正式版构建成功：
  - 程序：`rust\gui\src-tauri\target\x86_64-pc-windows-gnu\release\ncx-gui.exe`
  - 安装包：`rust\gui\src-tauri\target\x86_64-pc-windows-gnu\release\bundle\nsis\nanocodex_0.1.0_x64-setup.exe`
- 最新正式版已在本地启动并验证窗口响应正常；启动时进程号为 `28924`（进程号仅是当次运行状态，不应作为后续判断依据）。
- `git diff --check` 无格式错误，仅有 Windows 换行提示。

### 后续接手建议

1. 用现有长会话继续追问，确认恢复的是同一个 session，且较早的用户目标仍进入模型上下文。
2. 实测一句明确的“根据以上资料生成一个 PDF”，确认只有新建或更新有效 PDF 后任务才结束。
3. 实测“PDF 去哪了”“查找已有 PDF”等只读请求，确认不会被创建闸门拦截。
4. 若继续修改，先确认 `git status --short`，不要提交或还原用户的 `rust/Cargo.lock` 与 `parse_xlsx.py`。

### 追加修复：模型连接中断恢复（2026-08-18）

- 复现现象：DeepSeek 流式请求在底层重试耗尽后返回 `RequestError: error sending request`，Agent 过去会把英文底层错误直接当最终回答并结束当前问题。
- 当前网络与 `api.deepseek.com:443` 已实测可达，接口返回 401（未携带鉴权的预期响应），因此截图属于短时发送链路失败，不是域名、端口或 Base URL 配置错误。
- `ncx-core/src/agent_loop/turn.rs` 现在对响应到达前的 `RequestError`/`TimeoutError` 做本轮级恢复：保留同一条用户问题并继续请求，不要求用户重复描述。
- 连续 3 次本轮级恢复仍失败时才结束，并显示中文可恢复提示；底层英文 `RequestError` 不再作为正常助手回答写入会话。
- 新增 2 个回归测试，覆盖“首个请求失败、随后成功”和“连续失败后中文结束且不无限重试”。
- 验证：`ncx-core` **193 通过**；GUI 后端 **25 通过**；Vite 正式构建成功（114 模块）；GNU 正式版和 NSIS 安装包重新生成成功。
- 最新正式版已启动并确认窗口响应正常；当次进程号为 `38580`。

### 全局技能：阿里百炼文生图（2026-08-18）

- 技能位置：`C:\Users\25376\.ncx\skills\aliyun-image-generation`，属于 nanocodex 全局技能，打开任意工作区都可发现。
- 技能名：`aliyun-image-generation`；触发场景包括画图、文生图、生成图片、海报、插画、产品图和视觉素材。
- 执行脚本：`scripts/generate_image.py`，纯 Python 标准库实现；默认模型 `qwen-image-3.0`，高质量可选 `qwen-image-3.0-pro`。
- 密钥从 `C:\Users\25376\Desktop\qw_key.txt` 自动选取第二个 `sk-ws-...` 工作空间密钥；第一行 `sk-sp-...` Token Plan 密钥不用于文生图，完整密钥不会进入日志、仓库或命令参数。
- 第二个密钥已确认能枚举中国区百炼 239 个模型，包含 `qwen-image-3.0`、`qwen-image-3.0-pro`、`wan2.7-image`、`z-image-turbo` 等图片模型。
- 接口约束：该密钥拒绝异步任务调用，且百炼兼容接口没有 `/images/generations`；脚本使用已验证成功的中国区百炼同步原生多模态生成接口。
- 验证：4 项离线测试通过；真实执行脚本生成 879264 字节 PNG，文件头和实际渲染均正常。测试图：`artifacts\aliyun-skill-live-test.png`。
- nanocodex 已在技能创建后重启，新进程号为 `38516`，窗口响应正常。

### 追加修复：任务化会话标题（2026-08-18）

- 根因：会话索引直接截取第一条用户消息，长背景会变成冗长标题；模型生成标题后 GUI 顶部也没有从当前会话索引同步。
- `ncx-core/src/agent_loop.rs` 新增独立标题概括：不携带会话历史或工具，只让当前模型输出约 6–18 字的动宾任务标题；失败不影响正文任务。
- `ncx-core/src/session_index.rs` 将标题上限收紧到 36 字，支持持久化模型标题并在后续轮次保持；模型失败时使用本地任务片段作为后备。
- GUI 仅在首轮任务正常完成后生成标题；取消、错误和后续轮次不会重复触发。后端发送 `session_title` 事件，前端同步当前顶部标题并刷新侧栏。
- 已将两个历史异常长标题（`18cccdb67fce9ddcb3a01`、`18ccc576322ce5047e281`）迁移为“整理大模型架构资料 PDF”，并确认会话索引仍可完整解析。
- 验证：`ncx-core` **196 通过**；GUI 后端 **27 通过**；Vite 正式构建成功（114 模块）；Windows GNU 正式版与 NSIS 安装包生成成功。
- OpenAI 官方文档只公开 Codex/ChatGPT 桌面端用于在项目和长期任务间切换，没有公开会话标题生成算法；本实现对齐其用户可见的短任务标题体验，不声称复刻内部实现。

### 追加修复：流式响应解码错误恢复（2026-08-18）

- 复现：模型流式响应中途损坏时，provider 返回 `StreamError: error decoding response body`；旧恢复逻辑只识别 `RequestError` 与 `TimeoutError`，因此英文底层错误直接显示为最终回复。
- `ncx-core/src/agent_loop/turn.rs` 已将 `StreamError` 纳入同一轮有限恢复：保留当前用户请求并自动重试，最多 3 次；仍失败时只显示中文可恢复提示。
- 新增回归测试验证流解码错误后第二次请求成功，且 `StreamError` 不会写入会话。
- 验证：`ncx-core` **197 通过，0 失败**。

### 追加调整：完成后隐藏工具过程（2026-08-18）

- 用户最终确认的展示规则：任务执行中允许实时显示工具名称、参数和输出；任务结束后不保留任何工具过程记录，历史会话也不回放工具过程。
- `gui/src/App.svelte` 新增完成态清理：收到 `done`、`error` 或 `loaded` 后过滤全部 `tool_group`，只保留用户消息、最终助手结果和必要错误提示；磁盘会话日志不删除，仍可用于审计和调试。
- GUI system prompt 要求最终回复只包含执行结果和简短的下一步建议，不复述工具调用、日志或中间过程。
- 验证：GUI 后端 **27 通过**；Vite 正式构建成功（114 模块）。

### 追加修复：累计用量与费用跨重启保留（2026-08-18）

- 根因：顶部累计费用依赖前端内存中的 `tokIn/tokOut`，重启后归零；旧版本未将 usage 写入会话日志，因此已丢失的历史数无法准确反推。
- 现在每轮完成后按 `session_id` 持久保存输入/输出 token；应用启动、恢复历史会话时自动加载，新建会话和切换工作区时正确清零，避免串会话。
- 费用仍按当前配置的每百万 Token 输入/输出单价和币种计算；从本版本开始的数据可跨重启保留。
- 验证：GUI 后端 **28 通过**；Vite 正式构建成功（114 模块）。

## 元信息
- 最后更新：2026-08-18（顶部“当前进度”为现行状态；下方 2026-06-29 内容保留作历史背景）
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

## 2026-08-18 会话切换与历史轻量化
- 后端恢复历史时只向 GUI 投影每轮用户消息与最后一条非空助手回答；工具名、参数、结果和中间播报不再跨后端/UI 边界。
- Resume/Fork 快照从原先同一路径读取两次改为一次读取，长会话切换减少重复 JSON 解析。
- 流式文本、工具、审批、提问、完成、恢复和错误事件全部携带 `session_id`；前端只接收当前会话事件，切换后旧任务不能污染新界面与累计用量。
- 新会话在进入命令队列前分配 ID，并以空消息种子创建；继续共享当前项目目录、规则、skills 和文件，但不会继承旧聊天与未完成计划。
- 保存设置或应用模型预设改为保留当前会话 ID 重建，不再暗中创建一个前端不知道的新会话；只有切换项目/显式新建才创建空会话。
- 首轮标题生成改为独立 `ncx-title` 线程，不再阻塞串行 agent 命令队列。
- 验证：`ncx-core` 197 项、GUI Rust 32 项测试通过，Vite 正式前端构建通过；正式 Tauri 构建在本轮交付前继续执行。

## 2026-08-18 多会话并发执行
- 修正上一版“切换时停止旧任务”的错误语义：导航/配置协调器不再直接等待 `run_turn`，每个 Prompt 按 `session_id` 分派到独立 `ncx-turn-<session>` OS 线程；切走后原会话继续执行。
- Prompt 前后端契约增加 `session_id`；不同会话可以并行，同一会话仍禁止重入，后续消息保持该会话内串行。
- 运行态、取消标记、审批/提问归属、always-allow grants 全部按 session 隔离；停止 A 不会取消、拒绝或清空 B。
- 每个 session 改写入 `.nanocodex/sessions/<session_id>.jsonl`，避免并发追加同一个审计日志；快照仍由 SessionIndex 分会话持久化。
- 前端为每个会话缓存正在执行时的可见消息、待发送队列、审批和问题；切回运行中的会话不会丢用户消息，后台完成的 token/费用会记到对应会话。
- 最近会话侧栏增加“执行中”状态；新建、继续、分叉或切换会话不再调用 `stop_generation`。
- 验证：GUI Rust 37 项测试与 Vite 生产构建通过；包含并发占用、同会话防重入、目标取消隔离、独立日志和前后端路由契约。
- 真实 WebView/CDP 集成：A、B 两个会话分别执行 12 秒 shell 任务，侧栏同时观测 `RUNNING_COUNT=2`；两者运行中成功切回 A（仍显示停止按钮），最终 `BOTH_FINISHED`。两个结果分别落到 `18cccdb67fce9ddcb3a01`、`18ccf0037389edf069980` 的独立 snapshot 和 JSONL，均含各自 `SESSION_A_DONE` / `SESSION_B_DONE`。
