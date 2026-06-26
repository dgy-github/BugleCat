# nanocodex 设计理念手册（中文）

> 这份手册讲的是 **“为什么这样设计”**，而不是 “怎么用”。用法看
> [README](../README.md) / [README.zh-CN](../README.zh-CN.md)；这里讲贯穿
> 全项目的优化理念、它们解决的真实问题，以及对应的代码位置和踩过的坑。
>
> 适用对象：想理解 nanocodex（Rust 线 `rust/`）为什么能用一对“便宜+强”的
> 模型挤出更高完成率的人。

---

## 0. 一条总原则：模型是天花板，框架抬下限

能力的瓶颈是**模型本身**，不是 agent 框架（plan / verify 跑在主模型上，整条
编排不可能超过主模型的推理上限）。所以 nanocodex 不试图“用框架变聪明”，而是：

- 把一对模型（`model` = 强/pro，`fast_model` = 便宜/flash）的**成本差**，
  花在**结构**上——多跑几次、互相检查、分而治之；
- 用这些结构去抬 **完成率 / 触达面 / 可靠性**（尤其是 simple+medium 任务），
  而不是去抬硬推理天花板。

> 校准：换更强的主模型才是抬上限的真正杠杆。`DeepSeekProvider` 是
> OpenAI 兼容的，改 `base_url`/`api_key`/`model` 即可换模型，零代码改动。

下面每一节都是这条原则的一个具体落点。

---

## 1. 分层编排：按风险投入算力

**理念**：不是每个任务都值得 plan + 多 worker + verify。先**分类**，再决定投入。

**机制**（`rust/crates/ncx-core/src/orchestrator.rs`，用 `-o` 启用）：

```
classify (fast，便宜的一次判断)
  ├─ Simple → 单次直跑（fast）
  ├─ Medium → plan(main) → workers×N(fast，并行) → verify(fast)
  └─ High   → plan(main) → decompose(main) → 递归子任务 / best-of-N → verify(main)
```

- `classify` 只花一次便宜调用，输出 simple/medium/high 一个词。
- 越难的任务，verify 越往主模型靠（High 在 main 上复核），worker 数也更多
  （`high_workers` 默认 3 > medium 的 `workers` 默认 2）——**动态投入**。

**坑/校准**：分类器很保守，小任务普遍判 Medium。这不是 bug，是模型判断；
框架对此的对策是“判错也不致命”——Medium 仍有 plan+verify 兜底。

---

## 2. best-of-N + 选优 + promote：用并行换可靠性

**理念**：单个 worker 不可靠，那就让 **N 个独立尝试**同跑，选最好的那个。

**机制**：

- Medium/High 的 worker 是**彼此独立的尝试**（不是分工），所以并行不会互相
  踩踏共享状态。
- 每个非主 worker 跑在**自己的工作区副本**里（`cli/runner.rs` 的 `copy_tree`
  到 scratch 目录），写操作互不冲突。
- verifier 在结果里点名 `BEST:<n>`，框架把**那个 worker 的工作区** promote
  回真实工作区（`promote_worker`），其余副本清理。
- verify 不过则把意见喂回去**闭环重试**，上限 `max_verify_retries`。

**为什么隔离+promote 很关键**：它让“多跑几次”从一个危险操作（并发写同一目录）
变成安全操作（各写各的，只采纳一个）。这也是递归（下一节）能 live 正确的前提。

---

## 3. 递归子任务分解：分治攻克 High 任务

**理念**：一个大任务，与其让一个 worker 一口气做完，不如**拆成有序子任务**，
逐个攻克、逐个落地。

**机制**（High 路径，受 `max_depth` 控制，默认 1）：

```
plan(main) → decompose(main) 产出 SUBTASK 列表
  ├─ ≥2 个子任务 → 顺序对每个递归 handle_at(depth+1)：
  │     每个子任务自己跑完整 pipeline 并 promote，再进入下一个
  │     → 最后 main 复核整体
  └─ 原子(<2) / 深度耗尽 → 回退 best-of-N
```

**为什么顺序递归在 live 下正确**：因为 `run_worker` 每次从**当前真实工作区**
复制 scratch，而每个子任务结束会 promote 回真实工作区。于是后一个子任务复制到
的是**前一个已落地**的状态——子任务之间天然串接，且不破坏第 2 节的隔离模型。

**防爆炸的三个旋钮**：

- `max_depth`：递归深度上限（0 = 完全关闭分解，High 退化为单次 best-of-N）。
- `max_subtasks`：单次分解的子任务数上限（默认 6），防止模型把任务过度拆成
  几十个小碎片，每个又是一条完整 pipeline。超额会截断并在 trace 里**明说丢了
  多少**（不静默截断）。
- 容错解析：真实模型常**不守 `SUBTASK:` 格式**（改用编号/项目符号列表），
  `parse_subtasks` 因此做了回退解析，否则会误判“原子”而丢掉一次有效分解。

**实测校准**：classify→decompose→recurse 这条路径已在 live 触发验证；但因
分类器保守 + 无 `fast_model` 时全程跑昂贵主模型，一条完整 High 递归很难在短时间
内跑到“最终 verify 通过”。这再次印证第 0 节：结构抬下限，速度/上限靠模型与配置。

---

## 4. 无工具的“推理节点”：让分类器分类、让 worker 干活

**理念**：不是所有节点都该拿到工具。**判断/规划**节点（classify / plan /
decompose / verify）应该只**思考并输出文字**，不该动工作区。

**真实踩坑（这条理念的由来）**：早期所有节点都挂了完整工具集。结果一个强模型
在 **classify 回合**里直接 `pwd → ls → update_plan → apply_patch` 把整个任务
做了——分类调用永远不快速返回，High 路径直接失控。

**机制**：trait 增加 `reason()`（`orchestrator.rs`），推理节点走它；
`cli/runner.rs` 把它实现为一个**无工具**的 AgentLoop（`ToolRegistry::empty`）。
没有工具 schema，模型就无法发起工具调用，只能直接给出分类/计划/裁决。执行节点
（Simple 直跑、worker）仍用带工具的 `run`/`run_worker`。

**副作用与权衡**：无工具后，工具训练过的模型有时会在 plan/decompose 里**幻觉出
工具调用文本**。对策是在这些节点的系统提示词里**明确声明“你没有工具、不要尝试
读文件”**，并配合第 3 节的容错解析。

---

## 5. 渐进披露：上下文是稀缺资源

**理念**：把“可能用到的全部内容”一次性塞进系统提示是浪费。只挂**索引**，
正文**按需加载**。

**两处同构的落点**：

- **Skills**（`ncx-core/src/skills.rs`）：系统提示里只注入每个技能的
  `name + description`（一级披露）；模型判断相关后调 `skill` 工具取回完整
  `SKILL.md` 正文 + 目录（二级），再用 `read_file` 取其中附带资源（三级）。
  这样一个很大的技能库也不吃上下文窗口。
- **tool_search**：小注册表把所有工具直接挂出；**大注册表**只挂核心工具 +
  一个 `tool_search`，命中结果在下一轮 schema 视图里才暴露。

**同一个形状**：始终可见的小目录 + 按需取回的重内容。这是把有限上下文留给
真正相关信息的通用手法。

---

## 6. 视觉分流：让每种 turn 走对的后端

**理念**：带图的 turn 不该硬塞给纯文本模型；它应该路由到**视觉后端**。

**机制**（`agent_loop.rs` + `cli/main.rs`）：

- `has_image_block` 检测本轮 user 输入是否含 `image_url` 块；
- 含图则 `active_provider()` 切到 `vision_provider`（由 `vl_base_url /
  vl_api_key / vl_model` 配置构造，缺省回退到主端点/主 key）；
- CLI `--image <path>`（可重复）与 REPL 内联 `--image` 把文件 base64 成
  `data:` URL，拼成 OpenAI 风格的多模态 `content` 数组；
- provider 请求层对 `content` 是**原样透传**的，多模态数组天然兼容，无需改
  请求构造。

**实测**：把 Windows 默认壁纸喂进去，路由到 `qwen3-vl-plus`，准确描述出
“蓝色渐变背景上的发光四格窗口 logo + 光束”；纯文本 turn 不触发该路由。

---

## 7. 自进化记忆：便宜常态 + 昂贵按需

**理念**：项目记忆要随使用变聪明，但**整理记忆的成本**不该压在每次启动上。

**机制**：

- 每次启动跑**便宜的启发式去重**（`consolidate`），保持记忆整洁、幂等；
- 真正昂贵的 **LLM 合并**（把一簇相关笔记折成一条）只在显式维护命令
  `ncx --memory-merge` 时跑；
- 检索用**混合词法+语义**排序：关键词、标签、短语命中、Jaccard 相似度、
  时近性，加一张 agent/runtime 术语的同义词小表。
- `remember` 工具让 agent 把核实过的事实追加进项目记忆——它会**在这个仓库上
  越用越懂**。

**分工**：记忆是“谁/是什么”（偏好、事实）；skills 是“怎么做 X”；
AGENTS.md / CLAUDE.md 是项目级指引。三者互补，每轮都注入。

---

## 8. 运行时控制面：别把可靠性外包给模型

**理念**：预算、上下文长度、质量门这些事，应该由**运行时**强制，而不是只靠
“在提示里求模型别超”。

**机制**（Rust 运行时边界）：

- **任务预算**：每次模型调用都带一条预算提示（剩余模型调用数 / 工具调用数 /
  上下文限额）；预算耗尽时循环**干净停下**，并回填未应答的工具调用以保持消息
  历史合法。
- **上下文编辑**：本地会话完整保留，但发给 provider 的是**发送时编辑过的视图**
  ——压缩旧工具结果、超预算后丢更老的前缀。
- **确定性 hooks**：`[[hooks]]` 在匹配工具前后或回合生命周期点跑项目命令；
  `pre_tool`/`user_prompt` 失败会**拦截**动作，`post_tool`/`stop` 输出用于
  审计/格式化/质量门。
- **检查点/恢复**：每次模型回合前存工作区文件检查点；CLI `/checkpoint`
  `/checkpoints` `/restore`，GUI 有对应面板。恢复前先存一份当前状态的安全点。

**共同点**：这些都把“可靠性”放在**类型化的运行时边界**上，而不是依赖模型侧的
约定。

---

## 9. 沙箱 + 审批：两条正交轴

**理念**：“物理上允许什么”和“越界时怎么办”是**两个独立问题**，分开建模。

- **沙箱模式**（能做什么）：`read-only` / `workspace-write` / `danger-full-access`。
- **审批策略**（越界怎么办）：`untrusted` / `on-failure` / `on-request` / `never`，
  审批引擎把每次越界解析为 `ASK` / `AUTO_APPROVE` / `AUTO_DENY`。

所有文件/shell/搜索/记忆操作都经由同一个 `ToolContext` / `ToolRegistry`，
让审批与沙箱检查**贴着真正发生动作的地方**。Windows 上是**策略级**强制（路径与
可写根门控），不是内核隔离。外部内容（文件、命令输出、web/MCP 结果）一律按
**不可信数据**处理，不当作指令。

---

## 10. 基准方法论：诚实地度量

**理念**：agent 是**非确定性**的，单跑的成败说明不了什么；度量必须诚实。

**机制**（`bench/`，纯 Python，零 Rust 耦合）：

- 每个 `(agent, task)` 跑 `--repeats` 次（默认 3），报告**通过率 k/N**，而不是
  单次成败；
- 每个任务一个目录：`prompt.txt`（给 agent）+ 可选种子文件 + **隐藏
  `check.py`**（事后评分，exit 0 = 过）；agent 跑在干净临时工作区里。
- 任务从单函数算法题（t1–t8）扩到更难的题（t9–t13：递归下降表达式求值、
  区间合并、`*`/`?` 通配 DP、带环检测的拓扑排序、嵌套结构点路径取值）。
- **grader 要先验过**：每个 `check.py` 都用一份**参考解**跑过、确认正确实现
  能过（坏 grader 比没题更糟）；新题也都 live 端到端验证（5/5）。
- 多臂对比：`nanocodex` / `nanocodex-orch` / `opencode` / `claude`。

**诚实纪律**：N 很小、在噪声内时，**不断言优势**。报告（md/json）落在
`bench/reports/`。

---

## 附录 A：可观测性（NCX_TRACE）

设 `NCX_TRACE=1` 打开 trace：

- 主循环：每轮 `iter=… finish=… n_tools=… ctx=…/…` + 每个工具调用与结果；
- 编排器：`[ncx-trace][orch]` 行——分类结果、分解出多少子任务、递归进入哪个
  子任务、是否触发子任务数截断、是否走视觉路由。

**坑**：别用 `| head` 截断 trace（SIGPIPE 会打断进程），重定向到文件再看。

## 附录 B：调参旋钮速查

| 旋钮 | 作用 | 默认 |
|---|---|---|
| `workers` | Medium 的 best-of-N worker 数 | 2 |
| `high_workers` | High（原子/深度耗尽）的 worker 数 | 3 |
| `max_verify_retries` | verify 失败后的闭环重试上限 | 1 |
| `max_depth` | High 递归分解深度上限（0=关） | 1 |
| `max_subtasks` | 单次分解子任务数上限（防过度拆分） | 6 |
| `fast_model` | 便宜模型；空则 fast 节点回退用主模型 | 空 |
| `vl_base_url/api_key/model` | 视觉后端；缺省回退主端点 | 空 |

---

## 一句话总结

> **把省下的钱花在结构上**：分类决定投入、并行换可靠、分治攻克大任务、
> 推理节点不碰工具、上下文按需披露、每种 turn 走对后端、记忆便宜常态昂贵按需、
> 可靠性放在运行时边界、度量保持诚实。框架抬下限；上限留给更强的模型。
