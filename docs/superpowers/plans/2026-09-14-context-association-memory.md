# Context Association and Memory Retrieval Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 建立可追溯、分层、冲突感知的上下文关联查询器。

**Architecture:** SQLite 作为事实事件、状态和索引存储；JSONL 保留追加式会话日志与灾备导出；Markdown 保留人工可读长期记忆。查询按当前会话/任务/工作区/长期记忆分层执行，最终进行时间、有效性、冲突和 token 预算裁剪。

**Tech Stack:** Rust、SQLite、JSONL、Markdown、现有 `Session`、`MemoryStore`、checkpoint。

**Spec:** `HANDOFF.md` 中 2026-09-14 工具策略记录及上下文/记忆管理讨论。

## Global Constraints

- 原始工具调用、工具结果、文件状态和用户指令必须可追溯，压缩不得删除事实来源。
- 当前活动轮和未完成工具调用不得进入压缩候选。
- 新指令按时间和作用范围覆盖旧指令；无法判定的相反指令必须标记冲突。
- 权限拒绝、用户拒绝和路径越界不得通过召回或 fallback 绕过。
- 所有查询结果必须带来源 ID、时间、session/task/agent 关联和有效状态。

---

### Task 1: 事实事件模型与 SQLite schema

**Files:**
- Create: `rust/crates/ncx-core/src/context_store.rs`
- Modify: `rust/crates/ncx-core/src/lib.rs`
- Test: `rust/crates/ncx-core/src/context_store.rs`

- [x] 定义 `ContextEvent` 字段：`event_id, session_id, task_id, agent_id, parent_agent_id, thread_id, turn_id, sequence, timestamp, event_type, status, payload_json, source_ref`。
- [x] 创建 SQLite 表、唯一键和索引：`session_id+sequence`、`task_id+status`、`tool_call_id`、`workspace`。
- [x] 实现追加写入、按 sequence 增量读取、终态查询和幂等 event_id 写入。
- [x] 测试断线补读、重复 event_id、乱序 sequence 和多 Agent 隔离。

### Task 2: JSONL/现有 Session 事件适配

**Files:**
- Modify: `rust/crates/ncx-core/src/session.rs`
- Modify: `rust/crates/ncx-core/src/session/semantic_compaction.rs`
- Test: `rust/crates/ncx-core/src/session/*`

- [x] 将现有 user/assistant/tool_call/tool_result/checkpoint 写入统一事件字段。
- [x] 保留 JSONL 原始记录，增加 source_ref，不覆盖旧日志格式。
- [x] 标记活动 turn 和未完成 tool_call，使压缩明确跳过它们。
- [x] 测试恢复后调用/结果配对、当前轮保护和历史可追溯性。

### Task 3: 分层 Context Retrieval

**Files:**
- Create: `rust/crates/ncx-core/src/context_retrieval.rs`
- Modify: `rust/crates/ncx-core/src/agent_loop.rs`
- Modify: `rust/crates/ncx-core/src/turn_context.rs`
- Test: `rust/crates/ncx-core/src/context_retrieval.rs`

- [x] 实现查询顺序：当前活动轮 → 当前 session 最近事件 → task/agent 未完成状态 → workspace 文件/Git 状态 → 长期记忆 → 历史摘要。
- [x] 每层设置 token 配额、去重键和来源元数据。
- [x] 采用结构化过滤、时间排序、关键词/全文匹配，再执行可选语义召回。
- [x] 仅将关联 notes 注入模型，不替代原始 tool result。

### Task 4: 指令有效性与冲突检测

**Files:**
- Create: `rust/crates/ncx-core/src/instruction_state.rs`
- Modify: `rust/crates/ncx-core/src/context_retrieval.rs`
- Test: `rust/crates/ncx-core/src/instruction_state.rs`

- [x] 记录指令的 polarity、scope、created_at、supersedes、revoked_by 和 status。
- [x] 检测“允许/禁止/撤销/限定”以及相同对象的相反指令。
- [x] 新指令覆盖旧指令时保留覆盖链；无法判定时返回 conflict，不自动选取。
- [x] 测试中文否定、相近但相反指令、跨 session 记忆不覆盖当前指令。

### Task 5: 记忆索引与合并安全

**Files:**
- Modify: `rust/crates/ncx-core/src/memory.rs`
- Modify: `rust/gui/src-tauri/src/memory_merge_job.rs`
- Test: `rust/crates/ncx-core/src/memory.rs`

- [x] 为 Markdown 记忆建立 SQLite 元数据索引：workspace、标签、时间、来源、状态和摘要关键词。
- [x] 合并时保留原始 note/source_ref，禁止把未经验证的会话摘要直接升级为长期记忆。
- [x] 记忆召回按 workspace、时间、状态和冲突链过滤。
- [x] 测试取消、workspace 切换、重复 note、旧记忆被新指令覆盖和失败恢复。

### Task 6: 工具状态与查询器集成

**Files:**
- Modify: `rust/crates/ncx-core/src/mcp_tool.rs`
- Modify: `rust/crates/ncx-core/src/agent_loop/turn.rs`
- Modify: `rust/gui/src-tauri/src/bridge.rs`
- Test: `rust/crates/ncx-core/src/agent_loop/tests/`

- [x] 在工具调用前后写入 tool_call/tool_result/node_state 事件。
- [x] 查询器返回工具状态、结果来源、退出码、workspace revision 和完成状态。
- [x] 未完成或状态 unknown 的写工具调用阻止自动压缩和重复执行。
- [x] 测试工具失败、部分成功、重试、取消、崩溃恢复和状态补读。

### Task 7: 压缩器接入与压力验证

**Files:**
- Modify: `rust/crates/ncx-core/src/session/semantic_compaction.rs`
- Modify: `rust/crates/ncx-core/src/agent_loop/compaction.rs`
- Create: `rust/crates/ncx-core/src/session/compaction_retrieval_tests.rs`

- [x] 仅压缩已完成历史；近期 N 轮按 token 预算动态选择，至少保留当前有效用户意图。
- [x] 摘要输出必须引用来源 ID，并区分事实、推断、已撤销指令和冲突。
- [x] 超大历史采用分块/增量摘要，单次摘要请求受独立输入预算限制。
- [x] 压测 1k/10k/100k 消息、超大工具输出、重复压缩、相反指令、截断摘要和取消恢复。
- [x] 运行 `cargo test -p ncx-core -p ncx-provider --lib`、Clippy、GUI 构建和离线恢复测试。

---

## 实施状态（2026-09-15，第三轮）

7 个任务、29 项 checkbox 全部完成。

第二轮遗留的四项本轮补齐：

- **Task 5 取消路径**：`MemoryIndex` 已接入 GUI 的 `memory_merge_job.rs`。索引重建放在 `commit_if_current` 的**同一把提交围栏内**——取消若赢得围栏，Markdown 重写和索引重建一起不发生，索引不可能描述一次从未落盘的合并。新增两项回归：取消后 Markdown 与索引都未变；提交后索引按合并结果重建。
- **Task 6 退出码逐调用关联**：新增 `ContextStore::tool_call_detail`，返回 tool/status/exit_code/result_hash/workspace_revision/source_ref/turn_id。退出码来自 shell 结果契约里既有的 `Exit code: N` 行（`classify_tool_result` 本就依赖同一契约）；**未报告退出码的工具就是没有，不会被推断成 0**。workspace revision 每轮读一次并由 `Session` 带进事件。
- **Task 7 GUI 构建**：`npm run build` 通过——protocol check v3（70 methods）、`tsc --noEmit`、Vite 155 modules。
- **运行时落差**：`RetrievalContextProvider` 现在持有 `MemoryIndex` 和 `InstructionLedger`，两层已真正参与注入。每个 direct-human 回合会把用户指令记进 ledger 并撤回与之矛盾的记忆；自动 Goal 回合不能触发撤回。

仍按「可选」处理未做的一项：Task 3 的**语义召回**。当前是结构化过滤 + 时间排序 + 关键词匹配；语义召回需要 embedding 依赖与额外模型调用，计划原文标注为可选。
