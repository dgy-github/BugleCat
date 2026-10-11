# 代码产物校验评测操作契约

## 当前正式 V2 路径（2026-10-11）

新的正式迭代使用独立冻结的 mixed MBPP/HumanEval 来源、`artifact_protocol_20261011_v2_r2.json`、235 校准组及 240 留出组。模板版本是 `jev-artifact-assertion-specific-v2`：每条完整断言前缀对应一个违例概率，max 只生成拒绝信号；低概率保持 unverified。原每阶段至少150正/50负、单侧95%误拦上界2%及检出下界50%不变。旧失败校准、旧完整分配及开发组全部排斥，不迁移旧阈值。

- `artifact_v2_formal_cohort`：候选前冻结公开来源、解释器/worker、完整分组、资源配置、编译程序和共享授权；按原顺序选择尚未启动的组。
- `verification_artifact_v2_formal_collect`：两个 split 共用的实际主模型采集程序。留出必须携真实合格校准及固定共享消费锁；`prepare` 零收费，显式 `live` 每组一次，首故障停止。
- `artifact_v2_formal_intake`：重验终态批次、SQLite实际start/finish、原请求/响应/receipt、源码和解释器；独立受限worker执行原断言。未发送为unlabeled，已启动无效或越界为Unknown，二者不混淆。
- `artifact_v2_formal` / `artifact_v2_formal_observations`：概率前完整登记、实际Jev传输证明、固定网格校准及一次留出结果；所有预测收据重新核对原bytes，失败计入分母。
- `artifact_v2_formal_holdout`：新校准合格后，先写共享main ledger固定source消费marker，再允许新留出采集；共享Jev ledger另有固定evaluation marker，换输出路径不能重新消费。
- `verification_artifact_v2_formal`：实际Jev程序，`validate`零网络；`live`要求原packet、registration、同一原预约及显式live。缺原证据或配额时在HTTP前拒绝。

当前累计授权为每服务750次：保留400→500→750原链、所有失败及历史pending，历史27次Jev另累计。授权次数不等于账户余额。原收费源码和实际EXE在候选前封存；审计和中断恢复不能修改或重建原owner。新checkout的离线构建与fake transport测试不会恢复原本地收费证据，也不授予付费权限。

当前正式V2评测进行中，尚未认证阈值或打开留出。新`scripts/package-artifact-evaluation-v2.py`默认只读重验原最终结果；`--stage`才复制已审计v3.3运行程序、两原V2评测EXE及公开源，做零收费localhost生命周期验证并封存ZIP。合格校准但缺真实留出不能作为最终结果打包。公开包不含真实收费ledger、候选、会话或原provider响应；安装器提供但未安装。基准认证范围仅限冻结断言和运行环境，生产默认off，不自动认证整体agent收益。

## 历史 V1 操作与失败结果

以下保留此前MBPP单一来源、500次上限及四程序运行包的原操作记录；不覆盖上方当前V2协议、授权或认证状态。

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
- `verification_artifact_holdout_collect`：独立 holdout main。`check-gate <manifest>` 和 `prepare <manifest> <newdir>` 先重验实际合格校准、原始一次打开锁和未启动来源前缀；`live <prepared-dir> --live` 才发送。
- `verification_artifact_holdout_formal`：独立 holdout Jev。`check-gate <prepared-jsonl> <id> <registration>` 与 `check-registration` 先重验完整原标签配额及固定阈值，再允许显式 `live`；calibration 程序不接受伪造 split 的留出请求。

原始响应、候选、会话、收费 ledger 和已封存 cohort 留在本地。公开源码与测试可在新 checkout 运行；涉及真实收费证据的复核需原始本地文件，缺文件时不宣称复核成功。离线测试使用的成功数据只是门禁夹具。

## 离线验证与中断恢复

从仓库根目录构建四个 Rust example，之后运行相关实际程序测试。以下测试不访问真实模型接口：

```powershell
cargo build --manifest-path rust/Cargo.toml -p ncx-core --example verification_artifact_profile_collect --example verification_artifact_profile_formal --example verification_artifact_holdout_collect --example verification_artifact_holdout_formal
python -m pytest tests/test_verification_artifact_profile_binary.py tests/test_verification_artifact_profile_formal_binary.py tests/test_verification_artifact_holdout_binary.py tests/test_verification_artifact_holdout_transport_guard.py -q
```

需要真实本地收费证据的测试在缺少原文件时显式 skip；fake transport 回归仍运行。新 checkout 不携带原预算授权的实际 ledger，不能用测试夹具申请正式认证。

收费中断后先复核原 SQLite start/finish、reservation、事件和收据。已启动或已预约组不再发送；原收据缺失时保留 unresolved，并暂停审计。已完成的概率观察逐条保存，恢复只从原 bytes 重建。只有证明尚未启动的后续组才能按原顺序建立新批次，不能把失败组换名重发。

## 结果字段的范围

- `artifact_benchmark_certified` / `certified_artifact_threshold`：只在真实校准及一次真实留出均通过冻结门槛时，限定冻结 MBPP task groups、main 资源配置、断言 Oracle、Jev 模型及模板。
- `certified_threshold`、`native_claim_certification`、`overall_agent_decision_benefit_certified`：本产物轨道不能据此认证这些更宽范围。
- Jev 只产生 reject signal 或维持 unverified；低违例概率不产生 Verified/Pass。
- 生产默认 off，enforce 类别为空。基准阈值认证不会自动批准生产启用。
- 原框架 A / 规则 B 没有对应真实执行基线时记 unavailable，不能伪造全放行基线来证明增量收益。

## 2026-10-11 本轮实际结果与交付

独立主模型232组中162 supported、53 unsupported、17 Unknown；预测前登记的215二元组Jev全部完成、无重试。九个冻结阈值全部不合格，qualified threshold为空，未打开留出，生产保持off。

阈值0.5误拦6/162、检出29/53；95%误拦上界7.1788%、错误检测下界42.5531%，均不满足2%/50%门槛。阈值0.9误拦0，但只检出10/53，错误检测下界10.6097%仍未通过。没有根据本轮结果下调门槛或制造负例。

公开原样结果：`reports/artifact_calibration_high_result_20261011.json`（本目录下）；交付清单：`reports/artifact_delivery_20261011.json`。新包 `releases/jev-artifact-calibration-20261011`，ZIP `releases/BugleCat-jev-artifact-20261011.zip`，30.24 MiB。GUI/CLI和4个评测程序均实际复验，installer未安装，原始证据/密钥留本地。公开JSON副本不携带原证据，在新checkout不能据此宣称重新认证。

调用215次成功返回不代表代码全部正确或框架收益已认证；完整置信界、tokens及限定原始请求延迟范围见交付清单。后续修改需要新冻结来源和独立评测，本轮没有自动扩预算或继续收费。
