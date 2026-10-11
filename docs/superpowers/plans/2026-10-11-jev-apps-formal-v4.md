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

离线正式Jev/一次留出管线已完成：九新Python所有者、三个new-only EXE、220相关回归/构建/静态检查通过，归档与实际编译源码一致。Root修复独立审查复现的复制EXE投射漂移和ZIP后费用闭包，登记23/终态13/打包11回归通过，并实跑四工具复制后的纯fake入口。公开证明 `reports/artifact-v3-apps-offline-tools-20261011.json`；源码/EXE此后保持封存，不改任何原付费main图。受控fixture的200 slot E2E不等于真实效果认证。第18批终态100已采集/216未发送、83正/5负/12Unknown，实际队列继续；真实Jev/留出/新整包仍待下列原门槛。以下73组快照为先前记录。

实际新授权桥接、原来源重建、正式cohort与首8零HTTP prepare/intake已通过；固定316队列继续采集。第13批终态73已采集/243未发送、61 supported/2 unsupported/10 Unknown，main591/1150、阶段Jev229/861；这是采集中快照，不是正式终态。5正文无效、2transport保留Unknown、不重试；3有效正文的Unknown经Root只读AST复核，分别使用受限array/sys导入及global，保持原Unknown，不修改候选、worker或标签。新main实现180项Python/10项Rust及构建检查通过，真实EXE与完整owner图一致；其付费所有者与EXE冻结。

Root注册事务23项回归覆盖原全数据、证据集合与预算在intake前后变化、证明结束后变化仍被原捕获SHA拒绝；终态工具13回归、打包门禁9回归，共45项/Ruff通过。实际复制四个APPS校准/留出主模型与Jev EXE并调用其offline-success入口，编译源图/EXE原字节匹配，真实收费0；尚未实际stage运行包。Jev校准及一次留出新入口正完成原字节闭包、全allocation、blind registration、固定阈值消费和临时账本fake E2E，离线fixture不能作效果认证。

先完成316原终态/独立标注，再按原150正/50负决定实际登记；不足即保留真实不足报告，正式Jev与留出不启动。配额足够才实际校准，通过原41点网格和误拦/检出要求后才打开原一次留出。最终认证只覆盖冻结APPS顺序断言，通用声明阈值和整体agent收益仍无证书，生产默认off。全校准、真实登记、Jev校准、留出opening/consumption与最终新运行包仍待完成。GitHub最新源码检查点`7cd1013e25989265e4b692a7ca0e559fc8f26436`已实际推送，包括九个新管线所有者、三个新工具入口、原证据闭包和新版打包器；终态/最终包与后续文档按明确公开路径提交。旧安全ZIP不包含本次APPS新工具，整体目标active。

2026-10-12第24批真实终态更新：127/316，103 supported/8 unsupported/16 Unknown，校准pending0；main645/1150、阶段Jev229/861。第23、24批传输失败均停止队列并留存；免费连通检查可达，只继续原未发送名单，失败组零重试。新的原终态报告操作脚本已准备，只有全316固定allocation终态后才调用真实只读终态所有者；不执行Oracle、不预约或发送Jev，不打开留出。该快照不能替代后续完整原始终态。

## 2026-10-12完整终态与剩余规划

1. 全316真实main及独立断言已完成：263 supported/25 unsupported/28 Unknown。297正文有效、12正文无效、7传输失败，未发送/校准pending均0。全原证据只读重验通过；9个范围Unknown全部Root人工复核并与最终候选SHA关联，保留原标签，零重执行、未读Jev概率。
2. 正例配额达到150，负例25低于50，正式状态为 `independent_quotas_not_met`。真实登记、Jev校准、原一次留出未启动。主模型累计834/1150、阶段Jev229/861，原历史pending1和Jev27保持；不追加本轮收费。
3. 最新开发运行ZIP已完成实际复制工具/GUI/CLI/生命周期/全ZIP/source/费用闭包验收，新增收费0：`releases/jev-apps-v3-incomplete-20261012.zip`，27.71 MiB，SHA `53ec7f462e14c4aa3a4a1d7334b2fda42d331e887ae40b477a4bcb7739ffc3ec`。v3.3修复运行时+四个新APPS工具；生产off，阈值null，强制类别为空。运行包交付不能当作认证通过或整体目标完成。
4. 先完成新独立来源的免费可行性普查：按来源/任务分组/历史暴露/执行范围区分真实难任务、真实缺陷产物与不可执行材料；只读现有来源和元数据，首轮不发送模型/Jev、不重执行本轮Oracle。不能复用原未开启留出补校准。
5. 根据来源事实形成新方案：若仍验证主模型自然产物，须在标签/概率前冻结全新名单与分层准入；若使用官方真实缺陷与修复产物，必须单独声明其来源和认证范围，不能称为主模型自然分布收益或原APPS阈值认证。生成的错误变体仅作为离线开发测试，不补正式自然负例配额。
6. 仅当来源、分组、断言范围、预注册与完整独立两阶段的预算可行后，提出可审查的全新固定方案。当前剩余主模型316不足以直接承诺新的至少200+200主模型样本；已批准预算不等于可更改本轮冻结来源。新的具体收费准入另行明确，按每组每服务一次、失败计数、无自动重试执行。
7. 新研究须重新独立标注、达到150正/50负、校准原误拦/检出门槛，再一次全新留出；不能迁移本轮或开发概率/阈值、降低门槛或重开失败留出。合格后才公布限定范围认证；通用声明与整体agent收益仍需各自证据。

本轮真实公开证据：`reports/artifact-v3-apps-formal-main-terminal-20261012.json`、`reports/artifact-v3-apps-independent-review-20261012.json`、`reports/artifact-v3-apps-package-20261012.json`。源码检查点7cd1013已实际推送，本节/上述新聚合待后续明确提交。
