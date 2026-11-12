---
name: product-sense
description: Transform dry, engineer-centric features, UI, CLI output, and docs into engaging, high-delight product experiences with gamification, micro-interactions (Juice), witty micro-copy, and tangible user value. Use when designing features, reviewing UI/UX, or running /fun-review.
capability: general
---

# Product Sense & Gamification Playbook (`product-sense`)

Use this skill whenever the user wants to make a technical product, feature, UI component, CLI flow, or README **more engaging, intuitive, fun, or sticky** — especially when moving from "purely functional engineering" to "delightful product experience."

---

## 1. Core Philosophy: Engineer Brain vs. Product Brain

Purely technical implementations often suffer from **State Exposure Syndrome** (exposing raw database fields, config flags, and state machines directly to the user). Apply this translation lens first:

| Dimension | Engineer Brain (Dry / Low Fun) | Product Brain (Engaging / High Delight) |
| :--- | :--- | :--- |
| **First 30 Seconds** | "Configure these 8 TOML flags before running." | "Click once to see magic; tweak flags only if curious." |
| **Metrics & Numbers** | `saved_tokens: 18420, ratio: 0.7904` | "Saved 18.4k tokens today — roughly 1.5 cups of iced Americano ☕" |
| **System Progress** | ` learns_count: 6, state: shadow ` | "Stage 2 Apprentice Cat 🐾 (6/10 field lessons mastered before evolving)" |
| **Safety / Blocking** | `Error: Action denied by sandbox policy.` | "🛡️ Close call! Intercepted a risky `rm -rf` before it touched your home dir." |
| **Comparisons** | `Worktree A diff vs Worktree B diff` | "⚔️ A/B Duel Arena: Branch A finished 4s faster at 30% lower cost — crown the winner!" |
| **Waiting / Latency** | `Loading...` or blank spinner for 20s | Dynamic status storytelling + rotating geeky tips + celebratory finish. |

---

## 2. The 5-Pillar Fun & Product Sense Diagnostic

Evaluate the target feature, file, or screen across these 5 pillars:

### Pillar 1: "Juice" & Emotional Micro-Feedback (多汁感与即时情绪反馈)
In game design, **Juice** is the visceral feedback that makes an action feel alive:
- **Action Acknowledgement**: Does clicking, running a command, or completing a task feel tactile (spring animations, subtle sound/visual pulse, state-aware icon changes)?
- **Character & Mascot Expression**: If the product has a persona or mascot (e.g., **BugleCat 吹号猫**), how does its posture/expression react to idle, thinking, danger-blocking, and victory states?
- **Turn Failures into Saves**: Never make the user feel scolded by a guardrail. Frame sandbox blocks, lint catches, and pre-execution reviews as *"Your copilot just saved you from a headache."*

### Pillar 2: Visible Compounding & RPG Progression (养成感与可见复利)
Users love watching something grow (Octalysis Drive #2: Development & Accomplishment & Drive #4: Ownership):
- **Surface Hidden Intelligence**: If the system learns, caches, or optimizes in the background (e.g., memory entries, routing rules, compounding feedback), visualize it as **XP bars, unlocked perks, or a skill tree** rather than buried log files.
- **Milestones & Streaks**: Celebrate "1st bug squashed", "10th clean test run", or "100k tokens saved" with non-intrusive badges or session summary cards.

### Pillar 3: Playful Framing & Drama (戏剧化与竞技感包装)
- **A/B Worktrees & Benchmarks -> "Arena / Duel"**: Present side-by-side comparisons like a head-to-head match card (Speed, Cost, Cleanliness, Verdict).
- **Pre-Execution Review -> "Tactical Briefing / Shield Check"**: Give risk levels intuitive visual badges (🟢 Breeze, 🟡 Watch Out, 🔴 High Voltage) with a one-line human explanation.

### Pillar 4: Peak-End Rule & Wait-State Delight (峰终定律与等待期彩蛋)
Users judge an experience almost entirely by its **Peak** (most intense/delightful moment) and its **End** (completion summary):
- **Kill Dead Air**: During long LLM streaming, compilation, or test runs, show progressive narrative stages (e.g., *"Sniffing the AST..." -> "Cornering the bug..." -> "Polishing the patch..."*) instead of static spinners.
- **Victory Lap**: End complex multi-step tasks with a crisp, gratifying **Mission Summary Card** (time saved, files healed, tests green, fun one-liner).

### Pillar 5: 30-Second "Aha!" Onboarding (30秒爽点前置)
- Hide low-level plumbing (`os_sandbox`, bootstrap confidence intervals, RPC gates) behind collapsible "Under the Hood" sections or advanced tabs.
- Lead with **1-click interactive presets** ("Try a safe refactor duel", "Watch the sandbox block a rogue script").

---

## 3. Output Format When Running a Fun / Product Review

When reviewing code, UI components, CLI output, or docs, always structure your response in **3 actionable parts**:

### Part 1: 枯燥度诊断 (Dryness & Friction Diagnosis)
Identify the top 3–5 places where "engineer-speak" leaks out or emotional payoff is missing. Cite exact file paths, lines, or UI states.

### Part 2: 三档趣味化改造菜单 (3-Tier Fun Upgrade Menu)
Always give the developer choices sorted by implementation cost:

1. **🟢 Tier 1 — 5分钟微文案与彩蛋改造 (Zero-Architecture Micro-copy & metaphors)**
   - Provide a **Before vs. After** table of exact strings, labels, empty states, loading messages, and error prompts ready to copy-paste.
2. **🟡 Tier 2 — 30分钟“多汁感”交互与视觉反馈 (Juice & Visual Polish)**
   - Concrete UI/CLI enhancements: status badges, tangible unit conversions (tokens -> coffee/treats), micro-animations, mascot state hooks, or ASCII celebration banners.
3. **🔴 Tier 3 — 半天级“养成/竞技”游戏化闭环 (Sticky Gamified Loop)**
   - A high-leverage product feature idea (e.g., Agent Evolution Card, A/B Duel Scoreboard, "Saved Your Bacon" Counter) with a clear data-to-UI mapping.

### Part 3: 极客审美防翻车护栏 (Developer Taste Guardrails)
Every suggestion **must** obey these 4 rules so the product stays sharp, never childish or annoying:
- **Zero Hot-Path Tax**: Animations or summaries must never slow down keyboard navigation or add artificial delay to fast commands.
- **Freshness Decay Protection**: Witty copy must rotate from a pool or stay concise so it doesn't become annoying on the 50th run.
- **Signal Over Noise**: Keep raw technical data 1 click/hover away (e.g., show `"☕ Saved ~¥1.2 (18.4k tokens)"` with exact token breakdown on hover).
- **Respect the Craft**: Humor should feel like a clever senior engineer friend (dry wit, cat/bugle motifs, crisp metaphors), never corporate cringe.
