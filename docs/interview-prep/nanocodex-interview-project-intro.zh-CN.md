# nanocodex 面试项目介绍（内容生成前置）

> 用途：这份文档不是给面试官直接读的，而是作为后续生成口播、自我介绍、项目介绍、PPT、宣传文案、短视频脚本时的统一上游输入。  
> 原则：后续生成内容时，优先基于这份文档改写，不要每次从 README、设计稿、流程图里重新拼素材，避免口径漂移。

## 1. GitHub / 在线链接

- GitHub 仓库：<https://github.com/dgy-github/nanocodex>
- 在线能力页：<https://dgy-github.github.io/nanocodex/nanocodex.html>
- 中文 README：<https://github.com/dgy-github/nanocodex/blob/main/README.zh-CN.md>
- 面试总入口：<https://github.com/dgy-github/nanocodex/blob/main/docs/interview-prep/INTERVIEW-MASTER-MAP.zh-CN.html>
- nanocodex 主看物图谱：<https://github.com/dgy-github/nanocodex/blob/main/docs/interview-prep/nanocodex-interview-final-map.zh-CN.html>

## 2. 一句话定位

`nanocodex` 是一个从零自研的、可本地部署的 Codex 风格 coding agent 项目，重点不只是“把模型接起来”，而是把工具调用、权限控制、上下文压缩、项目记忆、MCP、成本控制、桌面端交互和可分发交付做成一个完整工程系统。

## 3. 面试版 30 秒介绍

我做过一个从零自研的 coding agent，项目叫 `nanocodex`。它本质上是一个 Codex 风格的本地开发代理，不只是调用大模型回答代码问题，而是让模型能够在受控边界里调用文件、shell、补丁、搜索、MCP 等工具，完成真实的软件开发任务。我在这个项目里重点解决的是权限双层控制、工具工程、上下文压缩、项目记忆和本地可交付这几件事，后来又把它从 Python 原型线重构到 Rust + Tauri，变成更适合长期维护和分发的版本。

## 4. 面试版 90 秒介绍

`nanocodex` 可以看成我对“AI coding agent 怎么从 demo 走到工程系统”这件事的一次完整实现。项目早期先用 Python 验证 agent loop、工具调用、审批模型和桌面交互，证明产品面是成立的；后面再用 Rust 把核心运行时重写成更清晰的 crate 边界，加上 Tauri 桌面壳，解决启动性能、类型契约、可维护性和原生分发的问题。

它的核心不是让模型“自己决定一切”，而是把高风险部分尽量放回代码侧处理。比如工具暴露不是一次把所有 schema 都塞给模型，而是有动态 schema 选择和 `tool_search`；写操作不是直接放行，而是经过 sandbox 和 approval 两层正交门控；上下文不是满了就暴力摘要，而是先做非破坏式上下文编辑，再做更高成本的压缩；项目记忆也不是简单向量库堆砌，而是围绕本地工作流做按需召回和整理。整个项目体现的是一种很明确的工程观：LLM 负责推理和表达，确定性边界、安全约束、执行控制和可追责性必须落在系统代码里。

## 5. 这项目最值得讲的 5 个点

1. **权限双层正交**
   - 一层是 sandbox，决定物理上能不能读、写、联网。
   - 一层是 approval，决定超出边界时是自动拒绝、询问，还是在特定策略下放行。
   - 关键点不是“弹不弹窗”，而是安全边界不依赖模型措辞。

2. **从 Python 原型到 Rust 重构**
   - Python 线适合快速验证产品形态。
   - Rust 线适合收紧运行时边界、显式化类型契约、优化分发体验。
   - 这不是简单换语言，而是工程阶段切换。

3. **工具工程而不是纯聊天**
   - 模型要在真实仓库里调用文件、shell、补丁、搜索、MCP、技能文档等工具。
   - 我关注的是工具如何暴露、如何裁剪、如何并发、如何回填结果、如何在预算内安全运行。

4. **上下文压缩与项目记忆**
   - 不是粗暴“满了就总结”。
   - 优先做非破坏式上下文编辑、工具结果瘦身、动态可见视图，再配合项目记忆。
   - 目标是让长任务还能持续推进，而不是越跑越糊。

5. **本地部署与桌面交付**
   - 不依赖云端 IDE 才能用。
   - CLI、桌面 GUI、配置、审批、日志、成本、会话恢复都在本地闭环。
   - 更像一个可交付的开发工具，而不是实验脚本。

## 6. 适合投递 / 讲述的岗位方向

- AI Infra / AI Engineering
- Agent Platform / Agent Runtime
- Coding Agent / Developer Tools
- 本地部署 AI 工具链
- 企业级 AI 应用平台

如果岗位更偏“基建 / 框架 / 平台”，`nanocodex` 很适合作为主讲项目。  
如果岗位更偏“业务落地 / 治理 / 企业流程”，排产项目通常更容易对口。

## 7. 生成内容时必须保留的事实锚点

- 项目名：`nanocodex`
- 仓库：<https://github.com/dgy-github/nanocodex>
- 项目类型：本地可运行的 Codex 风格 coding agent
- 技术路线：先 Python 原型，再 Rust 重构，桌面端使用 Tauri
- 核心能力：
  - 工具调用
  - sandbox / approval 双层控制
  - 上下文压缩
  - 项目记忆
  - MCP 集成
  - skills 机制
  - 本地 GUI / CLI
- 核心视角：不是“一个会写代码的聊天机器人”，而是“一个把模型接入真实开发环境、并用工程边界约束住它的系统”

## 8. 生成内容时不要乱写的点

- 不要写成“所有决策都交给模型”。
- 不要写成“安全主要靠提示词约束”。
- 不要把它说成纯 SaaS 产品，它强调的是本地运行和本地控制。
- 不要把项目记忆直接说成“向量数据库语义检索”，如果要说，必须谨慎落到真实实现。
- 不要把任何未核实的机制说成既成事实，尤其是 best-of-N、编排、长期记忆这类容易被深问的点。

## 9. 可直接喂给生成器的结构化前置

```yaml
project_name: nanocodex
repo_url: https://github.com/dgy-github/nanocodex
project_type: 本地可部署的 Codex 风格 coding agent
stage:
  - Python 原型验证
  - Rust 运行时重构
  - Tauri 桌面化交付
one_line_positioning: >
  一个从零自研的 coding agent 工程系统，重点在工具调用、权限控制、
  上下文压缩、项目记忆、MCP 集成和本地可交付，而不只是接一个大模型聊天。
core_keywords:
  - coding agent
  - tool calling
  - sandbox
  - approval
  - context compaction
  - project memory
  - MCP
  - Tauri
  - Rust
  - local-first
interview_angles:
  - 为什么从 Python 过渡到 Rust
  - 为什么安全边界要落在代码而不是提示词
  - 为什么工具暴露要动态裁剪而不是全量给模型
  - 为什么上下文压缩要先做非破坏式编辑
  - 为什么本地部署和原生分发对开发工具很重要
target_roles:
  - AI Infra
  - Agent Platform
  - Developer Tools
  - 企业 AI 工程
must_keep_claims:
  - 这是一个从零自研的项目
  - 重点是工程边界而不是单纯模型接入
  - Python 是原型线，Rust 是产品化重构线
  - 项目关注本地可运行、可控制、可分发
avoid_claims:
  - 所有判断都靠模型
  - 安全主要靠 prompt
  - 未核实的 best-of-N / 编排细节
  - 夸大成通用 AGI 平台
```

## 10. 可直接复制的生成指令

如果后面要生成口播、自我介绍、项目介绍页、视频文案，可以先把下面这句和上面的 YAML 一起喂给模型：

```text
请基于这份 nanocodex 项目介绍前置材料，生成一版适合面试场景的中文内容。要求：
1. 保留事实锚点，不虚构没有实现的机制；
2. 强调“工程边界、工具系统、本地交付”，不要写成普通聊天机器人；
3. 语言自然、专业、可信；
4. 如果输出用于口播，优先使用短句和可讲述表达；
5. 如无特别要求，默认面向 AI Infra / Agent Platform / Developer Tools 岗位。
```
