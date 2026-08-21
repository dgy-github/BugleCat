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
