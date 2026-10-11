# APPS 正式评测下一步：完整量程校准与一次留出

## 已完成的真实开发检查

固定 8 个开发组已完成主模型采集和独立本地断言：6 supported、1 unsupported、1 Unknown。Unknown 为受限策略不支持的隐式 builtin，保留原记录。Jev 对 7 个可判定组各尝试一次，6 成功、最后 1 个 transport_error，无重试。五个成功正例的分数为 0.23–0.29，负例分数为 0.47；另一个正例因传输失败没有概率。原 0.5 起点网格未检出该负例。此规模不能认证准确性。

Root 已只读重建完整 allocation，重关联原候选/Oracle、实际请求字节、共享预约/一次 start marker、events、response、model、usage 与实际编译来源；没有重新执行 Oracle。公开聚合：`reports/artifact-v3-apps-development-jev-20261011.json`。全部 8 个开发组排除未来正式集。

旧 protocol、所有已付费 owner/EXE 和原始证据继续保留原字节。新的 `prospective_protocol.py` 使用固定 0.025 步长全量程并保留旧网格值，共 41 个候选阈值；新协议在任何新正式请求前固定。它没有选定生产阈值。原质量要求不变：每阶段至少 150 正例和 50 负例，误拦率单侧 95% 上界不超过 2%，负例检出率单侧 95% 下界至少 50%。所有数值工具仍不授予认证。

## 已实际核对的来源与预算

排除全部开发组和历史已启动/预约组后，来源资源合格池为 2266 个独立组：350 interview、1916 introductory。来源为官方 train 的主机分组，不能称为 APPS 官方 test。

Root 实际生成两份零费用来源提案；均未读取正式候选标签或概率，没有正式 cohort、预约、主模型/Jev/Oracle 执行。按固定哈希排名交替分配校准/留出，两个阶段互不重叠，难度成员及执行顺序预先固定。

- 建议方案：每阶段 316 组，各 175 interview、141 introductory；最多 632 次主模型与 632 次 Jev。公开提案 `reports/artifact-v3-apps-sampling-proposal-316-20261011.json`。
- 较小方案：每阶段 252 组，各 175 interview、77 introductory；最多 504 次主模型与 504 次 Jev。公开提案 `reports/artifact-v3-apps-sampling-proposal-252-20261011.json`。

用户已批准316组方案，新增主模型最多400次、Jev最多111次。Root实际创建新授权并验证原账本桥接：主模型累计518/1150、阶段Jev229/861，各余632，历史Jev27次另外保留。保留原费用、失败和pending，不重置计数。预算增加不能保证取得足量自然负例，也不能保证准确性合格。

## 实施与验收顺序

1. **授权与来源快照桥接。** 新授权文件只能在真实用户批准后创建。保留 400→500→750 的原 lineage、原 SQLite/预约根目录、全部费用/失败/永久 pending。分开封存采样时的原 750 source snapshot 与新的 budget snapshot；真实核对原 starts/finishes/reservations/metadata 一致，只允许已批准 chain/caps 扩展。不得忽略原摘要哈希或重置计数。
2. **正式主模型管线。** 完成 `formal_collection.py`、独立 Rust collector 和 `formal_intake.py` 的真实集成。固定完整两个阶段、来源/task SHA、public prompt、high/16384/120 秒配置、source kind、runtime、协议和编译图。全批一次完整原始来源/历史事务，逐条实际请求/SQLite/events/raw response/session/receipt/executable 关联；不得用 caller boolean 或持久缓存声明代替原始证明。保留所有失败、Unknown、pending 和未启动成员。
3. **独立标注。** 主模型仅接收原 public 字段。候选保留原始唯一 Python 代码，不修复候选；完整顺序断言在独立受限进程执行一次，create-new 原 Oracle。默认只读。实际 Oracle、源码图、runtime、全部原测试对和候选字节关联后才形成标签，标注完成前不读取 Jev 概率。
4. **真实正式登记。** 全校准 allocation 终态后，原始数据重验并检查至少 150 正/50 负。检查该 allocation 尚无 Jev 预约，create-new 登记。配额不足则生成不可认证终态，留出不开，不换样本、不造负例。
5. **正式 Jev 校准。** 每个已登记 binary 候选最多一次 Jev 请求，无自动重试。发送完整原候选和测试对，不发送 Oracle 结果或标签。实际请求、原始响应、精确 NOUL 问题、model、usage、共享预约/start marker 和实际编译来源关联。失败仍进入原 binary 分母。仅在新协议的固定 41 点上选择候选阈值；配额、误拦和检出门槛全部合格才可资格化留出阈值。
6. **一次真正的留出。** 真实校准原证据重验合格后，在原共享账本 create-new/fsync 独立 source-opening marker。此前不得生成留出主模型候选或标签。只消费原预定留出成员，独立标注与 Jev 观察规则相同，使用已锁定阈值；结果消费另设一次 marker，崩溃也保留消费，不能重新调参或重跑。
7. **结果与交付。** 从原登记和原观察重算终态。通过才认证受限 APPS 顺序调用断言范围；不声称通用代码正确或整个 agent 已减少幻觉。未通过则明确失败或不足，不启用生产策略。复验实际复制的运行程序/新工具、off/shadow/enforce/取消恢复、包清单/ZIP 全字节及费用前后不变；保存新运行包，更新 HANDOFF，按明确公开路径提交 GitHub。

## 当前未完成项

实际新授权桥接、原来源重建、正式cohort与首8零HTTP prepare/intake已通过，首8正式calibration主模型采集已启动。新main实现180项Python/10项Rust及构建检查通过，真实EXE与完整owner图一致；其付费所有者与EXE冻结。全校准独立标注、真实登记、Jev校准、留出opening/consumption与最终新运行包仍待完成。注册事务15项隔离回归通过，Jev及留出新入口继续开发。生产off、正式阈值null、真实留出未打开，整体目标active。旧安全ZIP不包含本次APPS新工具。
