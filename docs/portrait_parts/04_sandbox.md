## 沙箱 · 审批状态机

### 一句话主线
这套系统不是一张有跳转的状态图，而是**两层正交的纯函数 + 一层进程容器**：`SandboxPolicy` 判定"物理上是否允许"（三档 sandbox 模式 + 词法路径包含），`Approver::classify` 判定"越界时怎么办"（四档 approval policy → 三态 `Decision`），人类提示（`ApprovalHandler`）作为第三层留在 `ncx-core`，而 `PolicyExecutor` 只做进程级容器（Job Object + timeout），**不碰任何审批逻辑**。

### 30 秒 / 2 分钟 / 深挖 三档

**30 秒（主干）**：审批"状态机"实际是一个纯无状态函数 `Approver::classify(command, needs_escalation) -> Decision`，由四档策略选择（untrusted / on-failure / on-request[默认] / never），输出三态 `AutoApprove | Ask | AutoDeny`。与它正交的是 `SandboxPolicy` 三档模式（read-only / workspace-write / danger-full-access），回答 `can_read`（恒 true）和 `can_write`（词法路径包含）。每次调用都重新计算，无任何跳转或持久状态。

**2 分钟（加机制名）**：谁来判？shell 由 `ShellTool::needs_escalation` 先按 policy 模式算出 escalation bit，再交给 `Approver::classify`；若结果是 `Ask`，才让 `ApprovalHandler` 弹窗（trait 在 `ncx-core/tools.rs`，不在 sandbox crate）。apply_patch 的判官是 `policy.can_write` 逐目标 + `require_edit_approval`。唯一的持久状态是 `SessionGrants`（`HashSet<String>` 精确命令 + `allow_edits` bool），session 级、`Rc<RefCell>`，new/fork/resume 即丢。`PolicyExecutor` 在决策做完之后才被调用，只提供 Windows Job Object 容器和 timeout。untrusted 档还有 `is_trusted` 白名单（29 条 TRUSTED_COMMANDS）+ 7 条 dangerous_patterns 正则 + git 写子命令覆盖（15 条 GIT_WRITE_SUBCMDS）。

**深挖（细节钩子）**：`can_write` 在 workspace-write 下把目标对 workspace 转绝对再 `normalize` 词法化（不碰 FS、不解符号链接），按 component-wise `starts_with` 判定是否在 `writable_dirs`（workspace + writable_roots + 系统 temp 仅当 `allow_temp_write`）。`is_trusted` 先跑 dangerous_patterns（任意位置命中即 false），再剥路径前缀和 `.exe` 取 first_token，base==git 时找首个非 flag token 查 GIT_WRITE_SUBCMDS。shell 决策顺序：resolve workdir（`abs.canonicalize().unwrap_or(abs)`，tools.rs:1197）→ needs_esc → `session_grants.commands.contains(command)` 精确命中强制 AutoApprove，否则 classify → AutoDeny 报 'never' 错 / Ask 走 approver / AutoApprove 直接执行 → 然后才 `PolicyExecutor::run` → on-failure 档失败后追加一次 escalated 提示并**重跑同一条命令**。apply_patch：plan_mode 拒绝 → parse → 逐目标 `canonicalize`（tools.rs:654）后过 can_write 收集 `escaping` → `needs_prompt` → ApprovalRequest 带 full patch 作 details → 最终 `can_write` 闭包是唯一强制点。`step_decision` 已写好且单测但**活流程里没被调用**（dead）。

### 核心机制 · 6 根支柱

1. **三档 sandbox 模式（policy.rs:19-21）** — `read-only` / `workspace-write` / `danger-full-access`；`can_read` 恒 true，`can_write` 在 danger 恒 true、read-only 恒 false、workspace-write 走词法 `starts_with`。— 把"物理是否允许"与"越界怎么办"彻底分开，照搬 Codex 三模式。

2. **四档 approval policy → 三态 Decision（approval.rs:161-188）** — never（escalation? AutoDeny : AutoApprove）/ on-request 默认（escalation? Ask : AutoApprove）/ on-failure（永远 AutoApprove，失败后才问）/ untrusted（is_trusted && !escalation ? AutoApprove : Ask）；未知策略串 → Ask。— 纯函数每次重算，把 yes/no 提示完全踢出 sandbox crate。

3. **untrusted 三重过滤（approval.rs:119-142）** — dangerous_patterns 先跑（任意位置命中即否，7 条正则）→ 剥路径/`.exe` 取 base token 查 29 条白名单 → base==git 再查 15 条写子命令。— 保守白名单：只让已知安全命令自动跑（`git status` 自动、`git push` 仍问），危险模式前置兜底。

4. **ApprovalHandler trait + ApprovalDecision + SessionGrants（tools.rs:44-78）** — `#[async_trait(?Send)]` 的提示抽象住在 ncx-core（避开与纯 `Approver` 重名），返回 `Deny|Once|Always`；"记住决定" = `SessionGrants{commands: HashSet<String>, allow_edits: bool}`，`Rc<RefCell>` 挂在 ToolContext。— 提示是 CLI/GUI 上层关切故用 trait；always-grant 仅 session 级、命令精确匹配，刻意窄。

5. **shell 执行路径与判官顺序（tools.rs:1184-1294）** — needs_escalation 先算（danger→false / read-only-class→`!looks_read_only` / workspace-write→`!can_write(workdir)`，workdir 已 canonicalize），session_grants 精确命中可短路 AutoApprove，再 classify，Ask 才弹窗，最后才 `PolicyExecutor::run`。— escalation 是只有 tool 知道的 policy 模式启发式，算完后把 auto/ask/deny 交给纯 Approver。

6. **PolicyExecutor 只做容器（executor.rs:83-195）** — 最小化 env + cmd.exe /C（或 bash -l -c）+ Windows Job Object（KILL_ON_JOB_CLOSE，+ ACTIVE_PROCESS_LIMIT）+ timeout；**executor.rs 从不 import / 引用 `ncx_sandbox`，不碰任何 approval 类型**（注：ncx-tools crate 的 Cargo.toml 仍把 ncx-sandbox 列为依赖，故 crate 链接了它，但执行器源码不触审批逻辑）。— 设计明确分工：执行器只管进程/资源容器，FS/网络隔离留在 policy+approval 层；决策到此已成定局。

### 关键数字 / 必背细节
- **4** approval policies：untrusted / on-failure / **on-request（默认）** / never（approval.rs:18-21）
- **3** Decision 态：AutoApprove / Ask / AutoDeny（approval.rs:24-32）
- **3** sandbox 模式：read-only / workspace-write / danger-full-access（policy.rs:19-21）
- **3** ApprovalDecision：Deny / Once / Always；`approved() == !Deny`（tools.rs:44-56）
- TRUSTED_COMMANDS = **29** 条；GIT_WRITE_SUBCMDS = **15** 条；dangerous_patterns = **7** 条正则；WRITE_TOOLS = **2**（shell, apply_patch）
- 默认 `approval_policy = on-request`；默认 `timeout_s = 120`，shell timeout 参数限 **1..600**（tools.rs:131, 1178）
- `PolicyExecutor.active_process_limit` 默认 **512**，0 关闭上限（executor.rs:84-94）
- timeout → `exit_code = 124, timed_out = true`（executor.rs:189-191）；spawn 失败 `exit_code = 1`
- `MAX_OUTPUT = 16000` 字符，head/tail 各 **8000**（executor.rs:20, 68-74）
- `CREATE_NO_WINDOW = 0x0800_0000`（executor.rs:135）；TerminateJobObject exit 1（executor.rs:326）
- `allow_temp_write` 默认 false；`network_access` 默认 false，policy 自己绝不强开（policy.rs:30-31, 60-63）
- 关键函数名：`Approver::classify` / `SandboxPolicy::can_write` / `is_trusted` / `looks_read_only`（detect.rs:53-73）/ `split_chain`（detect.rs:39-47，按 `&&` `||` `;` `|` `&` 换行六分隔符切段）/ `step_decision` / `normalize`（policy.rs:134-146）

### 取舍与坑
- **`step_decision`（approval.rs:54-62）是死代码**：已定义 + 单测（248-268），但 shell 和 apply_patch 活流程都没调它。逐步确认实际靠 `ApprovalDecision::Always` + `require_edit_approval` 实现。它是"per-step 覆盖层"的可复用原语，目前未接线。
- **on-failure 重跑的是同一条命令**（tools.rs:1288）：没有第二个"非沙箱"执行模式——PolicyExecutor 根本没有 FS/网络沙箱可放松，重跑只是用户 bless 了一次。
- **`sandbox_denied` / `denial_reason` 是 vestigial 字段**：ExecResult 带着、render() 也处理，但 executor.rs 从不 set true；真正的拒绝在上层以纯错误字符串抛出（tools.rs:1227-1232, 705-707），是为对齐 Python ExecResult 形状。
- **shell always-allow 是精确字符串匹配**（tools.rs:1219，HashSet）：`ls -la` 被授予不会 bless `ls -l`；尾随空格或参数重排即失效。
- **apply_patch 'Always' 仅在 escaping 为空时设 `allow_edits`**（tools.rs:710）：越界 patch 永远不能被一次性 blanket 授权，每次重问。
- **shell 的 needs_escalation 只查 `can_write(WORKDIR)`**（tools.rs:1157），不看命令实际写哪些文件；workdir 在 workspace 内但写到别处的命令不会被这条启发式标记（read-only 模式改靠 `looks_read_only` token 扫描）。
- **"无 approver" 两条路失败模式不同**：shell + Ask 硬报错（tools.rs:1259-1262）；apply_patch + needs_prompt 静默落到 `can_write` 闭包去拒绝越界目标（tools.rs:715-718）。
- **两条写路径的真正差异是粒度/检查对象，不是符号链接解析**：shell 在调 `can_write` 前先 canonicalize workdir（tools.rs:1197），apply_patch 在 can_write 前先 canonicalize 每个目标（tools.rs:654）——**两侧都先 canonicalize 自己的输入再做词法 can_write，符号链接在两条路上都已被解析**。真正的不对称在于：shell 只查 `can_write(WORKDIR)`（工作目录是否可写），不看命令实际写到哪；apply_patch 逐个声明的写目标查 can_write。
- **danger-full-access 只清零 escalation bit，不改 approval policy**：untrusted + danger 下，未知命令仍会 Ask，因为 classify 的 is_trusted 独立于 escalation（approval.rs:179-185）。
- **未知/拼错的 approval policy 串 → Ask**（approval.rs:186），fail-safe；空/默认 ToolContext 用 on-request。

### 高频追问与应答
- **Q：这"状态机"到底有几个状态、怎么跳转？**
  A：严格说没有跳转。它是纯函数 `Approver::classify`，每次调用从 (policy, command, needs_escalation) 重新算出三态之一，不存任何中间态。全子系统唯一持久状态是 `SessionGrants`，且它在 ncx-core 不在 sandbox crate，session 级生命周期。

- **Q：danger-full-access 是不是就完全放行了？**
  A：不是。danger 只让 `can_write` 恒 true、`needs_escalation` 恒 false（清零 escalation bit），但**不改 approval policy**。若 policy=untrusted，未知命令照样 Ask，因为 classify 里 untrusted 分支检查 is_trusted 与 escalation 无关。

- **Q：untrusted 下 `rm -rf /tmp/x` 会怎样？为什么 `rm` 本身不在白名单也被拦？**
  A：`is_trusted` 第一步就跑 dangerous_patterns（`rm -[rf]` 等 7 条正则），任意位置命中直接返回 false，**先于** first_token 解析和白名单查询。所以即使前导是个 trusted token 也会被拦，classify 返回 Ask（不是 AutoApprove）。

- **Q：apply_patch 写到 workspace 外怎么处理？能"总是允许"吗？**
  A：每个目标（含 move_to）resolve 后过 `policy.can_write`，失败的进 `escaping`，触发 escalated 提示（reason 列出越界路径，details 是完整 patch）。即使用户选 Always，只要 escaping 非空就**不会**设 `allow_edits`——越界写每次都重新批。最终 `can_write` 闭包是唯一强制点，连没有 approver 时越界目标也直接失败。

- **Q：on-failure 重跑时是不是放松了沙箱？**
  A：没有。tools.rs:1288 重跑的是**同一条命令**走同一个 PolicyExecutor，没有第二个非沙箱模式。重跑唯一区别是用户 bless 过一次。这也暴露真正的 FS/网络沙箱并未实现，PolicyExecutor 只做进程容器。

- **Q：PolicyExecutor 怎么参与审批决策？**
  A：完全不参与。executor.rs 从不 import / 引用 `ncx_sandbox`、不碰任何 approval 类型（虽然 ncx-tools 的 Cargo.toml 把 ncx-sandbox 列为依赖，crate 链接了它）；`.run()` 被调用时 auto/ask/deny 已成定局。它只负责最小化 env、cmd.exe /C / bash -l -c 启动、Windows Job Object（KILL_ON_JOB_CLOSE）杀整个后代进程树、以及 timeout（超时 exit 124）。Job API 失败时降级为无容器运行而非报错（对齐 Python OSError fallback）。

### 自测 · 主动回忆
1. [L1] 四档 approval policy 分别是什么？默认哪个？
2. [L2] read-only 模式下 shell 命令是用 `can_write` 判 escalation 的吗？不是的话用什么？`split_chain` 按哪些分隔符切段？
3. [L2] `is_trusted` 的三个检查步骤及顺序是什么？为什么 dangerous_patterns 必须放第一步？
4. [L3] shell 写路径和 apply_patch 写路径在符号链接处理上真的不同吗？两者真正的不对称在哪？
5. [L3] "无 approver 配置" 时，shell 和 apply_patch 的失败方式有何不同？
6. [L4] 为什么 `ApprovalHandler` trait 住在 ncx-core 而不是 sandbox crate？这体现了什么分层设计？
7. [L4] `step_decision` 的设计意图是什么？它现在为什么是死代码？
8. [L3] danger-full-access + untrusted policy 组合下，未知命令会 AutoApprove 吗？为什么？

**答案要点**
1. untrusted / on-failure / on-request（默认）/ never；常量在 approval.rs:18-21。
2. 不是。read-only-class（`!writes_allowed`）走 `!looks_read_only(command)`（detect.rs:53-73）；只有 workspace-write 才用 `!can_write(workdir)`。`split_chain`（detect.rs:39-47）按 `&&`、`||`、`;`、`|`、`&`、换行**六个分隔符**切段，逐段比对 READ_ONLY_PREFIXES（且整串先过 WRITE_OR_SUBSHELL token 否决）。
3. ① dangerous_patterns 正则（任意位置命中即 false）② 剥路径前缀 + `.exe`、小写化取 first_token 查 29 条白名单 ③ base==git 时查 15 条写子命令。先跑正则是 fail-safe，保证 `rm -rf` 这类即使被 trusted token 引导或 rm 不在白名单也被拦。
4. 在符号链接处理上**两者其实一致**：shell 在 `can_write` 前先 canonicalize workdir（tools.rs:1197），apply_patch 在 can_write 前先 canonicalize 每个目标（tools.rs:654），符号链接两侧都已解析；`can_write` 本身两边都是纯词法（policy.rs:11-15, 134-146）。真正的不对称是检查对象/粒度：shell 只查 `can_write(WORKDIR)`（工作目录是否可写），不看命令实际写到哪；apply_patch 逐个声明的写目标查 can_write。
5. shell + Decision::Ask 且无 approver → 硬报错 'no approver configured'（tools.rs:1259-1262）；apply_patch + needs_prompt 且无 approver → 静默落到最终 `can_write` 闭包，越界目标被拒、in-workspace 目标可能放行（tools.rs:715-718）。
6. 因为提示是 CLI/GUI 上层关切，把 yes/no round-trip 留在 ncx-core 让 sandbox crate 保持 prompt-free（纯决策）；trait 命名也为避开与纯 `ncx_sandbox::Approver` 重名。体现 policy（物理允许）/ approval（越界怎么办）/ prompt（人类交互）三层解耦。
7. 意图：让用户关掉 auto-approve、对每个状态修改动作强制确认，叠加在沙箱-escalation 决策之上，且**永不软化 AutoDeny**。现为死代码：活流程改用 `ApprovalDecision::Always` + `require_edit_approval` 实现 per-step，故 step_decision 虽有单测但未接线。
8. 不会。danger 只清零 escalation bit，untrusted 分支判 `is_trusted(command) && !needs_escalation`；未知命令 is_trusted=false，结果仍 Ask。danger 不改 approval policy。

### 别发散到这
- **实际 patch 应用 / hunk 匹配算法**（patch.rs 的 staging、re-check）——属于 apply_patch 工具内部，只需知道它逐路径复查 `can_write` 即可。
- **Tauri GUI 的 modal 实现 / bridge.rs 的 GuiApprover**——属于 GUI 层，只需点名"Always/Once/Deny 的提示在这弹"。
- **detect.rs 内 `looks_read_only` 的完整 token 表**——属于命令检测子系统，只需知道它是与 `is_trusted` 独立的第二套"命令是否安全"启发式。
- **agent loop / orchestrator / tool dispatch**——上游怎么决定发出 tool call，不在本子系统。
- **Job Object 的 Win32 API 细节 / POSIX 进程组**——属于 executor 平台实现，知道"KILL_ON_JOB_CLOSE 杀后代树、失败降级"即可。

### 一句话收尾
记住一句话：**两层正交纯函数（policy 判物理、approver 判越界）+ 一层 session-scoped 的 SessionGrants + 一个不碰审批只做容器的 PolicyExecutor**——能把这三块的边界讲清楚，深挖时再用 classify 的四档分支、is_trusted 三步（29 条白名单 / 15 条 git 写子命令）、can_write 词法包含这些细节往下堆，就既不浅也不散。
