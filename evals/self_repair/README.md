# BugleCat 自我修复评测子包

## 六类源码自修复题库

本子包在真实 BugleCat 源码上注入故障，再让 BugleCat 自己提交候选修改（不自动 Git 提交）。
覆盖失败分类/重试、补丁上下文/缩进、预算/退出码、编译门禁、路径/编码、诊断/脱敏六类。
题目定义在 `cases.py`；执行与独立验收分离，既有测试冻结，健康代码须通过、故障种子须测试失败。

[中文自我测试报告](../../docs/reports/self_repair_20260908.md)

[最新：2026-09-08 下午复跑报告](../../docs/reports/self_repair_20260908_081319.md)：首轮 6/6，独立重放全部通过，40 项验收检查通过；[结构化结果与候选补丁](results/20260908_081319/summary.json)。候选补丁作为故障注入评测证据收录，不用于覆盖主项目健康源码。早前报告与失败轨迹保留。

在项目根目录运行，需要 Python 3.11+、Rust 工具链、已缓存的 Cargo 依赖、
`cargo build --manifest-path rust/Cargo.toml --release -p ncx-cli` 构建的执行器，以及本机已配置的
`preset:deepseek` / `deepseek-v4-pro`。当前运行入口针对 Windows `ncx.exe`。

```powershell
python -m evals.self_repair.suite --work-root D:/BugleCat-selftest-new
# 离线验证题目，不调用模型
python -m evals.self_repair.suite --work-root D:/BugleCat-seedcheck-new --validate-only
# 精确选择题目
python -m evals.self_repair.suite --work-root D:/BugleCat-patch-new --cases s2_patch
# 发布可读报告（替换 RUN_ID）
python -m evals.self_repair.recheck evals/self_repair/results/RUN_ID/summary.json D:/BugleCat-selftest-new
python -m evals.self_repair.suite_report evals/self_repair/results/RUN_ID/summary.json docs/reports/self_repair_RUN_ID.md
```

目录必须是仓库之外的新目录，不覆盖旧副本。单题每次 600 秒 Agent 预算，最多一次反馈；
外层 630 秒回收自有子进程树，独立验收限时 600 秒。逐次保存补丁、退出码、耗时、
脱敏 Provider 元数据与测试结果；首轮通过、反馈后通过、正常完成及基础设施异常分别统计。
不会设置定时任务，也不会自动把合成故障候选合并进主仓库。

每个候选包含冻结的 `self_repair_check.py`：只封装可见测试命令和允许的编译环境变量，
不含参考解或 API 凭据。先在模拟 Agent 精简环境下验证健康测试可运行，避免宿主 Cargo
正常、Agent shell 却找不到 MSVC 链接器的环境偏差。模型应使用这一入口，无需搜索或安装工具链。
首次环境不一致的试运行见 `results/20260908_001556/PILOT.md`，不混入正式六类统计。

验收使用共享 Cargo 依赖缓存，但每次外部验收先通过 `cargo clean --profile dev -p ...`
失效化四个可变包的开发构建产物，避免跨副本时间戳导致旧代码被复用。源码、注册表依赖缓存
和 release 执行器保留；可重新构建恢复。所有 Cargo 操作必须串行，不要同时运行其他构建。
`recheck` 会从单提交故障种子复验健康/故障/每次补丁，报告生成器拒绝缺少复验或结果不一致的证据。
若仅健康预检失败且没有任何模型调用，可用 `retry_invalid` 补跑该题，原预检保存在 `preflight_history`；
不能用这个入口覆盖模型已经失败的尝试。

这是合作式开发测试，不是隔离恶意代码的安全沙箱：候选副本不导出健康 Git 历史、
历史答案或本套验收器，但 workspace-write 的读取权限不限制宿主其他目录，且使用共享 Cargo 缓存。
不要向不可信模型/候选开放敏感主机。测试完整性检查冻结文件与原有测试身份，不能证明抵御任意作弊。

### 历史真实缺陷修复已集成

下文第二轮挑战的最终候选 `pro_candidate_r2_final.patch` 经 9/9 独立验收后，
已在本次交付中合入 `bench/run.py` 与 `tests/test_bench_run.py`。历史章节中的“未合入/未提交”
描述当时状态，不表示本次交付仍未集成。历史失败和反馈前补丁保留用于追溯，不是推荐应用的补丁。

## 历史单题挑战：benchmark 空响应重试

## 可运行入口

在项目根目录显式执行（会调用已配置的真实 DeepSeek 服务）：

```powershell
python evals/self_repair/probe_deepseek.py
python evals/self_repair/run_challenge.py D:/github_dgy/BugleCat-self-repair-20260908
python evals/self_repair/acceptance.py D:/github_dgy/BugleCat-self-repair-20260908
```

`probe_deepseek.py` 对已配置的 Pro 与实验模型分别做短问答和工具调用，单探针总上限 60s，
不自动重试。验收脚本只使用离线替身，独立于修复 agent，检查真实分类/调度逻辑。
挑战运行器保留二进制 SHA-256、源码基线、耗时、退出码和脱敏 Provider 诊断；不保存凭据。

## 连接诊断与时间预算

- OpenAI 兼容 Provider 对流式、非流式请求统一输出 `[ncx-provider]` JSON：请求模型、
  attempts/retries、HTTP 状态（尚无响应时为 null）、错误类型、elapsed_ms。
- 错误类型包括 connect_error、connect_timeout、headers_timeout、read_timeout、http_error、
  stream_error、decode_error、request_deadline。原始 URL、请求头和错误正文不进入诊断。
- `timeout_s` 现在约束一次模型调用的全部 HTTP 尝试与退避等待，默认 120s；
  配置的 max_retries 仍生效，默认最多重试 3 次。带 Provider 诊断的最终错误不再被 Agent
  外层重复重试。连接等待最多 15s，已有 streaming headers/idle 限制继续生效。
- `NANOCODEX_TURN_TIMEOUT_S` 默认 600s，可设置为 1–3600s；模型等待和工具调度通过既有
  取消机制收尾，保存轨迹。同步代码和停止 Hook 的清理时间可能使实际返回略晚于期限。
- CLI 将 timeout 返回非零退出码，并保存为 Failed；全局模型路线保持不变。

2026-09-08 短探针对比四项全部 HTTP 200：Pro 短请求 1.97s / 工具 2.32s；实验模型短请求
1.60s / 工具 2.06s。这只能说明低上下文非流式请求当前可用，不证明长上下文流式任务稳定。

本挑战由本机 BugleCat CLI 实际执行，外部验收器独立检查结果。挑战环境为隔离仓库副本；修复不会自动合并或推送。

## 题目

修复自身 `bench/run.py` 的重试可靠性问题，要求：

- 失败输出含 `empty response` 或 `空内容`，即使目标文件缺失，也应触发空响应重试。
- Python 编译失败优先分类为 `compile_failed`。
- 成功输出只是提到空响应时，不得触发重试。
- 重试不得超过 `empty_response_retries`；0 表示禁用。
- `attempts` 保留每次实际调用的证据，以及逻辑轮次和重试序号。
- `runs/passes` 按逻辑轮次统计；`mean_s` 包含该轮全部调用耗时；timeout 统计包含所有实际调用。
- 保持 `run_once()` 三元组兼容接口。
- 添加离线、确定性的回归测试；运行测试和 lint。

允许修改 `bench/run.py` 并新增测试。禁止修改任务题目、hidden checker，或削弱已有测试。测试不得调用真实模型；修复 agent 不得自行提交或推送。

## 2026-09-08 首轮执行结果（历史记录）

- 源码基线：`805261ec4fd8ae961fd1bec2589dc95295043433`。
- 执行器：本机已有 `rust/target/release/ncx.exe`（构建时间 2026-09-04；不是最新源码重建产物）。
- 执行器 SHA-256：`B61DB4A2EDD6DB07D17C10464354987B9FA11BDC4AC4981F1D235D801CFB8AF4`。
- 隔离副本：`D:/github_dgy/BugleCat-self-repair-20260908`。
- 外部验收脚本：`D:/github_dgy/self_repair_acceptance_20260908.py`，未放入 agent 工作区。
- 原版外部验收：9 项中 3 项通过、6 项失败。失败涵盖分类优先级、成功误重试、重试恢复和完整耗时/证据统计。
- 两次实际 CLI 启动均返回 `HTTP 502`、退出码 1；CLI 报告的 prompt/completion token 均为 0（不能据此推断上游账单）。
- 两次启动后的隔离副本 Git 工作树均无代码改动；没有可供验收的修复补丁。

结论：**本次挑战被模型服务错误阻塞，未完成自修复**。原版验收结果证明题目存在真实缺陷；本次服务错误不能用来判断 BugleCat 是否具备修复该缺陷的能力。

服务恢复后，应使用同一基线、题目和外部验收脚本重跑，并分别记录服务成功率和补丁验收通过率。

## 切换 DeepSeek 后的追加执行

- 使用已保存的 DeepSeek 线路：`https://api.deepseek.com`，模型 `deepseek-v4-flash-vision-exp`。
- 地址、协议、凭据和模型仅覆盖本次 CLI 子进程环境，全局默认线路未修改。
- 本次成功进入搜索和读取代码的工具调用阶段，随后 CLI 报告连接模型服务失败、已自动重试多次，最终退出码 1。
- CLI 报告 usage：prompt 75457、completion 6806、total 82263；这不是费用或上游账单证明。
- 隔离副本仍无代码修改，未产生可验收补丁。记录属于模型调用中断，不能计为自修复成功。

## 新执行器 + DeepSeek Pro：最终独立验收

- 本轮使用重新构建的 release CLI；二进制哈希与完整诊断见
  `challenge_20260908_020133.json`，不再使用上述首轮旧执行器。
- 耗时 313.76 秒，进程退出码 0；30 次模型调用均 HTTP 200、无 HTTP 重试。
  这说明本轮连接成功，不代表修复成功，也不是持续可用性保证。
- BugleCat 仅修改 `bench/run.py`，没有新增要求的离线测试；失败补丁保留在
  `pro_candidate_failed.patch`，未合入主项目。
- 独立执行 `python evals/self_repair/acceptance.py D:/github_dgy/BugleCat-self-repair-20260908`
  在加载候选模块时失败：第 1 行 `logical_attempts = [evidence]` 被错误插入文件开头，
  报 `IndentationError: unexpected indent`。因此 9 项行为验收未能开始，不能记作通过。
- 结论：连接阻塞已在本轮解除，自修复挑战仍失败。下一步需调查补丁定位和结束门禁，
  尤其是语法未通过或达到迭代上限时不得把进程正常退出当作任务完成。

本轮运行时改动验证：CLI 37、core 283、provider 51 个 Rust 测试通过；相关三 crate 的
Clippy（all-targets / -D warnings）、Rust fmt、评测脚本 Ruff 通过。未重跑全量 benchmark，
未重新验证 GUI 构建，未提交或推送本轮改动。

## 编辑可靠性修复与第二轮挑战

本轮修复针对上一轮的已确认根因：

- V4A 普通上下文参与整段匹配，不再丢弃插入定位；空白容错匹配保留原文件上下文的真实缩进，不把上下文当作待修改代码。
- CLI 只有 `completed` 才退出 0、保存 Completed；`task_budget`、`compile_failed`、`verification_failed` 等均失败退出。
- 原生 `apply_patch` 和复用它的 `str_replace_editor` 编辑 Python 后，自动执行隔离的 Python 语法编译。使用 `-I -S` 和 `compile(..., 'exec')`，不执行目标模块，也不产生缓存文件。
- 编译诊断记录路径、状态和错误类型/行号，不包含源码。编译器不可用、读取失败、15 秒检查超时均视为验证失败，不能直接完成。
- 多文件编辑在检查前全部登记为未验证；结束前重新检查已登记文件并保存最新证据。失败会让模型继续修复；预算/超时结束保留验证失败状态与工具轨迹。
- 边界：这不是测试通过门禁，语法通过不等于功能正确；当前自动登记范围是原生编辑工具，尚不自动发现任意 shell/MCP 新建的 Python 文件。已登记文件若被其他工具改坏，结束前复检仍会拦截。

修复回归：425 个 Rust 测试通过（CLI 38、core 289、provider 51、tools 47），严格 Clippy、Rust fmt、Ruff 与原有 eval harness 的 3 个 Python 测试通过。回归测试实际复现了旧版插入错位、重复行误改、容错匹配改变缩进、取消漏登记、复检证据丢失及完成门禁失效。

第二轮使用干净副本 `D:/github_dgy/BugleCat-self-repair-20260908-r2`，源码基线保持不变；使用最新构建执行器，不携带上一轮失败补丁。挑战前独立验收仍为 3/9 通过。

### 第二轮最终结果：一次反馈后通过

- 首次运行 413.78 秒、26 次模型请求均 HTTP 200，原生编辑后的自动编译全部通过；独立验收 **8/9**。失败原因是候选额外要求“目标文件缺失”才重试，而空响应在文件存在时同样应重试。其自测通过并不代表满足全部验收要求。
- 将这一反例交还 BugleCat，在同一候选副本上追加一次纠错任务（不是从干净基线独立重试）。反馈文本保存在 `feedback_r2.txt`；没有向模型提供独立验收脚本，也没有由外部助手代写候选代码。
- 纠错运行 43.18 秒、7 次请求均 HTTP 200。最终独立验收 **9/9**；外部再次执行完整 Python 测试 **616 passed in 12.03s**，Ruff、py_compile 和 diff check 通过。
- 外部额外检查了多逻辑轮次平均耗时、物理 timeout 累加、attempt/retry 序号、原始错误保留，以及 passed=True 时即使 failure_kind 字段异常也不重试，均通过。
- 两轮执行器 SHA-256 相同：`321c8cb2a5a699da3bba2db71a3416c08454ce3fa770dfb1e9af309e206ceda1`。仅覆盖这道离线自修复题，不代表全量 benchmark 已重新通过。
- 候选只修改 `bench/run.py` 并新增 `tests/test_bench_run.py`（12 个离线测试）；未改 baseline 测试或题目/checker，未提交、推送或自动合并候选代码。

证据：

- `challenge_20260908_022224.json`：首次运行诊断。
- `acceptance_r2_before_feedback.json` / `pro_candidate_r2_before_feedback.patch`：首次验收与候选快照。
- `challenge_20260908_022331.json`：追加纠错诊断。
- `acceptance_r2_final.json` / `pro_candidate_r2_final.patch`：最终独立验收与完整候选补丁。

如需复验现有候选，运行开头列出的 acceptance 命令并将路径替换为 `BugleCat-self-repair-20260908-r2`；不要把旧副本的失败补丁混入第二轮。
