# nanocodex 视频口播版

> 用途：基于 `nanocodex-interview-project-intro.zh-CN.md` 派生的口播稿版本。  
> 适用场景：项目介绍视频、面试作品集视频、主页介绍视频、个人品牌短视频。  
> 口径要求：不夸大、不虚构、强调工程边界与本地交付。

## 1. 口播标题

- 我做了一个从零自研的 Coding Agent：`nanocodex`
- 一个真正能本地跑起来的 AI Coding Agent 项目
- 不只是接模型，我把 Coding Agent 做成了工程系统

## 2. 视频简介文案

项目名：`nanocodex`  
GitHub：<https://github.com/dgy-github/nanocodex>

这是一个从零自研的、本地可部署的 Codex 风格 coding agent。它不只是让模型回答代码问题，而是让模型在受控边界内调用文件、shell、补丁、搜索、MCP 等工具，完成真实的软件开发任务。项目重点放在权限控制、工具工程、上下文压缩、项目记忆和本地可交付。

## 3. 30 秒口播版

大家好，我做过一个从零自研的 coding agent，项目叫 `nanocodex`。  
它不是普通的代码聊天机器人，而是一个真正能在本地开发环境里工作的 AI agent。  
模型可以在受控边界里调用文件、shell、补丁、搜索和 MCP 工具，完成真实开发任务。  
我重点做了权限双层控制、工具系统、上下文压缩、项目记忆，还有本地 GUI 和 CLI 的交付。  
这个项目我先用 Python 跑通原型，后面又用 Rust 和 Tauri 重构成更适合长期维护和分发的版本。  
如果你关心 AI Infra、Agent Runtime 或 Developer Tools，这个项目会很有代表性。

## 4. 60 秒口播版

我做过一个从零自研的 coding agent，名字叫 `nanocodex`，GitHub 也已经开源了。  
这个项目想解决的，不是“怎么让模型会说代码”，而是“怎么把模型真正接进开发环境，而且还能控得住”。  

所以我做的不是一个普通聊天工具，而是一个本地可运行的 Codex 风格 agent。  
它可以在受控边界里调用文件、shell、补丁、搜索、MCP 和技能文档，去完成真实的软件开发任务。  

这个项目里我重点处理了几件事。  
第一，是 sandbox 和 approval 组成的双层权限控制。  
第二，是工具系统怎么动态暴露给模型，而不是一次把所有能力全塞进去。  
第三，是长任务里的上下文压缩和项目记忆，避免 agent 越跑越糊。  
第四，是本地 GUI、CLI 和原生分发，让它更像一个真正能交付的开发工具。  

整个项目经历了两个阶段：前期我用 Python 快速验证产品原型，后面再用 Rust 重写运行时，用 Tauri 做桌面端，把它收紧成更适合长期维护的工程系统。  
我觉得这个项目最能代表我的一件事，是我不是只会接模型，而是会把模型放进一个真正可控、可追责、可交付的工程边界里。

## 5. 90 秒口播版

我做过一个从零自研的 coding agent，项目叫 `nanocodex`，现在已经开源在 GitHub 上。  
这个项目的核心，不是做一个“会聊天的代码助手”，而是把 AI coding agent 真正做成一个工程系统。  

它本质上是一个本地可部署的 Codex 风格 agent。  
模型不是只负责回答问题，而是可以在受控边界里调用文件、shell、补丁、搜索、MCP 和其他工具，去完成真实的软件开发任务。  

但我一开始就不想把高风险控制交给模型自己判断。  
所以项目里最重要的设计之一，就是把安全和执行边界尽量收回代码侧。  
比如写操作不是直接放行，而是经过 sandbox 和 approval 两层正交控制。  
工具也不是全量暴露，而是按回合动态裁剪。  
上下文也不是满了就暴力总结，而是先做非破坏式编辑，再做更高成本的压缩。  

这个项目还有一个我自己很看重的点，就是它不是停在原型。  
我前期先用 Python 跑通 agent loop、工具调用、审批和桌面交互，确认产品面成立；  
后面再用 Rust 把运行时拆成更清晰的 crate 边界，再配上 Tauri 桌面壳，解决类型契约、启动性能和原生分发的问题。  

所以 `nanocodex` 对我来说，不只是一个 AI demo，  
而是一套关于工具工程、权限控制、上下文工程、本地交付和 agent runtime 的完整实践。  
如果岗位偏 AI Infra、Agent Platform 或 Developer Tools，这个项目会非常适合作为主讲项目。

## 6. 适合字幕版的短句拆分

1. 我做过一个从零自研的 coding agent。  
2. 项目名字叫 `nanocodex`。  
3. 它不是普通的代码聊天机器人。  
4. 它是一个本地可部署的 Codex 风格 agent。  
5. 模型可以在受控边界里调用真实工具。  
6. 比如文件、shell、补丁、搜索、MCP。  
7. 我重点做了权限双层控制。  
8. 也做了工具系统、上下文压缩和项目记忆。  
9. 前期我用 Python 验证原型。  
10. 后期我用 Rust 和 Tauri 做了工程化重构。  
11. 这个项目更像一个可交付的开发工具。  
12. 而不只是一个接上模型的 demo。

## 7. 适合视频首屏的 3 句 Hook

- 我不是只接了一个模型，我做了一套真正能跑在本地的 coding agent 系统。
- 大多数 AI 编码项目停在 demo，我把它做到了权限控制、工具系统和桌面交付。
- 如果你想知道 AI coding agent 怎么从原型走到工程系统，这个项目就是我的答案。

## 8. 适合视频结尾的 3 句收束

- 这个项目现在已经开源，GitHub 上可以直接看到完整实现。
- 对我来说，AI agent 最重要的不是“会说”，而是“能在边界里真正做事”。
- 如果岗位偏 AI Infra、Agent Platform 或 Developer Tools，`nanocodex` 会是我最想展开讲的项目之一。

## 9. 可直接喂视频生成器的口播输入

```yaml
video_topic: nanocodex 项目介绍
language: zh-CN
tone:
  - 专业
  - 稳定
  - 可信
  - 工程感强
target_audience:
  - 面试官
  - 技术同行
  - AI Infra 招聘方
core_message: >
  nanocodex 不是普通代码聊天机器人，而是一个从零自研、本地可部署、
  强调权限边界、工具工程、上下文压缩和工程交付的 coding agent 系统。
must_mention:
  - 项目名 nanocodex
  - GitHub 链接 https://github.com/dgy-github/nanocodex
  - Python 原型 -> Rust 重构 -> Tauri 桌面端
  - sandbox / approval 双层控制
  - tool calling / MCP / context compaction / project memory
avoid:
  - 夸大成通用 AGI
  - 说成纯聊天机器人
  - 虚构没有实现的机制
recommended_duration:
  - 30s
  - 60s
  - 90s
```
