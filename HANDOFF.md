# HANDOFF — nanocodex (Rust 线)

> 新接手的 agent：先读完再动手。与上一级 `D:\agent_prac\HANDOFF.md`（面试准备）是两条独立线。
> Python 时代历史在 git 历史 + SESSION_MEMORY.md。

## 元信息
- 最后更新：2026-06-26
- 分支：**`rust-capability`**（基于 rust-rewrite，已推 `origin`）。Python 树 `nanocodex/*.py` 不动。
- remote：`origin` → https://github.com/dgy-github/nanocodex.git（凭据已配）
- 路径：crates `rust/crates/`，GUI `rust/gui/`，基准 `bench/`。
- 工具链：无 MSVC，用 `x86_64-pc-windows-gnu`；每条 cargo 前 `export PATH="$HOME/.cargo/bin:$PATH"`。

## 当前状态（已完成，约 225 测试全绿）
- 6 核心 crate + CLI(`ncx`) + Tauri GUI + **`ncx-mcp`**（MCP stdio 客户端，已 live 验证，未接进 agent）
- 工具：read_file·apply_patch·shell·update_plan·grep·glob·web_search·web_fetch·tool_search·remember·skill
- **Skills（已并入 rust-capability）**：SKILL.md 发现 + 渐进披露注入 + `skill` 工具 + builtin（`commit-message`，include_str! 编入二进制，FS 同名可覆盖）+ `/skills` 命令。stream C vision 基础（`7de2235`）也随 FF 一起进了 rust-capability。
- 分层 flash/pro 编排器（`-o`，verifier 选 BEST worker + promote）；memory 自进化 + 启发式/LLM consolidate（`--memory-merge`）；keyed 搜索(Tavily/DDG)
- 已并入并行会话 18 commit：session 持久化/resume、checkpoints、hooks、project_instructions、富 slash、compact、token usage、release 脚本

## 并行拆分（多会话同时做）——接手按此认领
**硬约束**：① 每会话**独立 git worktree**（别共用工作目录）：`git worktree add ../ncx-A -b feat/mcp rust-capability`；
② 从已推的 `rust-capability` 分叉；③ push 前 `cargo test` 全绿；④ 频繁 `git pull --rebase`；⑤ 一个会话当 integrator 合并。

| 流 | 任务 | 拥有/新建文件（低冲突） | 依赖 |
|---|---|---|---|
| **A 分支 feat/mcp** | McpTool（包 `Rc<tokio::Mutex<McpClient>>`+ToolDef→实现 `Tool::execute`→call_tool，非只读走审批）+ `~/.nanocodex/mcp.toml` 配置 + 启动连服务/list_tools/注册 + 真实 server 验（`npx -y @modelcontextprotocol/server-everything`） | 新 `ncx-core/src/mcp_tool.rs`；`ncx-config` 加 servers 字段 | ncx-mcp(done) |
| **B 分支 feat/skills** ✅完成(`b70907b`) | SKILL.md 发现 + 渐进披露注入 + `skill` 工具(已 live 验) | `ncx-core/src/skills.rs`(新)；tools/lib/cli/runner/gui 各加几行 | 无 |
| **C 分支 feat/vision** | VL 视觉后端分流（image turn 路由到 vision provider） | `ncx-provider` vision 路径；`ncx-config` vl 字段（agent_loop 已有 image_url 检测） | 无 |
| **D 分支 feat/orch** | 编排器加深：动态 worker 数 / 更好 plan 拆分 / 递归子任务 | **独占 `ncx-core/src/orchestrator.rs`** | 无 |
| **E 分支 feat/bench** ✅完成(`b175a74`，已并入 rust-capability) | bench 入库：+4 难任务(t5_roman/t6_lru/t7_balanced/t8_wordfreq) + 每任务 `--repeats`(默认3)出通过率 + md/json 报告(`bench/reports/`已 gitignore) + `--tasks` 过滤；Claude 臂沿用已有接线。smoke 验：nanocodex×2 跑 t6/t7 全过 | **整个 `bench/`（纯 Python，零 Rust 冲突）** | 无 |

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

## 记忆指针（auto-memory）
rust-rewrite-setup · rust-rewrite-rationale · rust-apply-patch-tool-desc · rust-tauri-gui-gotchas · rust-orchestrator-capability
