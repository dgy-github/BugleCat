# Windows 命令执行的 OS 级隔离

本文说明 `rust/crates/ncx-tools` 的命令执行器在 Windows 上由操作系统强制的限制：哪些已隔离、哪些没有，以及已知的兼容性问题。策略与审批层（`ncx-sandbox`）的词法检查保持不变，OS 层是叠加在它之上的第二道防线。

## 什么时候生效

| 调用路径 | 执行器 | 写入隔离 |
| --- | --- | --- |
| `shell` 工具（策略层判定无需提权） | `PolicyExecutor::for_policy(&ctx.policy)` | 是 |
| 后台进程工具 `spawn_managed`（无需提权） | 同上 | 是 |
| 需提权且已被用户批准 / 会话内放行的命令、on-failure 批准后的重试 | `PolicyExecutor::new()` | 否（用户已明确放行） |
| 用户配置的 hooks、内置 git 只读命令、交互式 PTY 终端 | `PolicyExecutor::new()` / `spawn_pty` | 否 |

按沙箱模式：

- `workspace-write`：工作区和 `writable_roots` 可写，其余位置不可写。
- `read-only`：只有每次执行独立的临时目录可写（但见下文"已知限制"）。
- `danger-full-access`：不做写入隔离。

非 Windows 平台目前没有 OS 级写入隔离，`write_isolation` 字段会被忽略。

## 机制

1. **Low IL 令牌**：先用 `CreateRestrictedToken(DISABLE_MAX_PRIVILEGE)` 去掉特权，再把完整性级别设为 Low（S-1-16-4096），然后 `CreateProcessAsUserW` 以挂起状态启动 `cmd.exe /D /S /C "chcp 65001>nul & <command>"`。进程先被放入 Job Object，再 `ResumeThread`，所以进程树里没有任何进程能在 Job 之外运行。stdio 走命名管道：父进程端由 tokio 异步读写，并通过 `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` 只让子进程继承这三个句柄。
2. **强制完整性标签**：给每个可写根目录加一条 Low 级 `SYSTEM_MANDATORY_LABEL_NO_WRITE_UP` 的 ACE（`OI|CI` 继承）。用户其他文件默认是 Medium，Low 进程写不进去，这由内核保证。
   - 幂等：先读现有标签，已经是 Low + `OI|CI` 就跳过；同一进程内再用缓存去重。首次给大目录打标签时会把继承标签传播到已有的全部文件（一次性开销，在 `spawn_blocking` 中执行）。
   - `.git/hooks`、`.git/config` 若在准备阶段已存在，会被显式标为 Medium，使其不能被原地改写（属纵深防御，见"已知限制"）。
3. **每次执行的临时目录**：`%TEMP%\ncx-sandbox\run-<pid>-<n>`。父目录 `ncx-sandbox` 带 Low 标签，每次执行时 `TEMP`/`TMP` 指向这个子目录，执行结束后删除。
4. **Job Object 限制**（所有经执行器启动的命令都会加，包括不做写入隔离的那些）：
   - kill-on-close，活动进程上限 512；
   - `JOB_OBJECT_LIMIT_JOB_MEMORY`：整个进程树合计的提交内存上限，默认 4 GiB，可用环境变量 `NCX_SANDBOX_JOB_MEMORY_MB` 调整（填 `0` 表示不限），也可以直接设置 `PolicyExecutor::job_memory_limit`；
   - UI 限制：禁止读/写剪贴板、切换/创建桌面、全局原子表、修改系统参数和显示设置、注销/关机。为了不影响普通控制台工具，没有开启 USER 句柄隔离（`UILIMIT_HANDLES`）。

## 明确降级，不会静默失败

下列情况下命令照常执行，但**不做写入隔离**。此时 `ExecResult::sandbox_note` 会被填写，渲染结果里会出现 `[sandbox] OS write isolation unavailable (<原因>); the command ran without it.`；后台进程则会在输出开头多一条 `[sandbox] ...`：

- 可写根位于网络盘（`DRIVE_REMOTE`）：远端服务器按网络登录身份做访问检查，本地的 Low IL 在那里不起作用。
- 文件系统不支持 ACL（`FILE_PERSISTENT_ACLS` 未置位，例如 FAT/exFAT）。
- 可写根是盘符根目录、用户目录或其祖先，或与 `SYSTEMROOT`、`Program Files`、`ProgramData` 有重叠：给这些位置打 Low 标签等于开放大片区域，因此直接拒绝。
- 读写标签、创建令牌、创建进程等 Win32 调用失败。

## 隔离了什么 / 没隔离什么

已由 OS 强制：

- 写工作区（及 writable roots、本次临时目录）以外、标签为 Medium 及以上的文件和目录，包括 `%USERPROFILE%` 下的文件、`~/.cargo`、npm 缓存、`%APPDATA%` 等。
- 进程树数量、合计内存、剪贴板/桌面等 UI 能力。
- 进程树生命周期：超时或结束时整树一起被杀。

**未隔离**：

- **网络**：Low IL 进程照样能联网。网络隔离需要 AppContainer，不在本次范围内。
- **读取**：Low IL 可以读用户能读的任何文件。敏感文件读取仍只靠策略层拦截。
- 本来就带 Low 标签的位置，例如 `%USERPROFILE%\AppData\LocalLow`、其他程序打过 Low 标签的目录。
- 非 Windows 平台。
- 用户已批准的提权命令、hooks、交互式 PTY 终端。

## 已知限制

- **标签是持久的**：工作区打上 Low 标签后会一直保留。之后即使以 `read-only` 模式运行，Low 进程依然能写这个工作区，因为它已带 Low 标签，所以 `read-only` 只对从未在 `workspace-write` 下用过的目录完全有效。如需撤销，可执行 `icacls <dir> /setintegritylevel (OI)(CI)medium /T`，或用 `icacls` 删除该标签。
- **`.git/hooks` / `.git/config` 的保护只是纵深防御**：`NO_WRITE_UP` 不包含 `DELETE`。由于 `.git` 本身带 Low 标签，Low 进程可以用改名的方式替换这些条目，例如 `git config` 先写 `config.lock` 再改名覆盖，实测仍然能成功。另外，在受限命令里新建的仓库（`git init`），要到下一次执行准备阶段才会被打标签。真正的边界仍是策略层的 `is_protected_write_path` 检查和审批流程。
- **执行器的精简环境变量**（`build_env`）不传 `ProgramFiles(x86)`、`ProgramData` 等变量，导致 rustc 找不到 MSVC 的 `link.exe`。这个问题在本次改动之前就存在，与 Low IL 无关：不做隔离运行时同样失败。下面的兼容性测试通过 `NCX_PROBE_PASS_ENV` 把这些变量传进去，以便单独观察 Low IL 本身的影响。
- 基于 msys 运行时的程序（例如 Git for Windows 的 `usr\bin` 下的工具；实测中它的 `link.exe` 因为 PATH 顺序被 rustc 误选为链接器），在 Low IL 下启动时会因为无法在 `\BaseNamedObjects` 创建命名对象而崩溃（`0xC0000142`，日志中为 `NtCreateDirectoryObject ... 0xC0000022`）。

## 兼容性实测（2026-10-21，Windows 11 Pro 26200）

测试工具：`cargo run -p ncx-tools --example sandbox_probe -- <workspace> <command>`。工作区为 `%TEMP%\ncx-compat\ws`，模式为 `workspace-write`；`NCX_PROBE_UNCONFINED=1` 用于跑不做隔离的对照组。

| 命令 | Low IL 结果 |
| --- | --- |
| `python -c "print(1)"` | 成功（首次执行包含打标签，共 0.9 s） |
| python 脚本：写工作区文件、用 `tempfile` 创建临时文件、写 `%USERPROFILE%` | 工作区写入成功；临时文件落在 `%TEMP%\ncx-sandbox\run-*`；写 `%USERPROFILE%` 被拒绝（`PermissionError [Errno 13]`） |
| `cargo build`（无依赖的小 crate） | 成功，与对照组一致，产物可运行 |
| `cargo build`（依赖已在本地 registry 解包的 crate） | 成功 |
| `cargo build`（依赖本地尚未缓存的 crate 版本） | **失败**：能下载，但写 `~/.cargo/registry/cache/...` 时报 `os error 5` |
| 同上，加 `set CARGO_HOME=%CD%\.cargo-home` | 成功（依赖下载到工作区内） |
| `npm --version` | 成功 |
| `npm install <pkg>` | **失败**：写 `%LOCALAPPDATA%\npm-cache\_cacache` 时报 `EPERM` |
| `npm install <pkg> --cache .npm-cache` | 成功 |
| `git init / add / commit / log / status` | 成功 |

### cargo / npm 缓存的处理建议

我们**不**自动给 `~/.cargo`、npm 缓存打 Low 标签：否则受限进程可以篡改 registry 里的源码或缓存包，这些代码之后会被用户在 Medium 级别编译或执行，相当于把隔离绕过去。可选做法：

1. 默认做法：保持隔离。需要下载新依赖时会失败，此时走现有的审批流程（on-failure 下批准重试、on-request 下批准提权），批准后的命令不做隔离。也可以事先在受限环境之外执行 `cargo fetch` / `npm ci`。
2. 让缓存落到工作区内：`CARGO_HOME=<ws>\.cargo-home`、`npm_config_cache=<ws>\.npm-cache`（或 `--cache`）。代价是每个工作区各下一份依赖。另外，`~/.cargo/config.toml` 里的镜像等配置只有在工作区位于用户目录之下时才会通过目录向上查找被读到，所以必要时需要把配置复制进去。这两个目录应加进 `.gitignore`。
