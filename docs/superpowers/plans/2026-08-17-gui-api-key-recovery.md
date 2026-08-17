# GUI 缺少 API Key 的恢复实现计划

> **供 Agent 执行：** 逐任务执行并在每项完成后复核。

**目标：** 缺少 API Key 的首次启动保持 GUI 可恢复；保存有效配置后无需重启即可恢复 Agent。

**架构：** Bridge worker 在初始化失败后继续保留命令循环与错误状态；前端以 `ready` 事件作为运行时操作可用的条件，并在失败时引导用户进入设置。

**技术栈：** Rust、Tauri 2、Svelte 5、Cargo、Vite。

## 全局约束

- 所有新增用户文案使用中文。
- 不暴露 API Key。
- 保持 `x86_64-pc-windows-gnu` 构建。
- 不新增凭据存储或完整引导页。

---

### 任务 1：保持失败初始化后的 Bridge worker 可恢复

**文件：** 修改 `rust/gui/src-tauri/src/bridge.rs`；测试同文件的 Rust 测试模块。

**接口：** `build_agent(...) -> Result<... , String>`；新增纯函数 `unavailable_agent_error(&str) -> String`。

- [ ] 写失败测试：

```rust
#[test]
fn unavailable_agent_reports_configuration_error() {
    assert_eq!(
        unavailable_agent_error("No API key found."),
        "Agent 尚未配置：No API key found."
    );
}
```

- [ ] 运行测试并确认失败：

```powershell
cargo +1.96.0-x86_64-pc-windows-gnu test -p ncx-gui unavailable_agent_reports_configuration_error --target x86_64-pc-windows-gnu
```

- [ ] 最小实现：初始 `build_agent` 失败时不再 `return`；保留 `Option<AgentLoop>` 和最后错误。`Reload` 成功时写入 Agent 并清空错误，失败时更新错误；其它依赖 Agent 的命令发送 `Agent 尚未配置：<实际错误>`。

- [ ] 运行 GUI Rust 测试：

```powershell
cargo +1.96.0-x86_64-pc-windows-gnu test -p ncx-gui --target x86_64-pc-windows-gnu
```

### 任务 2：把未就绪状态显示成配置提示

**文件：** 修改 `rust/gui/src/App.svelte`；测试 `rust/gui/e2e/question.mjs`。

**接口：** 新增 `agentReady: boolean`、`agentInitError: string`；消费 `ready` 与 `error` 事件。

- [ ] 写失败测试：无 `ready` 事件时点击权限模式，断言没有调用 `invoke("set_permission_mode", ...)`。

- [ ] 运行测试并确认失败：

```powershell
npm.cmd run test:e2e:question
```

- [ ] 最小实现：`ready` 时将 `agentReady` 设为 true 并清空错误；初始化 `error` 时设为 false 并保存错误。未 ready 时 `selectMode` 不调用后端，显示“请先在设置中配置模型 API Key”；在权限区域显示同一中文提示与已有设置入口。

- [ ] 验证前端：

```powershell
npm.cmd run test:e2e:question
npm.cmd run build
```

### 任务 3：验证保存配置后的恢复与桌面产物

**文件：** 只修改任务 1、2 验证所发现的必要缺口。

- [ ] 手动验证：无 API Key 启动时提示配置、权限切换不报线程错误；在设置保存有效配置后收到 `ready` 并重新启用控件。

- [ ] 完整回归：

```powershell
python -m pytest -q
cargo +1.96.0-x86_64-pc-windows-gnu test --workspace --target x86_64-pc-windows-gnu
```

- [ ] 构建桌面端：

```powershell
npm.cmd run tauri:build
```

