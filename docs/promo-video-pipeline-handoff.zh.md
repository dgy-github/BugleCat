# 宣传片全流程方案 · 交接文档

> 元信息：2026-06-30 · 目标档次：**品牌宣传片级** · 生成后端：**复用 `ncx-dreamina-gateway`（即梦/剪映系）** · status: `draft v0.1`
>
> 读法：这份文档给的是**全流程 + 复用清单 + 落地阶段 + 坑**。接手第一件事看 §7「待确认项」——那几项没定，下面的实现细节会跑偏。

---

## 0. 一句话

用 agent 编排一条链：**brief → 脚本 → 分镜 → 首帧素材 → dreamina 出镜头 → 配音配乐 → 图形动效包装 → 剪辑合成 → 审校 → 多版本导出**。

品牌级质感 = **关键镜头 AI 生成（I2V + 一致性锁定）+ 图形动效包装 + 两道人工闸**。不是全自动，也不该全自动。

---

## 1. 目标与硬边界

- **目标**：品牌宣传片级（实拍级镜头感、强一致性、精致包装），不是口播/图文 shorts。
- **后端**：视频镜头生成复用 `ncx-dreamina-gateway`。
- **硬事实（别绕）**：开源没有 turnkey 的 brand-grade 出片方案。所有现成项目要么是 shorts 拼接（MoneyPrinterTurbo），要么是研究级 demo（ViMax/MovieAgent）。**brand-grade 只能走"AI 出素材 + 人工把关 + 图形包装"的混合路线。**
- **全流程最难的一点**：**跨镜头一致性**（产品外观、角色、色调、光线在不同镜头间会漂）。这是 brand feel 的命门，也是 §5 单列一节的原因。

---

## 2. 全流程 pipeline

| # | 阶段 | 输入 | 输出 | 复用什么 | brand-grade 要点 | 人工闸 |
|---|---|---|---|---|---|---|
| 1 | **立项 brief** | 产品/卖点/受众/渠道 | creative brief | — | 定调性、参考片(benchmark)、竖横/时长/渠道 | 🔴 人定 |
| 2 | **脚本文案** | brief | 旁白脚本 + 叙事结构 | LLM | 结构：钩子→痛点→方案→证据→CTA；时长分配 | 🔴 终稿人定 |
| 3 | **分镜 storyboard** | 脚本 | 逐镜表(画面/景别/运镜/时长/旁白/情绪) | 你的 `storyboard`(plan_storyboard) | 镜头语言 + 一致性锚点(哪些镜头共用产品/角色/色板) | 审 |
| 4 | **首帧/视觉资产** | 分镜 + 品牌 asset | 每镜首帧图 + 参考图库 | 文生图 / 产品图 / logo | **I2V 首帧是一致性的锁**；固定色板/logo/产品图 | 审 |
| 5 | **镜头生成** | 首帧 + 镜头 prompt | 每镜视频片段 | **`ncx-dreamina-gateway`** | I2V 优先(锁一致性)；多 seed、挑选、局部重生成 | 🔴 选镜 |
| 6 | **配音 + 配乐** | 脚本 | 旁白音轨 + BGM + SFX | TTS / 音乐库 | 情绪匹配；卡点用的节拍表 | 审 |
| 7 | **图形动效包装** | logo/字幕/数据 | 字体动画/下三分之一/转场/片头尾 | **OpenMontage**(HTML/CSS/GSAP) | brand feel 一半在这里，别省 | 审 |
| 8 | **剪辑合成** | 5/6/7 全部 | 粗剪→精剪成片 | FFmpeg / 剪映 / dreamina 剪辑 | 按分镜时间线 + 音乐卡点对齐 + 调色 | 🔴 调色/节奏 |
| 9 | **审校迭代** | 成片 | 返工清单 | 你的 `check_continuity` | 一致性/品牌合规/卡点逐镜比对 | 🔴 终审 |
| 10 | **导出交付** | 定稿 | 多版本(竖/横/裁时长)+字幕文件 | FFmpeg | 各渠道适配、字幕 srt、封面 | — |

---

## 3. Agent 编排：哪些自动、哪些必须人工闸

品牌级的关键不是"多自动"，是**把人工判断放对位置**（和 harness 那套一个道理：agent 产"待审素材"，人在闸上拍板）。

- **agent 自动**：脚本草稿、分镜草稿、首帧候选批量出、镜头批量生成(多 seed)、字幕、粗剪、continuity 初筛。
- **人工闸（brand-grade 必须保留）**：
  1. **brief 定调** —— 源头的价值/调性，只能人定。
  2. **脚本终稿** —— 批量出镜头前必须锁死（返工最贵）。
  3. **选镜** —— 从 dreamina 的多 seed 候选里挑，漂了的重生成/换实拍。
  4. **调色 + 节奏** —— brand feel 的手感。
  5. **终审** —— 一致性 + 品牌合规。

> 映射：agent 全程产"待审素材"（同"分级置信/待验证主张"），人只在上面 5 道闸上拍板。**别让 agent 越过闸直接定稿。**

---

## 4. 复用清单（buy vs build）

| 需要的能力 | 直接复用 | 说明 |
|---|---|---|
| 分镜→镜头**一致性架构** | 参考 **ViMax**（首帧参考图锁多镜头一致性）、**MovieAgent**（director/shot agent 分解） | 别从零发明分镜 agent 的拆法 |
| **图形动效包装** | **OpenMontage**（HTML/CSS/GSAP 程序化 promo，适配 Claude Code） | brand feel 的包装腿 |
| **装配腿**（TTS→字幕→BGM→FFmpeg 合成） | **MoneyPrinterTurbo** 的 service 层 | 这段最成熟，抄它 |
| **镜头生成** | **`ncx-dreamina-gateway`** | 你已有 |
| 分镜 + 连续性 | 你的 `storyboard`（plan_storyboard / check_continuity） | 你已有，接上即可 |

链接见文档末尾。

---

## 5. 一致性怎么保（brand-grade 命门，单列）

多镜头之间产品/角色/色调漂移 = brand-grade 最容易崩的地方。对策，从强到弱：

1. **I2V 优先，不要纯 T2V 拼多镜头**：每个镜头用**锁定的首帧图**驱动生成，首帧承载了产品/角色/色板 → 一致性从源头锁。⚠️ **当前 gateway 不支持 I2V/首帧（见 §7 第 1 条，已源码核实）——本节方案在 gateway 补齐 I2V 端点前无法落地，是全案前置阻塞项。**
2. **固定参考 asset 库**：产品图、角色参考、色板、logo 做成固定资产，每镜注入。
3. **seed / 风格锁定 + 局部重生成**：同一场景复用 seed；漂的局部重生成，不整条重来。
4. **continuity check agent**：用你已有的 `check_continuity`，逐镜比对上下镜的产品/色调/光线，产返工清单。
5. **兜底**：漂移严重、AI 救不回来的镜头 → **人工重挑 seed 或直接换实拍/图形**。别硬凹。

---

## 6. 分阶段落地（MVP → full，飞轮式）

- **P0（MVP，先跑通一条链）**：单产品 · 30s · 竖屏 · 混合档。`brief → 分镜 → dreamina I2V 出镜头 → TTS 字幕 → FFmpeg 合成`。目标是**端到端出一支能看的片**，不追求完美。
- **P1（加质感）**：图形动效包装（OpenMontage）+ BGM 卡点 + continuity check 接入。
- **P2（多版本 + 选镜）**：多 seed 批量出→选镜、竖/横/裁时长多版本导出、品牌合规校验。
- **P3（编排收口）**：agent 编排把 §2 全链串起来，§3 的人工闸标准化成审核界面。

每一阶段都要**产出一支真片**再往下走（别攒到最后一次性验收）。

---

## 7. 待确认项（没定这几个，下面会跑偏 — 交接后第一件事）

1. ~~**dreamina gateway 能力**~~ → ✅ **已源码核实（2026-06-30，`rust/crates/ncx-dreamina-gateway/src/lib.rs`）**：
   - **当前是 mock，不发真实请求**（`provider_mode:"mock"` 写死；README 明示 first local-test stage 不发真实 Dreamina 请求）。**连真实出图都还没接。**
   - **唯一生成端点 `/v1/images/generations` 是纯文生图**：请求体只有 `model/prompt/n/size/response_format`，**无图片输入字段 → 不支持 I2V、无首帧/参考图接口**。
   - **视频只在 `/v1/models` 目录挂名**（`jimeng-video-seedance-*` 等 9 个），**没有任何视频生成路由 → 视频未接线**。
   - **同步返回，无异步任务/轮询**；账号池(≤5 sessionid 轮询)+ API key 鉴权是脚手架，真实 provider adapter 未实现。
   - **结论**：要支撑本方案，gateway 需先做三件事 →（a）接真实 provider adapter 替掉 mock；（b）加视频端点 + 异步任务/轮询；（c）请求体加图片输入(首帧/参考图)字段。**这是本方案的前置阻塞项。**
2. **品牌 asset 从哪来**：产品图 / logo / 色板 / 角色参考谁提供？
3. **成片规格**：目标时长、投放渠道、竖屏还是横屏、几个版本？
4. **旁白**：TTS 还是真人配音？TTS 用哪家？
5. **音乐**：有无版权 BGM 库（brand 片不能用来路不明的音乐）？

---

## 8. Do-Not（坑，别重复踩）

- ❌ **别指望全自动出 brand grade** —— 没有人工闸必崩；brand feel 是判断，不是流水线。
- ❌ **别用纯 T2V 拼多镜头** —— 一致性一定漂；brand-grade 走 I2V + 首帧参考图。
- ❌ **别在 brief/脚本终稿锁定前批量出镜头** —— 返工成本最高的就是这一步。
- ❌ **别把 dreamina 当稳定同步接口** —— 要做异步任务 + 失败重试 + 多 seed 挑选。
- ❌ **别省图形动效包装和音乐卡点** —— brand feel 一半在节奏和包装，不在单镜画面。
- ❌ **别让 agent 越过人工闸直接定稿** —— agent 只产"待审素材"，拍板在人。

---

## 9. gateway 改造任务清单（前置阻塞项 · 带接口草案）

> 目标：把 `ncx-dreamina-gateway` 从"图片 mock 门面"改造成"能真实出视频、支持 I2V 首帧"的后端。行号基于 `rust/crates/ncx-dreamina-gateway/src/lib.rs`（2026-06-30 核实）。
> 推荐顺序：**T1 → T3 → T2**（先打通真实通路和首帧字段，再上异步视频）。代码为**草案**，非可编译成品；新依赖需加（`reqwest`、`async-trait` 等）。

### T1 · 接真实 provider adapter（替掉 mock）

- **现状**：`provider_mode` 写死 `"mock"`（`health` `lib.rs:439`）；`images_generations`（`lib.rs:454`）返回假 URL；`pick_token_label`（`lib.rs:609`）**拿到 token 却只用 label、丢掉了 sessionid**——真实调用要用 sessionid（`pick_provider_token` `lib.rs:222` 本来就返回完整 token）。
- **改**：抽 provider trait，mock/jimeng 两实现；`provider_mode` 改由 env 驱动。
- **接口草案**：
```rust
// src/provider/mod.rs
#[async_trait::async_trait]
pub trait DreaminaProvider: Send + Sync {
    async fn generate_image(&self, req: &ImageJob) -> Result<Vec<Asset>, GatewayError>;
    async fn submit_video(&self, req: &VideoJob) -> Result<String, GatewayError>; // -> provider_job_id
    async fn poll_video(&self, provider_job_id: &str) -> Result<JobStatus, GatewayError>;
}
pub struct MockProvider;                                   // 现有 mock 行为
pub struct JimengProvider { http: reqwest::Client }        // 用 sessionid 调即梦网页 API
```
- **配置**：`GatewayConfig`（`lib.rs:24`）加 `provider: ProviderKind`，读 `NCX_DREAMINA_PROVIDER=mock|jimeng`。
- **验收**：`NCX_DREAMINA_PROVIDER=jimeng` 时 `/v1/images/generations` 出真实 URL（非 mock）。

### T3 · 请求体加图片输入（首帧/参考图，I2V 的前提）

- **现状**：`ImagesRequest`（`lib.rs:396`）只有 text；`chat_completions` 的 `extract_prompt`（`lib.rs:629`）**丢弃 `image_url`**。
- **改**：请求体加图片输入字段（url 或 b64）。
- **接口草案**：
```rust
pub struct ImageInput { pub url: Option<String>, pub b64_json: Option<String> }
// ImagesRequest / VideosRequest 增加：
pub reference_images: Option<Vec<ImageInput>>,   // 参考图 / 首帧
```
- **验收**：带 `reference_images` 的请求，provider 收到并用作首帧/参考。

### T2 · 加视频端点 + 异步任务/轮询（工作量大头）

- **现状**：视频模型只在 `built_in_models()`（`lib.rs:348`）挂名、无路由；生成是同步的。
- **改**：加"提交 + 轮询"两个端点 + 一个 job store（照抄 `StateStore` 落盘模式 `lib.rs:306`）。
- **接口草案**：
```rust
// 挂到 api_router（lib.rs:418）
.route("/v1/videos/generations", post(videos_submit))       // 提交 -> {id, status:"queued"}
.route("/v1/videos/generations/{id}", get(videos_poll))     // 轮询 -> {id, status, data:[{url}]}

pub struct VideosRequest {
    pub model: Option<String>,                     // jimeng-video-seedance-2.0-pro 等
    pub prompt: String,
    pub reference_images: Option<Vec<ImageInput>>, // 首帧(I2V)
    pub duration: Option<f32>,                     // 秒
    pub ratio: Option<String>,                     // "9:16" / "16:9"
    pub n: Option<u32>,
}
pub enum JobStatus {
    Queued, Running,
    Succeeded { assets: Vec<Asset> },
    Failed { error: String },
}
```
- **异步**：`videos_submit` 落一条 job → `tokio::spawn` 后台跑 `submit_video` + 轮询 `poll_video`；`videos_poll` 读 job store。
- **健壮性**：provider 调用包 N 次重试 + 超时；job 落盘（kill/重启后状态不丢）。
- **验收**：POST 视频模型 → 拿 job_id → 轮询到 `succeeded` + 真实视频 URL；重启后 job 状态仍在。

### 里程碑

- **M1（T1 + T3）**：真实出图 + 能带参考图 → 可先验证 §5 一致性里"首帧图"这一环。
- **M2（T2）**：真实出视频（I2V）→ §5 全案解锁，§2 全链可切回即梦。

---

## 附：复用项目链接

- OpenMontage：https://github.com/calesthio/OpenMontage
- ViMax（HKUDS）：https://github.com/HKUDS/ViMax
- MovieAgent（showlab）：https://github.com/showlab/MovieAgent
- MoneyPrinterTurbo：https://github.com/harry0703/MoneyPrinterTurbo
