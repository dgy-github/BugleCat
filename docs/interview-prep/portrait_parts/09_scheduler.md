## 定时器设计 · 离线调度

> **诚实边界**：nanocodex 当前**没有**定时器（已核实 grep 全代码库无调度实现）。这一章是"如果要给单文件 CLI agent 加调度，我会怎么设计"的实现方案文档，同时是面试深答——讲述时先亮出"这是设计不是既成事实"再展开。

### 一句话主线
定时器的难点**不是计时**（那是 OS 的事），是**"没人在场时 `Decision::Ask` 怎么办"**——答案是把调度器当成一个新的"上层"，给出 fail-closed 的无人值守解析器（`Ask→Deny`、`AutoDeny` 不软化、只有 `AutoApprove` 放行），复用现成的双层门（`SandboxPolicy.can_write` ⊥ `Approver.classify`），`classify()` 一行不改，不发明任何新权限。

### 30 秒 / 2 分钟 / 深挖 三档

**[30 秒]** 不写调度守护进程——agent 冷启动 ~5.5ms，直接把无头单发命令（`ncx -w ws -s sb -a policy "prompt"`，既有单发路径已存在）委托给 OS 调度器（Windows Task Scheduler / cron / launchd），每次触发新起一个一次性进程，单文件还是单文件。真正的难点是"无人值守时 approval 的 Ask 怎么办"：让调度器这个新上层给出 fail-closed 解析器——`Ask` 一律 `Deny`、`AutoDeny` 不软化、只有 `AutoApprove` 放行。定时任务因此天然只能干"能自动批"的活；要跑写操作，必须在 `schedules.toml` 里显式预授权一个不产生 `Ask` 的策略组合，把授权决定前移到写配置那一刻。

**[2 分钟]** 触发机制三选一：A. 委托 OS 调度器（推荐，零 daemon、随开机存活、错过的触发 OS 能补跑）；B. 自带常驻 daemon（违背"单发快启动"气质、等于重造 cron，不推荐）；C. 复用 Temporal Schedules（视频子 crate 已依赖，只适合长时/贵/须恢复的作业，轻任务上是杀鸡用牛刀）。数据模型 `~/.nanocodex/schedules.toml`（沿用 `config.toml`/`mcp.toml` 约定）：`id`/`cron`或`at`/`tz`/`workspace`/`prompt`/`sandbox`/`approval`(默认最保守 read-only+on-request)/`max_iterations`/`max_tool_calls`/`on_missed`/`overlap`(默认 skip)。CLI 不引 clap，照搬既有"做一件事即退出"范式加管理 flag：`--schedule-sync`/`--schedule-list`/`--schedule-run <id>`/`--schedule-remove`；OS 实际调用的仍是普通 `ncx` 单发命令，无需新执行子命令。

**[深挖]** 重叠触发用 per-id 轻量锁文件（`~/.nanocodex/locks/<id>.lock`，含 pid+起始时间），拿不到锁则 `overlap=skip` 直接跳过并记一条 "skipped: still running"——这是排产订单级写锁的同构（"有在跑就别重入"），锁要带 TTL/陈旧 pid 检测防崩溃锁死。执行路径完全复用既有单发路径（可带 `-r` 接续会话或 `-o` 走编排），上下文压缩、双预算全部复用；无人看 stdout，结果落既有 session log + 一份 run-history（exit code/末条回复/触发时间），这份 run-history 是排产 `audit_log` 的同构物。分阶段实现：MVP（新 crate `ncx-schedule`：toml 模型+无人值守解析器+锁+只做一个后端(Windows Task Scheduler)+管理 flag+run-history 落盘）→ 加固（cron/launchd 后端+`on_missed`补跑+结果通知+预授权写任务显式校验+解析器 `Ask→Deny` 的属性测试）。

### 核心机制 · 6 根支柱

1. **无人值守解析器（新增的唯一权限逻辑）** — `Decision::Ask → Deny`（fail-closed）、`AutoDeny` 不软化、仅 `AutoApprove` 放行。— 这是"能力剥夺"范式的延伸：read-only 让 `can_write` 恒 false，unattended 让 `Ask` 恒 Deny；安全建在代码不建在措辞。
2. **委托 OS 调度器（推荐方案，非自建 daemon）** — 注册 `ncx -w … "prompt"` 进 Win Task Scheduler/cron/launchd，零 daemon、随开机存活、错过补跑。— agent ~5.5ms 冷启动让"每次触发新起进程"几乎免费，本就是单发 CLI 快启动的红利。
3. **无头单发执行路径复用** — 触发即拼一条既有 `ncx [OPTIONS] [PROMPT]` 命令，可带 `-r`/`-o`；执行路径、上下文压缩、双预算全部复用。— 不造新执行入口，定时器只是既有单发路径的一个新调用方。
4. **schedules.toml 声明式数据模型** — 沿用 `config.toml`/`mcp.toml` 约定，纯数据零 I/O 校验（cron/policy 合法性在 sync 时一次性查）。— 和既有配置层同一套解析习惯，不新造格式。
5. **CLI 管理 flag，不引 clap** — `--schedule-sync/list/run/remove`，触发时 OS 调的仍是普通单发命令。— 守住手写 parser 的启动快/二进制小气质，不把项目推向 clap。
6. **per-id 锁文件防重入** — `<id>.lock` 含 pid+起始时间，`overlap=skip` 默认跳过并记录。— 排产订单级写锁的同构，"有在跑就别重入"，锁需 TTL 防崩溃锁死。

### 关键数字 / 必背细节
- agent 冷启动 **~5.5ms**（推断/记忆，未逐条复验，是"每次触发新起进程免费"的前提数字）
- 复用既有 4 档 approval policy（`untrusted`/`on-failure`/`on-request`默认/`never`）+ 3 态 `Decision`（`AutoApprove`/`Ask`/`AutoDeny`）——定时器**不新增**这套枚举，只加一层"Ask→Deny"的解析
- `schedules.toml` 默认 `sandbox`/`approval` = 最保守组合 `read-only` + `on-request`
- `overlap` 默认 `skip`；`on_missed` 取值 `skip`/`run-once`
- 触发到执行流水 **6 步**（①schedules.toml → ②OS 调度器 → ③无头单发 → ④现成双层门 → ⑤无人值守解析器 → ⑥落 session log + run-history）
- MVP 阶段先只做 **1 个后端**（Windows Task Scheduler）

### 取舍与坑
- **三个触发方案里明确排除自带 daemon**：`ncx schedule daemon` 违背"单发快启动"气质、进程得一直活着（谁拉起/重启的问题）、等于重造 cron——即使它"跨平台统一"这个优点也不够抵消代价。
- **轻任务不用 Temporal**：视频子 crate 已依赖 Temporal（自带 durable/崩溃恢复/exactly-once Schedules），但只用在长时/贵/须恢复的作业上；"每晚跑一次测试"这种轻任务上 Temporal 是杀鸡用牛刀（要跑 Temporal server）。
- **定时任务默认干不了需要人批的写**：这是 fail-closed 的直接代价——想让它跑写操作，必须在配置里显式预授权一个不产生 Ask 的策略组合（如 `approval=never` + `sandbox=workspace-write`），把决定前移到人写配置那一刻，而不是运行时兜底。
- **不做运行时人审兜底**：无人值守本就没人审；不能指望运行时突然冒出一个人来点 Ask，只能靠配置期预授权。
- **不把定时器塞进沙箱 crate**：它是"上层"，approval.rs 的分层边界（policy 判物理/approval 判越界/prompt 留给上层）要守住，定时器只是调用 sandbox crate 的又一个"上层"，不该反向侵入。
- **MVP 不做分布式/多机**：单机 OS 调度已够，多机场景（共享锁）留到后面再谈。

### 高频追问与应答
- **Q：为什么不自带一个调度守护进程？**
  A：daemon 得一直活着、谁拉起谁重启，等于重造 cron，还违背单发快启动的气质；委托 OS 调度器还免费拿到"错过补跑"和"开机存活"，agent 本身 ~5.5ms 冷启动让每次触发新起进程几乎免费。

- **Q：重叠触发（上一次没跑完）、或者触发时机器关机了，怎么处理？**
  A：重叠触发靠 per-id 锁文件跳过重入（跟排产订单级写锁同构，锁带 TTL 防崩溃后永久锁死）；关机错过的触发靠 OS 调度器自带的 run-once-after-missed（对应 `on_missed=run-once`），不是 nanocodex 自己实现补跑逻辑。

- **Q：这算不算给 agent 引入了新的攻击面？**
  A：没有新权限逻辑——双层门（`can_write` ⊥ `classify`）原样复用，`classify()` 一行不改；新增的唯一决策是"无人时 Ask→Deny"，这是**收紧**不是放宽。能被配置/演化的最多是"跑哪条 prompt"，执行始终过沙箱。这和 ncx-forge"训练接缝即安全边界"是同一条信条的延伸。

- **Q：定时任务想执行写操作（比如自动 commit），怎么让它不卡在 Ask 上？**
  A：不是在运行时放宽 Ask 判断，而是在 `schedules.toml` 里为该 schedule 显式配置一个不会产生 Ask 的策略组合（例如 `approval=never` + 限定好的 `sandbox=workspace-write`）——越界仍 auto-deny，界内自动执行；授权决定被前移到人写配置的那一刻，运行时无人值守也不需要有人来点头。

- **Q：为什么不干脆一直跑一个后台进程等到点再触发？**
  A：因为那就是方案 B（自建 daemon），已经在设计里明确排除——它违背 nanocodex"单发 CLI、启动即走"的气质，而且需要额外解决"谁保证 daemon 本身一直存活"这个新问题，本质是重新发明 OS 调度器已经解决好的东西。

### 自测 · 主动回忆
1. [L1] 定时器真正的设计难点是什么？为什么说"不是计时"？
2. [L1] 触发机制三个方案分别是什么？推荐哪个，为什么？
3. [L2] 无人值守解析器具体怎么处理 `Decision::Ask`？这个设计属于 nanocodex 已有的哪种范式的延伸？
4. [L2] `schedules.toml` 的默认 sandbox/approval 组合是什么？为什么选最保守的？
5. [L3] 想让定时任务执行写操作，应该在运行时做什么，还是在配置期做什么？为什么？
6. [L3] 重叠触发和机器关机错过触发，两者的处理机制分别是什么？是不是同一套？
7. [L4] 为什么定时器不应该被塞进 sandbox crate？这体现了什么分层原则？

**答案要点**
1. 计时本身是 OS 该管的事；真正难点是"没人在场时，原本该由人来回答的 `Decision::Ask` 该怎么处理"——这是任何自主/后台 agent 的真正命门。
2. A 委托 OS 调度器（推荐，零 daemon、开机存活、错过补跑，且冷启动~5.5ms 让每次起新进程几乎免费）；B 自带 daemon（违背单发气质，不推荐）；C 复用 Temporal Schedules（只给视频那类长时/须恢复作业用，轻任务杀鸡用牛刀）。
3. `Ask → Deny`（fail-closed）、`AutoDeny` 不软化、只有 `AutoApprove` 放行；这是"能力剥夺"范式的延伸（read-only 让 can_write 恒 false，unattended 让 Ask 恒 Deny）。
4. 默认 `sandbox=read-only` + `approval=on-request`，最保守组合——定时任务无人值守，先假设它不该有能力做任何需要人确认的事，需要更大权限必须显式在配置里加。
5. 应该在**配置期**（`schedules.toml` 里预授权一个不产生 Ask 的策略组合），而不是运行时——因为运行时无人值守本就没人能回答 Ask，任何"运行时兜底"设计都是自相矛盾的。
6. 不是同一套：重叠触发靠 nanocodex 自己实现的 per-id 锁文件（带 TTL）；机器关机错过触发靠 OS 调度器自带的 run-once-after-missed 能力，nanocodex 不用自己重新实现这部分。
7. 因为 approval.rs 的分层边界是 policy（物理允许）/approval（越界怎么办）/prompt（人类交互，留给上层）三层解耦；定时器只是 sandbox crate 之上的又一个"上层"调用方，塞进 crate 内部会破坏这个既有分层。

### 别发散到这
- **Windows Task Scheduler / cron / launchd 各自的注册 API 细节**——只需知道"三个后端要写一层跨平台注册抽象"，不需要展开具体调用。
- **Temporal 的具体 Schedule 实现**——只需记住"视频子 crate 已依赖，长时/须恢复作业才用它，轻任务不用"，不要深入 Temporal 内部机制。
- **锁文件的具体格式/TTL 数值**——目前是设计方案层面的"带 TTL 防陈旧 pid"，没有给出具体数字，不要编造一个精确 TTL 秒数。
- **调度触发后 agent 内部的 run_turn/工具执行细节**——那是 Harness 子系统的地盘，这里只需说"复用既有单发路径"。
- **通知机制（写文件/webhook）的具体实现**——只在"加固阶段"提一句，不是 MVP 范围。

### 一句话收尾
记住开场那句主线——**"难点不是计时，是无人在场时 Ask 怎么办；答案是 fail-closed 到 Deny，复用现成双层门，不发明新权限"**——这是"能力剥夺"范式在"无人值守"场景下的自然延伸，讲述时先亮明"这是设计不是既成实现"，再顺着 6 支柱往下堆细节，不要横向漂到 sandbox 执行机制本身或 OS API 细节。
