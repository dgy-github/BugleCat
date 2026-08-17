# 模型厂商目录与费用展示实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让用户按厂商选择可直接调用的模型，自动保存匹配的接口与费用，并能从 OpenRouter 实时获取全量模型目录。

**Architecture:** `ncx-config` 持有费用币种并保持旧配置兼容。GUI 后端保存精选预设和 OpenRouter 会话缓存，负责把一个预设原子写入配置；Svelte 设置页只渲染后端目录并调用预设命令，避免接口和价格逻辑散落到前端。

**Tech Stack:** Rust 1.96、Tauri v2、Svelte 5、reqwest、Node/Playwright、Cargo、Vite。

## Global Constraints

- 保持 Windows GNU 工具链；构建前把 WinLibs `mingw64\bin` 放到 `PATH` 前端。
- 主工作树有未提交的 `App.svelte`、`app.css` 改动；实施必须从 `654cc23` 创建独立工作区，不能覆盖这些改动。
- 所有价格按每百万 Token、原始币种 `CNY` 或 `USD` 展示；不做汇率换算。
- 自动化测试不得使用真实 API Key 或真实付费请求。
- Anthropic 原生 Messages API 不在本期直连范围；Claude 通过 OpenRouter 使用。

---

## 文件结构

- `rust/crates/ncx-config/src/config.rs`：新增 `price_currency` 运行时配置与校验。
- `rust/crates/ncx-config/src/loader.rs`：读取默认、配置文件和环境变量中的币种。
- `rust/crates/ncx-config/src/writer.rs`：写入币种和 `available_models`。
- `rust/crates/ncx-config/src/lib.rs`：导出币种常量。
- `rust/gui/src-tauri/src/model_catalog.rs`：精选目录、OpenRouter 解析器和纯函数测试。
- `rust/gui/src-tauri/src/lib.rs`：目录、刷新、原子应用预设命令与 DTO。
- `rust/gui/src/App.svelte`：厂商—模型联动、价格卡、币种、OpenRouter 搜索/刷新。
- `rust/gui/src/app.css`：设置页目录和价格卡局部样式。
- `rust/gui/e2e/question.mjs`：厂商预设与费用展示回归。
- `config.example.toml`：币种和快速切换示例。

## Task 1: 为费用币种扩展配置

**Files:**
- Modify: `rust/crates/ncx-config/src/config.rs`
- Modify: `rust/crates/ncx-config/src/loader.rs`
- Modify: `rust/crates/ncx-config/src/writer.rs`
- Modify: `rust/crates/ncx-config/src/lib.rs`

**Interfaces:**
- Produces: `Config::price_currency: String`、`VALID_PRICE_CURRENCIES`。
- Consumes: 现有 `price_in`、`price_out` 和 `write_nanocodex_config`。

- [ ] **Step 1: 写入失败测试**

在 `loader.rs` 测试模块加入：

```rust
#[test]
fn legacy_price_config_defaults_to_cny_and_explicit_usd_round_trips() {
    let tmp = std::env::temp_dir().join("ncx_config_test_price_currency");
    std::fs::create_dir_all(&tmp).unwrap();
    let paths = no_paths(&tmp);
    write(&paths.nanocodex, "api_key = \"k\"\nprice_in = \"1.25\"\n");
    let legacy = load_config_impl(Overrides::default(), &paths, &empty_env()).unwrap();
    assert_eq!(legacy.price_currency, "CNY");

    write(&paths.nanocodex, "api_key = \"k\"\nprice_currency = \"USD\"\n");
    let usd = load_config_impl(Overrides::default(), &paths, &empty_env()).unwrap();
    assert_eq!(usd.price_currency, "USD");
}
```

在 `writer.rs` 测试模块加入：

```rust
#[test]
fn writer_persists_currency_and_available_models() {
    let tmp = std::env::temp_dir().join("ncx_writer_test_price_currency");
    std::fs::create_dir_all(&tmp).unwrap();
    let target = tmp.join("config.toml");
    write_nanocodex_config(&map(&[
        ("price_currency", "USD"),
        ("available_models", "gpt-5,gpt-5-mini"),
    ]), &target).unwrap();
    let parsed: toml::Value = std::fs::read_to_string(target).unwrap().parse().unwrap();
    assert_eq!(parsed["price_currency"].as_str(), Some("USD"));
    assert_eq!(parsed["available_models"].as_str(), Some("gpt-5,gpt-5-mini"));
}
```

- [ ] **Step 2: 验证测试为红**

Run: `cargo test -p ncx-config legacy_price_config_defaults_to_cny_and_explicit_usd_round_trips --target x86_64-pc-windows-gnu`

Expected: FAIL，因 `price_currency` 或 writer 键尚不存在。

- [ ] **Step 3: 最小实现**

在 `config.rs` 添加：

```rust
pub const VALID_PRICE_CURRENCIES: &[&str] = &["CNY", "USD"];
// Config fields:
pub price_currency: String,
// Config::default:
price_currency: "CNY".to_string(),
```

`validate` 拒绝未知币种。loader 的默认合并值加入 `price_currency = "CNY"`，支持 `NANOCODEX_PRICE_CURRENCY`，并写入最终 `Config`。writer 的 `WRITABLE_KEYS` 加入 `price_currency`、`available_models`；`lib.rs` 导出新常量。

- [ ] **Step 4: 验证为绿**

Run: `cargo test -p ncx-config --target x86_64-pc-windows-gnu`

Expected: PASS。

- [ ] **Step 5: 提交**

```powershell
git add rust/crates/ncx-config/src/config.rs rust/crates/ncx-config/src/loader.rs rust/crates/ncx-config/src/writer.rs rust/crates/ncx-config/src/lib.rs
git commit -m "功能：支持模型费用币种配置"
```

## Task 2: 建立精选目录与 OpenRouter 解析器

**Files:**
- Create: `rust/gui/src-tauri/src/model_catalog.rs`
- Modify: `rust/gui/src-tauri/src/lib.rs`
- Modify: `rust/gui/src-tauri/Cargo.toml`

**Interfaces:**
- Produces: `CatalogModel`、`CatalogProvider`、`catalog()`、`find_preset()`、`parse_openrouter_models()`。
- Consumes: OpenRouter `data[].id`、`name`、`context_length`、`pricing.prompt`、`pricing.completion`。

- [ ] **Step 1: 写入失败测试**

```rust
#[test]
fn curated_presets_have_an_endpoint_price_and_source() {
    let providers = catalog();
    for id in ["deepseek", "bailian", "ark", "zhipu", "moonshot",
               "minimax", "openai", "gemini", "openrouter"] {
        let provider = providers.iter().find(|p| p.id == id).expect("missing provider");
        assert!(!provider.models.is_empty());
        for model in &provider.models {
            assert!(!model.model_id.is_empty());
            assert!(model.base_url.starts_with("https://"));
            assert!(matches!(model.price_currency.as_str(), "CNY" | "USD"));
            assert!(model.source_url.starts_with("https://"));
        }
    }
}

#[test]
fn openrouter_prices_are_converted_from_per_token_to_per_million_usd() {
    let models = parse_openrouter_models(r#"{
      "data": [{"id":"openai/gpt-test","name":"GPT Test","context_length":128000,
      "pricing":{"prompt":"0.00000125","completion":"0.00001"}}]
    }"#).unwrap();
    assert_eq!(models[0].model_id, "openai/gpt-test");
    assert_eq!(models[0].price_in, 1.25);
    assert_eq!(models[0].price_out, 10.0);
    assert_eq!(models[0].price_currency, "USD");
}

#[test]
fn openrouter_parser_rejects_missing_model_id_without_panicking() {
    assert!(parse_openrouter_models(r#"{"data":[{"name":"bad","pricing":{}}]}"#).is_err());
}
```

- [ ] **Step 2: 验证测试为红**

Run: `cargo test -p ncx-gui curated_presets_have_an_endpoint_price_and_source --target x86_64-pc-windows-gnu`

Expected: FAIL，找不到 `model_catalog` 模块或测试符号。

- [ ] **Step 3: 最小实现**

实现：

```rust
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CatalogModel {
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub base_url: String,
    pub price_in: f64,
    pub price_out: f64,
    pub price_currency: String,
    pub source_url: String,
    pub updated_at: String,
    pub context_length: Option<u64>,
    pub direct_available: bool,
}
```

`catalog()` 为 DeepSeek、百炼、方舟、智谱、Kimi、MiniMax、OpenAI、Gemini、OpenRouter 提供近期主力、经济、推理文本预设。每项填写官方模型 ID、官方兼容地址、来源 URL 与 `2026-08-17` 数据日期。百炼需要工作空间 ID 的地址标为需要用户填写，不能伪装成固定可调用地址。

`parse_openrouter_models` 过滤空 ID，解析失败价格存 `0.0`，将每 Token 美元价格乘 `1_000_000`，接口固定 `https://openrouter.ai/api/v1`。Cargo 增加：

```toml
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
```

- [ ] **Step 4: 验证为绿**

Run: `cargo test -p ncx-gui model_catalog --target x86_64-pc-windows-gnu`

Expected: PASS。

- [ ] **Step 5: 提交**

```powershell
git add rust/gui/src-tauri/Cargo.toml rust/gui/src-tauri/src/model_catalog.rs rust/gui/src-tauri/src/lib.rs
git commit -m "功能：加入厂商模型目录与 OpenRouter 解析"
```

## Task 3: 暴露目录命令并原子应用预设

**Files:**
- Modify: `rust/gui/src-tauri/src/lib.rs`
- Test: `rust/gui/src-tauri/src/lib.rs`

**Interfaces:**
- Produces: `get_model_catalog`、`refresh_openrouter_models`、`apply_model_preset`。
- Consumes: `CatalogModel`、`write_nanocodex_config`。

- [ ] **Step 1: 写入失败测试**

```rust
#[test]
fn preset_updates_model_endpoint_price_currency_and_quick_switch_list_together() {
    let preset = CatalogModel {
        provider_id: "openai".into(), model_id: "gpt-5-mini".into(),
        display_name: "GPT-5 mini".into(), base_url: "https://api.openai.com/v1".into(),
        price_in: 0.25, price_out: 2.0, price_currency: "USD".into(),
        source_url: "https://openai.com/api/pricing".into(), updated_at: "2026-08-17".into(),
        context_length: None, direct_available: true,
    };
    let updates = preset_updates(&preset, &["gpt-5-mini", "gpt-5"]);
    assert_eq!(updates["model"], "gpt-5-mini");
    assert_eq!(updates["base_url"], "https://api.openai.com/v1");
    assert_eq!(updates["price_currency"], "USD");
    assert_eq!(updates["available_models"], "gpt-5-mini,gpt-5");
}
```

- [ ] **Step 2: 验证测试为红**

Run: `cargo test -p ncx-gui preset_updates_model_endpoint_price_currency_and_quick_switch_list_together --target x86_64-pc-windows-gnu`

Expected: FAIL，找不到 `preset_updates`。

- [ ] **Step 3: 最小实现**

`AppState` 增加：

```rust
openrouter_models: Mutex<Vec<CatalogModel>>,
```

实现 `preset_updates`，始终成组写入 `model`、`base_url`、`price_in`、`price_out`、`price_currency`、`available_models`。预设命令仅接受 `direct_available == true` 项，单次 `write_nanocodex_config` 完成写入再发送 `Command::Reload`。

刷新命令以 `reqwest::get("https://openrouter.ai/api/v1/models")` 获取目录；成功时替换缓存；失败时缓存非空则返回缓存并标记 `stale = true`，缓存为空才返回中文错误。设置与状态 DTO 都新增 `price_currency`。将三个命令加入 `generate_handler!`。

- [ ] **Step 4: 验证为绿**

Run: `cargo test -p ncx-gui --target x86_64-pc-windows-gnu`

Expected: PASS。

- [ ] **Step 5: 提交**

```powershell
git add rust/gui/src-tauri/src/lib.rs
git commit -m "功能：模型预设自动配置接口与费用"
```

## Task 4: 改造设置页与费用显示

**Files:**
- Modify: `rust/gui/src/App.svelte`
- Modify: `rust/gui/src/app.css`
- Modify: `rust/gui/e2e/question.mjs`

**Interfaces:**
- Consumes: `get_model_catalog`、`refresh_openrouter_models`、`apply_model_preset`、`price_currency`。
- Produces: 厂商—模型联动、价格卡、OpenRouter 搜索/刷新、正确币种的会话费用。

- [ ] **Step 1: 写入失败页面回归**

```javascript
async function runModelCatalogTest(page) {
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await page.getByLabel("厂商").selectOption("openai");
  await page.getByLabel("模型").selectOption("gpt-5-mini");
  await page.getByText("输入 $0.25 / 百万 Token", { exact: true }).waitFor();
  await page.getByText("输出 $2.00 / 百万 Token", { exact: true }).waitFor();
  await page.getByRole("link", { name: "查看官方价格" }).waitFor();
}
```

在原问题测试前调用该函数。

- [ ] **Step 2: 验证页面回归为红**

Run: `npm.cmd run test:e2e:question`

Expected: FAIL，找不到“厂商”选择器或美元价格卡。若 Windows WebView2 未开放调试端口，记录环境限制，继续使用 Vite 构建与人工验证；不得把端口故障报告为断言通过。

- [ ] **Step 3: 最小实现**

扩展 TypeScript 设置和目录类型，打开设置时并行读取设置、位置和目录。添加：

```ts
const currencySymbol = (currency: string) => currency === "USD" ? "$" : "¥";
const formatModelPrice = (value: number, currency: string) =>
  `${currencySymbol(currency)}${value.toFixed(value >= 1 ? 2 : 4)} / 百万 Token`;
```

设置页先显示带 `aria-label="厂商"` 的选择器，再显示带 `aria-label="模型"` 的选择器。选模型调用 `apply_model_preset`，价格卡显示输入、输出、更新时间、官方链接。增加“手动覆盖价格”复选框；关闭时价格和币种控件禁用，开启后沿用 `save_settings` 保存。

选 OpenRouter 时显示刷新、搜索和本地筛选；刷新失败显示旧数据提示，不清空列表。顶部累计费用以 `currencySymbol(priceCurrency)` 显示，价格为零时只显示 Token。CSS 仅添加设置页目录、价格卡和小屏换行样式，不重排用户已有消息与输入框样式。

- [ ] **Step 4: 验证前端**

Run: `npm.cmd run build`

Expected: PASS。

Run: `npm.cmd run test:e2e:question`

Expected: PASS，包含新增断言。若调试端口仍受 WebView2 环境限制，启动 `npm.cmd run tauri dev`，人工选择 OpenAI / GPT-5 mini，确认美元价格、接口与链接；再选择 OpenRouter，确认刷新失败不会清空模型。

- [ ] **Step 5: 提交**

```powershell
git add rust/gui/src/App.svelte rust/gui/src/app.css rust/gui/e2e/question.mjs
git commit -m "功能：设置页按厂商选择模型并展示费用"
```

## Task 5: 更新示例并验证交付

**Files:**
- Modify: `config.example.toml`
- Verify: Rust workspace、GUI 前端与 Tauri 发布构建。

- [ ] **Step 1: 写入示例契约测试**

```rust
#[test]
fn documented_currency_key_is_writable() {
    assert!(WRITABLE_KEYS.contains(&"price_currency"));
    assert!(WRITABLE_KEYS.contains(&"available_models"));
}
```

- [ ] **Step 2: 验证示例契约**

Run: `cargo test -p ncx-config documented_currency_key_is_writable --target x86_64-pc-windows-gnu`

Expected: PASS。

- [ ] **Step 3: 更新示例配置**

在 `config.example.toml` 的价格区块加入：

```toml
# 费用估算使用的原始厂商币种：CNY 或 USD。
price_currency = "CNY"
# 模型快速切换列表；设置页选择厂商预设后会自动维护。
available_models = "deepseek-chat,deepseek-reasoner"
```

注明厂商预设会自动写入接口与价格，保留手动 `base_url`、模型和 API Key 说明。

- [ ] **Step 4: 全量验证**

Run:

```powershell
$mingwBin = 'C:\Users\25376\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin'
$env:Path = "$mingwBin;$env:Path"
cargo +1.96.0-x86_64-pc-windows-gnu test --workspace --target x86_64-pc-windows-gnu
```

Expected: PASS。

Run: `npm.cmd run build` from `rust/gui`

Expected: PASS。

Run: `pytest -q` from repository root

Expected: PASS。

Run:

```powershell
$env:RUSTUP_TOOLCHAIN = '1.96.0-x86_64-pc-windows-gnu'
$env:CARGO_TARGET_DIR = 'D:\github_dgy\nanocodex\rust\target'
npm.cmd run tauri:build
```

Expected: PASS，生成 `nanocodex_0.1.0_x64-setup.exe`。

- [ ] **Step 5: 提交**

```powershell
git add config.example.toml rust/crates/ncx-config/src/writer.rs
git commit -m "文档：补充模型费用与快速切换配置示例"
git status --short
git log --oneline -5
```

交付报告测试结果、OpenRouter 刷新结果、安装包绝对路径，以及无真实密钥不能验证的直连调用范围。
