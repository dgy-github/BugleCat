# Privacy Gateway · 精炼刷题卡（口播自检版）

> 对 `privacy-gateway`（本地隐私网关）刷题、录语音。少而尖：1 道总纲 + 3 道必刷 + 2 道补充。
> 刷法：① 只看【题目】录一段 → ② 对【答题骨架】自检漏了哪条 → ③ 背【一句话】 → ④ 想【易错点】别答错。
> 配套流程图：`privacy-gateway-flowcharts.zh-CN.html`（每题对应其中一张图）。

---

## Q0 · 总纲（一句话讲清这个网关）
**题目：** 这个 privacy gateway 是干嘛的？它怎么做到"数据不出域"？

- **考点：** 定位 + 隐私工程的整体判断。
- **答题骨架：**
  1. **local-first**：默认只用本地模型；`review/pro` 模式才在本地答完后把材料发线上（Codex）复核。
  2. **不出域三道闸**：① 命中密钥硬 <code>block</code> 不外发 ② 外发前 <code>redact_text</code> 脱敏 ③ 喂线上的上下文 <code>review_summary</code> 全程脱敏、secret 轮次直接跳过。
  3. **单入单出**：`/ask` 只暴露一个输入一个输出，`{output, session_id}`。
- **一句话：** local-first + 脱敏做在模型外 + 命中密钥硬 block——"数据不出域"是工程闸门，不是 prompt 请求。

---

## Q1 · 数据不出域 / blocked 【必刷】（图②）
**题目：** 什么内容会被拦住不外发？blocked 到底怎么触发的？

- **考点：** 隐私分级 + 脱敏机制的精确边界。
- **答题骨架：**
  1. **三级分级** `classify_privacy`：SECRET(3 条正则)→secret / PII(4 条)→internal / 内部标记词→internal / else public。
  2. **`redact_text`**：SECRET→`[REDACTED_SECRET]` <b>且置 blocked=True</b>；PII→`[REDACTED_PII]`（<b>不</b>置 blocked）。
  3. **blocked 时**：原始 user 文本被"禁止外发"占位替换，只把脱敏摘要送线上。
  4. **`merge_privacy`** 是 lattice：多源取最高密级（public0/internal1/secret2）。
- **一句话：** blocked 只由 3 条 SECRET 触发（密钥/token/PEM），PII 只脱敏不 block；脱敏在模型外做，不赌模型自觉。
- **易错点：** 别说"PII 也会 block"——PII 只替换。SECRET 3 条、PII 4 条要记准。

---

## Q2 · 何时真发线上复核 【必刷】（图③）
**题目：** 什么情况下才会把请求发到线上复核？mode 起什么作用？

- **考点：** 风险路由 `review_requirement` + 职责分离。
- **答题骨架：**
  1. **7 分支优先级**（先命中先返回）：隐私≠public → 高危词(44) → 业务规则词(19) → 反直觉词(15) → user&gt;80 → answer&gt;120 → 否则 low_risk。
  2. **mode 只有 local vs 非 local 两分支**（`pro` 无独立代码路径）；`local` 直接返回、非 local 才可能复核。
  3. **`review_requirement` 只判"要不要复核"，不决定 mode**（mode 由调用方传）；即便非 local，low_risk 也会走快路径跳过复核。
- **一句话：** 隐私优先级最高、有敏感必复核；risk 决定"是否发"、mode 决定"走不走复核这条路"，两者解耦。
- **易错点：** 别把 mode 说成三选一开关；别说 review_requirement 决定 mode。

---

## Q3 · 复核挂了怎么办 【必刷】（图④）
**题目：** 线上复核（Codex）调用失败了，系统怎么处理？会不会就把本地答案直接给用户？

- **考点：** 韧性 + fail-safe 判断力。
- **答题骨架：**
  1. **调用链**：本地答 → 判是否需复核 → 脱敏 `summarize_for_online` → 两条跳过快路径 → `codex.review` 打 `/responses`（gpt-5.4）。
  2. **多 key 轮转 + 回退**：可重试错误(401/403/408/409/429/5xx 或含 rate/quota/timeout)轮换 key；主 key 全败 → **deepseek 回退**。
  3. **失败 guard**：都失败时 <b>不是无脑放本地</b>——若"需复核 &amp; (策略非 allow_local 或已 blocked)" → `guarded-fallback` 拒绝把本地结论当最终答；否则才按 allow_local 返回本地(fallback=True)。
- **一句话：** 复核失败有 guard——命中隐私/需复核的场景宁可拒答也不把未复核的本地结论当最终答。
- **换业务：** 高合规场景把默认策略从 `allow_local` 改严，失败一律 guard。

---

## Q4 · 上下文怎么控（成本 + 不出域）【补充】（图⑤）
**题目：** 长会话上下文怎么管？既要省成本又要不泄露敏感？

- **考点：** 上下文工程。
- **答题骨架：**
  1. **prefix_cache 前缀整形**：把所有 system 合并成 `STABLE_SYSTEM_PREFIX` + 剥每轮变化的元数据 + 长消息中截 + 只留最后 16 条 → <b>前缀字节稳定 → 命中 llama.cpp KV cache</b>（省首 token 计算）。整形≠语义压缩。
  2. **后台异步压缩**：append 入 `context_compaction_queue`，`compaction_worker` 线程批量压（跳最近 4 轮、取旧≤8、secret 轮只留占位）。
  3. **两套上下文**：喂本地用全量 `context_messages`；喂线上用 `review_summary`——<b>全程脱敏、跳过 secret turn</b>。
- **一句话：** 前缀稳定命中 KV cache 省成本，后台压缩控长度，喂线上的上下文单独脱敏——成本和隐私两条线分开治。
- **易错点：** prefix_cache 是"整形让前缀稳定"不是"语义摘要"；实际用 `ContextStore`(sqlite+FTS)，`SessionStore`(内存版)没被用。

---

## Q5 · 意图路由为什么两级 【补充】（图⑥）
**题目：** 你的动作路由为什么要 classifier + LLM 两级，不直接上一个 LLM？

- **考点：** 路由设计 + 成本意识。
- **答题骨架：**
  1. **一级 ActionClassifier**（本地 lexical+embedding 召回器）：打分排序，&lt;0.55 无候选；把 catalog 收窄成 top5。
  2. **强召回直连**：conf≥0.9 &amp; score≥10 &amp; margin≥3 → 直接执行，<b>跳过 LLM</b>。
  3. **二级 IntentRouter**（小 LLM qwen3-router）：只在有召回时才调（无候选跳过省 LLM），定 `{kind,tool}`，tool conf&lt;0.7 降 clarify。
  4. **ActionRouter** 是唯一执行者（配置驱动 actions.yaml + 冷却锁）。
- **一句话：** 便宜的本地召回先收窄、能直连就不调 LLM，贵的 LLM 只做最后定夺——成本分层。
- **易错点：** `ActionPlanner` 已装配但当前 chat 链没用它，活跃路径是 classifier→intent_router。

---

### 刷题优先级
- **先刷 Q1 / Q2 / Q3**（不出域/何时复核/复核失败 guard）——网关最差异化、最能体现"隐私工程"判断。
- **Q0 做开场**，30 秒讲清定位。
- 和排产呼应：网关的"数据不出域" = 排产的"敏感本地" = 合规的"通道隔离"，都是同一条信条的不同承重。
