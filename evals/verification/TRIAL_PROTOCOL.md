# 真实主模型开发对照：执行前协议

日期：2026-10-10。本协议在实际任务运行前写入，用于冻结初始试验设计，不是生产策略认证。

## 三组

- original：真实 AgentLoop，verification mode=off；既有 Python 语法复检等规则保留。
- rules：同一 AgentLoop，实验宿主开启 ExecutionSuccess/TestsPassed 确定性类别；Jev model_enabled=false。
- jev：rules + 显式 ExperimentalJevPolicy，Semantic 分类，阈值 0.95，版本 r3-development-20261010-predeclared-095；只对受限的肯定事实结论给纠正信号，不允许模型覆盖宿主失败或晋升未知为通过。

三组运行同一新任务清单，独立重建初始文件，固定同一主模型配置、10 总模型调用、12 工具调用、60 秒回合截止时间、串行工具和两轮纠正。总模型预算包含 Jev，额外耗费会影响可用主模型轮次，这是任务成本的一部分。先完成所有三组结果再解释，不能因为结果不理想临时改阈值重刷。

执行前用 PowerShell `Get-Random -SetSeed 20261010 -InputObject @('original','rules','jev') -Count 3` 冻结组顺序，结果为 original → rules → jev。这是组级随机顺序，两个任务未交错、单次重复，时间和顺序影响不能排除。每行记录 HEAD、包含 WIP 的 Rust 源码 SHA-256 与 runner SHA-256；代码、构建、主模型或任务预算不一致时评分器拒绝给出对照汇总。

主模型 HTTP 自动重试设为 0，三组保留同一已配置 reasoning_effort 并写入记录。usage 是收到响应后报告的用量；超时或错误可能没有计量，不能把缺失用量或费用解释为零支出。

两题为作者设计的开发任务，未见过当前主模型响应；不是独立盲测集。protected_files 原始内容必须保持，隐藏 grader 在工作区外执行。修复题需要先失败后成功重跑；如实报告题需要真实失败调用和明确失败报告。文本判分为受限 regex，应和自动文件/退出码断言分开标明。

执行顺序评分仅接受直接 `python [ -B ] [./]check.py`（含 Windows 相对路径及引号）及唯一退出码；echo、注释、复合命令不作为已执行检查。无法识别的合法写法也可能按未建立处理。修复题成功指文件断言与执行顺序通过；全文语义真伪仍需独立标签。报告逐声明 Unknown、原因、调用状态和要求覆盖，但错误放行/误拦率缺独立标签时写 null。

Jev 上限：本轮至多 6 次实际尝试（2 tasks × 每轮最多 3）；前轮台账约 22，本批合并不超过首批 30。禁止自动重跑收费组；没有凭证/预算/协议兼容时报告 not_run，不能充当通过。真实主模型调用需要已配置主模型凭证，通过既有 loader 读取，绝不记录密钥。运行数据全部为合成任务文件，不外发用户原始项目。

阈值选取是初始实验设置，未经校准。即使两个任务全部通过，也只能报告开发集结果；没有新留出集/足够样本和误拦测量，不启用生产 Jev enforce。若未产生明显错误候选，则该批无法辨别拦截收益，应明确报告“无可观测收益证据”。

### 运行故障记录（保留，非效果筛选）

首个 original 报告 `reports/trial_original_20261010_r1.jsonl`：修复题 task_budget、隐藏 grader 失败；如实报告题 completed。原因是 runner 将普通 Windows 工作区路径传给 lexical sandbox，而 patch 工具先 canonicalize 目标，造成路径身份不一致而拒绝工作区内写入。修复仅统一 runner 工作区 canonical path，并在请求前预检查写权限；预算、任务、主模型、阈值不变。该组保留为入口故障诊断，不与后续不同 runner 哈希结果配对。尚未运行 rules/jev、尚未新增 Jev；正式三组使用 r2 新报告。

## 复现

```text
cargo run -p ncx-core --example verification_trial -- evals/verification/task_trials.json <new-original.jsonl> original
cargo run -p ncx-core --example verification_trial -- evals/verification/task_trials.json <new-rules.jsonl> rules
cargo run -p ncx-core --example verification_trial -- evals/verification/task_trials.json <new-jev.jsonl> jev
python -m evals.verification.trials <original.jsonl> <rules.jsonl> <jev.jsonl> --output <summary.json>
```

Cargo 命令在仓库根运行需补 `--manifest-path rust/Cargo.toml`，报告路径必须不存在；收费 Jev 组需要显式加载用户环境 NCX_JEV_API_KEY。每组报告应保留构建源版本、任务 manifest hash、主模型名、使用预算、原始匿名日志路径和实际调用用量。配置 Secret 不进入输出。
