# Harness 组合配置

nanocodex 使用 TOML Profile、Bundle 和 Overlay 装配插件。插件激活顺序由服务依赖决定，文件中的行顺序只决定稳定的工具展示顺序。

## 目录

```text
harness/
├── profiles/<name>.toml
└── bundles/<id>.toml
```

Profile 按顺序叠加 Bundle：

```toml
name = "full"
bundles = ["base", "search", "workspace", "process", "session"]
```

Bundle 使用稳定 entry ID 引用编译进程序的 Rust 插件：

```toml
id = "search"

[[plugins]]
id = "search-tools"
plugin = "ncx.search"
enabled = true

[plugins.config]
provider = "default"
```

Overlay 按 entry ID 覆盖插件、启用状态或完整配置。未知 entry ID 会导致启动失败：

```toml
[[plugins]]
id = "process-tools"
enabled = false
```

## 选择规则

- 默认 Profile：`full`。
- `NANOCODEX_HARNESS_PROFILE`：选择 Profile 名称。
- `NANOCODEX_HARNESS_ROOT`：使用外部 `profiles/`、`bundles/` 根目录。
- 工作区存在 `.ncx/harness/profiles/` 时，自动把 `.ncx/harness` 作为外部根目录。
- 工作区 `.ncx/harness.overlay.toml` 自动作为 Overlay。
- `NANOCODEX_HARNESS_OVERLAYS` 可按系统路径分隔符追加多个 Overlay；后面的 Overlay 优先。

外部配置只能选择已编译进当前版本的插件。动态下载和加载不可信插件不属于 M1。

## M5 能力服务

`full` Profile 额外挂载 `media` Bundle，其中包含：

- `ncx.mcp`：MCP 启用状态、服务器数和活动工具数；实际连接仍经过现有 `--mcp` 启动门。
- `ncx.attachment`：附件扩展名与大小限制；CLI/GUI 在读取文件前消费该服务。
- `ncx.media`：视觉理解、生图和视频能力开关；实际供应商继续由现有 Provider/Skills/视频任务实现。
- `ncx.cost-telemetry`：币种、Token 单价、费用估算和遥测开关；运行时用当前模型配置替换默认零价格。

`minimal` 和 `headless` 不挂载媒体 Bundle。三种 Profile 有独立组合测试，避免能力在会话间串用。

## M6 进程外插件

工作区插件安装到 `.ncx/plugins/<plugin-id>`。设置页支持发现、安装、升级、启用和停用。插件目录必须包含：

```toml
id = "example.echo"
name = "Echo"
version = "1.0.0"
protocol = 1
command = "echo-plugin.exe"
args = []
capabilities = ["tool"]
```

外部插件只允许目录内相对可执行命令，以清空后的环境变量、管道 stdin/stdout/stderr 在独立进程运行。路径穿越、符号链接和 `.dll`/`.so`/`.dylib` 原生动态库会被拒绝。升级先写入 staging，再切换目录，失败时恢复旧版本。
