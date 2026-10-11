# 代码产物校验评测操作契约

本轨道判断主模型生成的 Python 代码是否满足冻结 MBPP 断言。它不证明主模型执行了测试，也不认证自然语言声明、任意项目正确性或整体 agent 决策收益。统计门槛以请求前冻结的 `reports/artifact_protocol_20261011_v1.json` 为准。

## 阶段与前置证据

1. **来源登记**：公开源、原始文件 hash、固定任务组与 split、Oracle 源、共享授权链和已启动组排斥在新主模型请求前冻结。原始费用和失败保留；同一组不再请求。
2. **主模型采集**：使用与登记一致的实际编译程序、资源配置和提示词。只有 task prompt 和公开函数接口进入主模型；参考实现与隐藏断言不进入请求。每批最多 8 组，取消或连续同类故障停止后续批。
3. **独立标注**：从原始 main response 提取唯一代码产物，用独立冻结断言作 supported / unsupported / unknown 标注。超过 Oracle 支持边界、缺失响应及传输失败保留为 unknown，不制造负例。疑难项复核不能替换原始候选或把 unknown 强行改为二元标签。
4. **预测前登记**：独立标签至少 150 supported、50 unsupported。登记全部实际启动组、原始候选、Oracle 和 Jev 请求的 raw / canonical hash；此时没有读取 Jev 概率。达不到配额时正式校准不启动。
5. **校准**：Jev 每个已登记二元组最多调用一次；失败仍入相应分母，概率保持空。按冻结网格选择阈值，单侧 95% 误拒绝率上界 ≤2%，错误拒绝率下界 ≥50%。输出 `qualified_for_holdout_threshold` 只允许开启新留出。
6. **一次留出**：重算原始校准证据和统计后，登记新源集合，排斥整个校准 allocation 与此前启动/预留组，固定阈值。共享 ledger 下的打开、标签登记、结果消费 marker 均只创建一次；失败不重开，不调参。留出同样需要独立 150 正/50 负。
7. **交付**：从原始收据重算最终结果，封存程序、源和评测证据 hash。报告未通过的门槛和未知样本，不以离线夹具、程序测试通过或运行包存在代替真实认证。

## 资源配置与版本隔离

旧 ultra/8192/60秒采集出现推理用完输出额度、最终 content 为空的失败。新配置为独立的 `deepseek-flash-high-16384-120-v1`，0 重试，模型提示与 Oracle 保持不变。新旧配置的样本不合并成同一正式分母；旧已启动组及失败继续计入授权上限。实际程序使用临时配置覆盖，不修改用户全局模型设置。

预算是请求数量授权，不是账户余额。当前阶段上限主模型/Jev各500次，包含此前400次及本次追加100次，旧失败和暂停遗留仍计数；金额与余额没有据此推算。费用操作仅由根操作员执行，开发与测试使用 fake transport / localhost。密钥通过既有本地配置或 `NCX_JEV_API_KEY` 提供，命令、公开报告和 Git 提交中不写密钥。

## 程序与复核入口

- `verification_artifact_profile_collect`：新 high 配置的 calibration main；`prepare` 不收费，`live ... --live` 才执行实际采集。
- `verification_artifact_profile_formal`：新配置的 calibration Jev。`check-source`、`check-registration` 和 `validate` 不调用网络；缺标签配额、原始 source join、授权或显式 live flag 时拒绝发送。
- `artifact_formal_intake`：独立标注及原始 SQLite start/finish、请求、响应、编译源/程序、Oracle 复核。
- `artifact_formal`：预测前标签登记、原始 Jev transport 重验和固定网格校准。
- `artifact_holdout`：重验校准、一次新来源登记、冻结标签、消费固定阈值结果；`revalidate_evaluation` 重新从原始证据验证结果。
- 独立 holdout main/Jev 程序须在发送前通过实际合格校准和原始 once-only lock；calibration 程序不接受伪造 split 的留出请求。

原始响应、候选、会话、收费 ledger 和已封存 cohort 留在本地。公开源码与测试可在新 checkout 运行；涉及真实收费证据的复核需原始本地文件，缺文件时不宣称复核成功。离线测试使用的成功数据只是门禁夹具。

## 结果字段的范围

- `artifact_benchmark_certified` / `certified_artifact_threshold`：只在真实校准及一次真实留出均通过冻结门槛时，限定冻结 MBPP task groups、main 资源配置、断言 Oracle、Jev 模型及模板。
- `certified_threshold`、`native_claim_certification`、`overall_agent_decision_benefit_certified`：本产物轨道不能据此认证这些更宽范围。
- Jev 只产生 reject signal 或维持 unverified；低违例概率不产生 Verified/Pass。
- 生产默认 off，enforce 类别为空。基准阈值认证不会自动批准生产启用。
- 原框架 A / 规则 B 没有对应真实执行基线时记 unavailable，不能伪造全放行基线来证明增量收益。
