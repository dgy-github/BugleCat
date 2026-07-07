# nanocodex · 面试口播终版（叙事骨架 + L3 血肉）

> 和排产终版同构：【叙事开口】+【L3 血肉：函数名/常量/数字】+【被追问】。
> nanocodex 的 L3 素材已很全（portrait 8 子系统 / 6 张流程图 / 刷题卡 / 30 条模块追问），本终版补的是**口播叙事层**——把它讲成一个"从痛点自己抽象设计"的故事。
> **定位优势**：从零自研，full ownership，每个设计决策都是我的——被钻到 L3 不怕，因为不是接手的。

## 🔢 数字速记
- **~199× 启动加速**（5.5ms vs 1.1s，实测）、**2.4MB 单文件二进制**、**137 个测试**。
- codex 血统：apply_patch 工具描述**逐字移植**。
- 沙箱常量：**3** 沙箱模式 / **4** 审批档 / **29** TRUSTED 白名单 / **15** git 写子命令 / **7** dangerous 正则。
- 唯一接缝：`NCX_GENOME`；工具可见上限 **9**。

---

## ① 定位 + 起源（自研,是最强的开场）
**叙事**：nanocodex 是我把 **OpenAI Codex 用 Rust 重写**的编码 agent。起源是我自己用 Codex / Claude Code 这类 Coding Agent 时遇到的真实痛点——上下文管理、任务拆解、文件修改、执行反馈、代码理解、测试闭环——**先理解问题、再抽象系统、再自己设计实现**。所以它是从痛点长出来的工程底座，不是接手的。
**L3 血肉**：单进程 current-thread Tokio runtime，`?Send` + `Rc<RefCell>`（I/O 密集用单线程 `join_all` 而非 OS 线程做只读并发）；8 个子系统（Harness/上下文压缩/工具/沙箱/编排/记忆/Skills/ncx-forge）围绕 `AgentLoop::run_turn` 转。
**被追问"和 codex 什么关系"**：能原样搬的逐字搬（apply_patch/@path/tool_calls 形态/沙箱三模式/审批四档）；**但权限控制我做了再设计**（见 ④），这是我的工程判断。

## ② 为什么用 Rust（反直觉,能筛掉背概念的）
**叙事**：不是为了性能——agent 是 **I/O 密集**（等模型/等工具），CPU 不是瓶颈。选 Rust 是为了：**~199× 启动加速**（5.5ms vs Python 1.1s，命令行工具冷启动体感命门）、**单文件 2.4MB 二进制**（分发零依赖）、类型系统把 agent 循环的不变式在编译期钉死。
**被追问"那为什么不用 Go"**：要的是"编译期把 `?Send`/借用/状态机不变式钉死"+ 生态（async-trait/serde），Rust 的类型表达力更贴合把"能力袋"和循环状态建模。

## ③ Harness 主循环 run_turn（Workflow 环）
**叙事**：一个回合是 `for iteration in 0..max_model_calls` 的有界循环——组装上下文→调模型→有工具就执行→喂回→再循环，直到模型不带工具作答/error/cancel/触及预算。
**L3 血肉**：`max_model_calls = max_iterations.min(task_budget.max_model_calls.max(1))`；双预算（model_calls/tool_calls）；**历史合法性不变式**——cancel/预算耗尽时 `backfill_unanswered_tool_calls` 给悬空 tool_call 补占位 role:tool（否则下次请求 400）。
**被追问"怎么防死循环"**：双预算 + 无进展检测 + max_model_calls 硬上限，退出点五个（无工具/error/cancel/预算/max）。

## ④ 权限双门（承重, 相对 codex 的再设计, 最该主攻）
**叙事**：安全建在代码上不建在措辞上。一次写操作过**两道正交的门**：物理门判"能不能写"、越界门判"越界了怎么办"，再加一层人类提示。
**L3 血肉**：
- `SandboxPolicy.can_write`（3 档模式 + 词法路径包含，纯函数零 I/O）⊥ `Approver::classify`（4 档策略 → 3 态 AutoApprove/Ask/AutoDeny）。
- **untrusted 三筛顺序**：7 条危险正则先跑(rm -rf 即使被引导也拦) → 29 条白名单(first_token) → git 15 条写子命令。
- `SessionGrants` 精确字符串匹配(ls -la ≠ ls -l)，session 级即丢。
- `PolicyExecutor` 只做容器(Job Object + timeout)，**不碰审批**。
- **能力剥夺**：read-only 下 can_write 恒 false、plan mode 拒所有写——物理隔离不是 prompt 劝阻。
**被追问"danger-full-access 是全放吗"**：否，只清零 escalation bit 不改 approval policy；untrusted+danger 下未知命令仍 Ask。

## ⑤ 上下文压缩（Context 环，非破坏）
**叙事**：`for_model_edited` 构建**发送时视图**，`self.messages` 原始历史不动（非破坏），两趟：压旧 tool 结果 → user 边界丢最旧前缀。
**L3 血肉**：切点**绝不落 tool_result**（否则历史以孤立 tool 开头→400）；非破坏才敢激进压（resume/回放/换模型都用原始）。
**被追问"为什么非破坏"**：压缩是有损猜测，保留原始是唯一能兜底的地基。

## ⑥ 编排 best-of-N（Workflow 扩展）
**叙事**：高复杂度任务不靠更聪明的模型，靠**结构**——classify→plan→N 个 worker 并行→verify 选 winner→回灌重试。
**L3 血肉**：父子**无消息总线**，三通道：prompt 下发 + 文件隔离-提升(`copy_tree` 私有 scratch，winner 才 promote) + `PASS/FAIL/BEST` 裁决回灌。classify/plan/verify 走 `reason()` = 空 `ToolRegistry`（**代码级拿不到工具**）；用尽重试仍 promote best 标 `[unverified]`。
**被追问"子 agent 怎么通讯"**：不对话——文本是下行通道，磁盘才是代码的真 IPC，裁决文本是回路。

## ⑦ 记忆（Memory 环）
**叙事**：项目记忆是"线索不是事实"。
**L3 血肉**：`recall` 注入标 RECALL_HEADER("线索而非事实")；`remember` 仅 CONFIRMED + 精确去重；`consolidate(0.85)` 启动即幂等去重。

## ⑧ 离线演化 ncx-forge（Feedback 环, 承重, 相对 codex 独有）
**叙事**：让 agent 离线自动优化自己，但**只演化文字、不动行为**。
**L3 血肉**：genome = system_prompt + 工具描述（**纯文本**）；`NCX_GENOME` 是训练↔运行唯一接缝；**可证明 no-op**（空/错→字节级回退默认）；执行始终过沙箱→被不可信失败轨迹喂养的 teacher 也注入不了新能力（**训练接缝即安全边界**）；sentinel 门无法证明注入生效前拒绝训练。
**被追问"怎么保证离线优化不引入风险"**：能演化的只有文字，执行永过沙箱——离线训练改不动安全边界。

## ⑨ 驾驭工程(元) + 哲学
**叙事**：核心信念——**模型是第一杠杆，harness 是第二杠杆**。编排靠结构不靠更聪明的模型，ncx-forge 只演化文字，沙箱托底。
**L3 血肉**：四信条同排产——LLM 原始输出不直接落地 / 安全建在代码 / 有确定性判据别用 judge / 数据不出域。

---

## 🎯 与排产的定位（选主讲用）
- **nanocodex**：工程/系统复杂度高，**从零自研 full ownership** → 基建/框架/平台岗位主讲，被钻不怕。
- **排产**：业务/治理复杂度高，已上线但半途接手 → 企业 AI 落地/治理岗位主讲。
- 串讲主叙事：**同一套驾驭工程，nanocodex 长成"沙箱+best-of-N+离线演化"，排产长成"确定性 workflow+四段治理+评测飞轮"；骨架同，承重随业务。**

## 一句话总纲（背这句）
> nanocodex 是我从用 Codex/Claude Code 的真实痛点出发、用 Rust 从零重写的编码 agent 工程底座。选 Rust 不为性能而为 ~199× 启动加速+单文件分发+编译期钉死循环不变式。它相对 codex 最大的再设计是**把权限控制做成两层正交纯函数 + 代码级能力剥夺 + 训练接缝即安全边界**——能原样搬的逐字搬，该做工程判断的地方我自己重设计。贯穿哲学：模型是第一杠杆、harness 是第二杠杆，安全建在代码不建在措辞。
