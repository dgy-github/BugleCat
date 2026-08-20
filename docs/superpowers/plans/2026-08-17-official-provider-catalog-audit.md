# 官方模型目录审计实施计划

> **执行方式：** 按任务逐项执行并保留测试证据。

**目标：** 将内置模型目录替换为经厂商官网逐项核验、可直接配置且价格来源明确的当前精选模型。

**方案：** 目录继续保留为应用内置的精选快照，不把聚合平台价格混入厂商直连价格。每个型号都携带其自身的官方价格页、核验日期、币种和直连接口；官方尚未开放按量 API 或未公开标准价格的型号不加入可一键应用的目录。

**技术栈：** Rust、Tauri、Svelte、现有 `cargo test` 与 Vite 构建。

## Global Constraints

- 只使用厂商官网公布的直连接口和价格；OpenRouter 仅作为单独的聚合渠道。
- 价格单位固定为每百万 Token 的当前公开输入/输出单价；缓存、长上下文阶梯和限时优惠必须明确说明。
- 不将订阅套餐、未开放的按量 API 或第三方转售价格写为厂商直连价格。
- 目录为常用精选，不承诺覆盖每家厂商全部历史型号；所有显示型号必须能被一键配置。
- 全部界面文案使用中文。

---

## 文件职责

- `rust/gui/src-tauri/src/model_catalog.rs`：官方直连型号、价格、来源和回归测试的唯一所有者。
- `rust/gui/src/App.svelte`：设置页展示核验日期和价格适用范围。
- `docs/superpowers/plans/2026-08-17-official-provider-catalog-audit.md`：本次取数边界和执行证据。

### Task 1: 为官方精选目录写回归测试

**Files:**
- Modify: `rust/gui/src-tauri/src/model_catalog.rs:421-488`

**Interfaces:**
- Consumes: `catalog() -> Vec<CatalogProvider>`
- Produces: 官方型号集合、型号 ID、价格、币种和核验日期的回归约束。

- [x] **Step 1: 写失败测试**

```rust
#[test]
fn official_catalog_uses_the_audited_current_models() {
    let ids = |provider_id: &str| {
        catalog().into_iter().find(|p| p.id == provider_id).unwrap()
            .models.into_iter().map(|m| m.model_id).collect::<Vec<_>>()
    };
    assert_eq!(ids("openai"), vec!["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"]);
    assert_eq!(ids("gemini"), vec!["gemini-3.7-flash"]);
    assert_eq!(ids("moonshot"), vec!["kimi-k3", "kimi-k2.7-code"]);
    assert_eq!(ids("minimax")[0], "MiniMax-M3");
}
```

- [x] **Step 2: 运行失败测试**

Run: `cargo +1.96.0-x86_64-pc-windows-gnu test -p ncx-gui --target x86_64-pc-windows-gnu official_catalog_uses_the_audited_current_models`

Expected: FAIL，因为旧目录仍含 `gpt-5`、`gemini-2.5-*`、`kimi-k2.5` 和 `MiniMax-M2.7`。

- [x] **Step 3: 为价格和来源写失败测试**

```rust
#[test]
fn audited_official_prices_keep_provider_currency_and_date() {
    for provider in catalog().into_iter().filter(|p| p.id != "openrouter") {
        for model in provider.models {
            assert_eq!(model.price_source, PriceSource::OfficialDirect);
            assert_eq!(model.updated_at, "2026-08-17");
            assert!(model.source_url.starts_with("https://"));
        }
    }
}
```

- [x] **Step 4: 运行失败测试**

Run: `cargo +1.96.0-x86_64-pc-windows-gnu test -p ncx-gui --target x86_64-pc-windows-gnu audited_official_prices_keep_provider_currency_and_date`

Expected: PASS only after every updated record retains an official source and audit date.

### Task 2: 替换为可直连的官方精选快照

**Files:**
- Modify: `rust/gui/src-tauri/src/model_catalog.rs:33-358`
- Test: `rust/gui/src-tauri/src/model_catalog.rs:421-520`

**Interfaces:**
- Consumes: `model(provider_id, model_id, display_name, base_url, price_in, price_out, price_currency, source_url, context_length)`
- Produces: `catalog()` 中可被 `apply_model_preset` 安全写入配置的直连型号。

- [x] **Step 1: 替换阿里、火山、智谱、Kimi 和 MiniMax 条目**

```rust
// 仅保留官网当前可直连的精选项：
// qwen3.7-max = ¥12/¥36；doubao-seed-evolving = ¥6/¥30；
// doubao-seed-2.0-code = ¥3.2/¥16；glm-5.2 = ¥8/¥28；
// kimi-k3 = $3/$15；kimi-k2.7-code = $0.95/$4；MiniMax-M3 = ¥2.1/¥8.4。
```

- [x] **Step 2: 替换 OpenAI 和 Gemini 条目**

```rust
// gpt-5.6-sol = $5/$30；gpt-5.6-terra = $2/$12；gpt-5.6-luna = $0.2/$1.2。
// gemini-3.7-flash = $0.75/$3.75（限时至 2026-12-31），使用官方 OpenAI 兼容地址。
```

- [x] **Step 3: 更正 Kimi 与火山的模型 ID、直连接口和官方来源链接**

```rust
const MOONSHOT_PRICING: &str = "https://platform.kimi.ai/";
// Kimi API 地址为 https://api.moonshot.ai/v1
// Seed-Evolving 使用稳定模型 ID doubao-seed-evolving。
```

- [x] **Step 4: 运行指定测试并确认通过**

Run: `cargo +1.96.0-x86_64-pc-windows-gnu test -p ncx-gui --target x86_64-pc-windows-gnu model_catalog`

Expected: PASS；旧型号不能重新出现在任何官方精选厂商中。

### Task 3: 在设置页让核验范围可见

**Files:**
- Modify: `rust/gui/src/App.svelte:1829-1846`

**Interfaces:**
- Consumes: `CatalogModel.updated_at`、`price_source`、`source_url`
- Produces: 每张官方模型卡显示“已按官网核验：2026-08-17；显示常规输入/输出价，缓存、长上下文阶梯和促销另计”。

- [x] **Step 1: 写入固定的中文说明**

```svelte
<small class="catalog-audit-note">
  已按官网核验：{model.updated_at}；显示常规输入/输出价，缓存、长上下文阶梯和促销另计。
</small>
```

- [x] **Step 2: 运行前端构建**

Run: `npm run build`

Expected: PASS，Svelte 类型检查和 Vite 打包均无错误。

### Task 4: 回归、格式化和本地运行

**Files:**
- Modify: `rust/gui/src-tauri/src/model_catalog.rs`
- Modify: `rust/gui/src/App.svelte`

- [x] **Step 1: 格式化 Rust**

Run: `cargo +1.96.0-x86_64-pc-windows-gnu fmt --check`

Expected: PASS。

- [x] **Step 2: 运行 GUI 后端测试**

Run: `cargo +1.96.0-x86_64-pc-windows-gnu test -p ncx-gui --target x86_64-pc-windows-gnu`

Expected: PASS。

- [x] **Step 3: 构建并启动新版桌面程序**

Run: `npm run tauri:build`

Expected: PASS；启动新生成的 `ncx-gui.exe`，在设置页验证厂商官方目录、价格来源和核验日期。

- [ ] **Step 4: 提交**

```text
git add rust/gui/src-tauri/src/model_catalog.rs rust/gui/src/App.svelte docs/superpowers/plans/2026-08-17-official-provider-catalog-audit.md
git commit -m "修复：核验厂商官方模型目录"
```
