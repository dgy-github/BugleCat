# nanocodex 项目能力记忆

本文件记录稳定的业务能力、模块职责和复用入口。完整文件说明在
`MODULE_CATALOG.md`，公共接口说明在 `INTERFACE_CATALOG.md`，符号级导航在
`SYMBOL_INDEX.md`。新增模块、移动职责或改变公共接口时必须更新本文件并重新生成目录。

## Python 参考实现

| 能力 | 主要位置 | 新功能优先复用/扩展 |
| --- | --- | --- |
| Agent 主循环 | `nanocodex/agent/loop.py` | 模型轮次、工具执行、hook、预算和结束条件 |
| 多 Agent 编排 | `agent/orchestrator.py`, `task_graph.py`, `roles.py`, `verifier.py` | 任务图、角色、验证和状态推进 |
| Prompt 与上下文 | `agent/prompt.py`, `compaction.py`, `orch_prompts.py` | 系统提示、压缩和编排提示 |
| 会话与历史 | `agent/session.py`, `session_index.py`, `state.py`, `store.py` | 消息、持久化、恢复、状态存储 |
| 记忆与事实 | `agent/memory_store.py`, `fact_merge.py`, `tools/record_fact.py` | 长期记忆、事实归并和写入入口 |
| Skills | `agent/skills_store.py`, `tools/skills_tool.py`, `builtin_skills/` | 技能发现、解析、CRUD 和内置工作流 |
| 沙箱与审批 | `sandbox/policy.py`, `approval.py`, `executor.py` | 权限决策和真实执行边界 |
| 工具系统 | `tools/base.py`, `tools/__init__.py` | `ToolContext`、注册表和通用工具契约 |
| 文件与 Shell | `tools/read_file.py`, `apply_patch.py`, `patch.py`, `shell.py` | 文件读取、补丁和命令执行 |
| Provider | `provider/base.py`, `provider/deepseek.py` | 模型请求、流式响应、工具调用和错误 |
| MCP 与市场 | `tools/mcp.py`, `mcp_store.py`, `marketplace.py` | MCP 发现、连接、存储和安装 |
| 配置 | `config.py` | 分层配置、校验、默认值和脱敏 |
| CLI/GUI | `cli.py`, `gui.py` | 用户命令、交互和桌面入口 |
| 调度 | `agent/schedule.py`, `schedule_runner.py`, `tools/schedule_tool.py` | 定时任务模型、执行器和工具接口 |
| Storyboard | `storyboard/`, `tools/storyboard_tool.py` | 分镜模型、外部客户端和流水线 |

## Rust 发布实现

| 能力 | 主要 crate | 新功能优先复用/扩展 |
| --- | --- | --- |
| 核心 Agent | `ncx-core/src/agent_loop.rs`, `agent_loop/turn.rs`, `runtime_profile.rs` | loop 生命周期、单轮状态机、CLI/GUI 共享装配、预算、模型热替换和结束条件 |
| Agent 上下文 | `ncx-core/src/turn_context.rs`, `prompt.rs` | 具名 turn-context provider、稳定顺序、可撤销注册和 prompt section 组装 |
| Agent 工具调度 | `ncx-core/src/agent_loop/tool_dispatch.rs`, `tool_scheduler.rs` | 公开 `ToolScheduler` 注入点；runtime 保留顺序屏障、只读并发判定、取消、事件和结果提交 |
| Agent Provider 接口 | `ncx-core/src/model_provider.rs` | loop 使用的可替换模型契约和 DeepSeek transport 错误映射 |
| 工具执行 | `ncx-tools` | 工具注册、执行、文件、补丁和检测 |
| Workspace inspection | `ncx-core/src/workspace_tools.rs` | Structured directory/path inspection plus fixed read-only Git status/diff tools; avoids platform-specific shell probes |
| 沙箱 | `ncx-sandbox` | policy 与 approval，不得在上层复制权限判断 |
| 配置 | `ncx-config` | 配置类型、加载和写回 |
| Provider | `ncx-provider` | 请求响应类型、流式处理和 Web 调用 |
| CLI | `ncx-cli` | 参数、runner 和交互入口 |
| MCP | `ncx-mcp`, `ncx-core/src/mcp_tool.rs`, `ncx-cli` | stdio 协议适配、工具预加载和 `/mcp reload` 原子热替换；失败保留旧集合 |
| 视频 Agent | `ncx-video-agent` | 媒体、任务、渲染、追踪、转写和结构化结果 |
| Dreamina 网关 | `ncx-dreamina-gateway` | 外部生成服务边界 |
| Tauri GUI | `rust/gui` | Svelte UI、Tauri 命令和 capabilities |

## 禁止重复的核心规则

- 沙箱、审批、路径和命令安全必须集中在现有执行边界。
- Provider/MCP 响应解析不得散落到 UI 或业务模块。
- 会话、记忆、检查点和追踪格式必须通过已有存储/类型入口。
- 定价、预算、任务状态和视频任务状态机不得复制第二套计算逻辑。
- 配置默认值和脱敏规则只能由配置模块拥有。
