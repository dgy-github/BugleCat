# HANDOFF — nanocodex

> 新接手的 agent：先读完此文件再动手。这是 nanocodex 子项目的交接，与上一级
> `D:\agent_prac\HANDOFF.md`（面试准备）是两条独立的线，互不覆盖。
> 工作已转到 **Rust 重写**；Python 时代的详细交接在 git 历史（此文件早期版本）+ SESSION_MEMORY.md。

## 元信息
- 最后更新：2026-06-25
- 分支：`rust-rewrite`（Python 树 `nanocodex/*.py` 未动，与 Rust 并存）
- remote：`origin` → https://github.com/dgy-github/nanocodex.git（凭据已配）
- Rust 工作区：`rust/`（crates 在 `rust/crates/`，GUI 在 `rust/gui/`）
- 工具链坑：本机无 MSVC，用 `x86_64-pc-windows-gnu`；每条 cargo 前 `export PATH="$HOME/.cargo/bin:$PATH"`

## 当前状态（已完成，全部编译+测试过，~172 测试全绿）
- 6 核心 crate：ncx-sandbox / ncx-config / ncx-provider / ncx-tools / ncx-core / ncx-cli
- CLI 二进制 `ncx`（一次性 + REPL + 斜杠命令）；Tauri v2 + Svelte 5 GUI（聊天/审批/设置/打包 .msi+.exe）
- 工具集：read_file · apply_patch · shell · update_plan · grep · glob · web_search · remember
- 分层 flash/pro 编排器（CLI `-o` 标志）：classify→plan(pro)→2×flash worker 并行→verify→闭环重试
- sub-agent 并行写隔离：每 worker 跑独立工作区副本；verifier 选 `BEST:<n>`、promote 该 worker 回真实 ws
- memory 自进化：`.ncx/memory/LEARNINGS.md` + remember 工具 + 启动召回注入 + 周期启发式 consolidate
- keyed 搜索后端：Tavily（有 key）否则回退 DuckDuckGo
- 性能：单文件 2.4MB（GUI 2.1–2.9MB 安装包）、启动 ~5ms（约 199× 快于 Python）
- 测试分布：core 61 / provider 31 / tools 36 / sandbox 15 / config 19 / cli 10

## 下一步（接手第一件事）
1. **未完成：LLM 版记忆摘要**（TaskList #17，in_progress）。要做：
   - `ncx-core/src/memory.rs` 加 `#[async_trait(?Send)] trait Summarizer { async fn merge(&[String])->Option<String> }`
   - `MemoryStore::summarize_consolidate(&dyn Summarizer, threshold)`：近似簇（Jaccard≥阈值）里 >1 条的让模型合并成 1 条，保留最新 ts + tags 并集；模型失败时 fallback 到「留最新」。mock 测试覆盖。
   - CLI 加 `LiveSummarizer`（fast provider 一次性）+ `--memory-merge` 维护命令（**别塞进每次启动，太贵**）。
2. **3 件增强做完且 `cargo test` 全绿后**：`git checkout -b rust-capability`，提交 `rust/` 与 `rust/gui/` 改动
   （**绝不 add `nanocodex/` 下的 .py**），`git push -u origin rust-capability`。remote 已确认存在。

## Do-Not（踩过的坑）
- 别把 tauri lib 设 `cdylib`/`staticlib`：gnu 链接器报 `export ordinal too large`，桌面用 `crate-type=["lib"]`。
- `@sveltejs/vite-plugin-svelte` 用 `^5` 配 vite `^6`（v4 锁 vite5 会 ERESOLVE）。
- 移植工具描述要**逐字照搬**（含格式示例）：apply_patch 描述精简过头 → 模型发 git-diff → 死循环。
  用 `NCX_TRACE=1` 抓 live tool 调用调试；`| head` 截管会 SIGPIPE 打断进程，验证时重定向到文件别用管道。
- GUI crate 用了 `async_trait` 宏 → 它自己 Cargo.toml 必须列 `async-trait`（否则 E0432 + 连带 E0195）。
- 别提交 `.claude/scheduled_tasks.json`（loop 会改它）。
- 校准用户预期：这些抬「完成率/可靠性/项目贴合度」，**不抬硬推理天花板**（封顶在 main 模型 deepseek-v4-pro < Fable）。
  上 5 成的真正杠杆是把 main 指向更强模型（`DeepSeekProvider` 已 OpenAI 兼容，改 base_url/key/model 零代码）。

## 记忆指针（auto-memory，已写）
rust-rewrite-setup · rust-rewrite-rationale · rust-apply-patch-tool-desc · rust-tauri-gui-gotchas · rust-orchestrator-capability
