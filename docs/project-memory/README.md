# 项目级代码与接口记忆

这里是开发前的第一查询入口。目的不是代替源码，而是让开发者和 AI 先知道项目已有
什么能力、模块负责什么、公共入口在哪里，再只打开少量相关源码确认实现。

## 查询顺序

1. [CAPABILITY_MAP.md](CAPABILITY_MAP.md)：先按业务能力查责任模块和复用边界。
2. [MODULE_CATALOG.md](MODULE_CATALOG.md)：查看每个源文件的职责和主要入口。
3. [INTERFACE_CATALOG.md](INTERFACE_CATALOG.md)：查工具、Provider、配置、CLI、沙箱、
   请求响应类型等可复用接口。
4. [SYMBOL_INDEX.md](SYMBOL_INDEX.md)：只有前三层不够时，再查具体类和方法。
5. 最后才打开命中的源码和附近测试确认行为。

因此，新功能不需要重新遍历整个项目。正常流程应当是：

```text
需求关键词
  -> 能力地图
  -> 模块/接口候选
  -> 打开 1~3 个命中源码和测试
  -> 复用或扩展现有责任边界
```

## 各文件职责

| 文件 | 类型 | 维护方式 |
| --- | --- | --- |
| `CAPABILITY_MAP.md` | 人工语义摘要 | 职责或复用规则变化时更新 |
| `MODULE_CATALOG.md` | 全代码模块说明 | 自动生成，模块 doc comment 提供摘要 |
| `INTERFACE_CATALOG.md` | 公共接口候选目录 | 自动生成，接口 doc comment 提供契约摘要 |
| `SYMBOL_INDEX.md` | 所有符号导航 | 自动生成 |
| `SYMBOL_INDEX.json` | 机器可读记忆 | 自动生成 |

## 更新规则

新增、删除、移动或重命名模块、类、方法、工具、命令、配置、请求响应类型后运行：

```powershell
python scripts/generate_project_memory.py
python scripts/generate_project_memory.py --check
```

如果模块职责改变，还必须更新 `CAPABILITY_MAP.md`。关键公共接口应增加 doc comment，
说明用途、输入、输出、错误、副作用和复用约束，使自动目录能够直接展示有效说明。
