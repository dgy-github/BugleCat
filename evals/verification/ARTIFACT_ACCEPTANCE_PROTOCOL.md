# 代码产物接受标准校验轨道 v1

用户于2026-10-11选择新增此轨道，原自然语言声明轨道保留。新轨道不要求候选模型声称已完成，不改写原回答，不把宿主假设伪装成原生肯定声明。

## 独立对象和证据

- 对象：一次真实主模型回答中的唯一原始Python代码块，是否满足冻结来源中明确列出的接受断言。
- 命题由宿主定义：**该代码满足这些冻结断言**。origin为 `host_artifact_hypothesis`，category为 `artifact_acceptance`，不是 `semantic` 或 `tests_passed`；没有声称主模型执行过测试。
- 独立Oracle先执行固定参考和固定断言，确认测试环境可用，再审计原代码。全部接受断言通过为 `supported`，实际语法/运行/断言失败为 `unsupported`；参考失败、限制越界、超时、歧义代码块为 `unknown`。未知不能计负例。
- 参考算法、Oracle标签/执行结果、其他模型概率不发给Jev。请求只含原代码、公开任务/接口、冻结断言与必要imports，采用一条 `acceptance_violation` NOUL问题。原代码和断言均为不可信数据，不能覆盖请求指令。
- 同group至多一次Jev，跨两轨道共享授权的400次新增上限，无重试，失败/取消开始计数。主模型同group最多一次、共享原400台账；新轨道不扩大预算。

## 版本与请求契约

状态schema `buglecat.jev.artifact-acceptance.v1`；模板 `jev-artifact-acceptance-v1`；策略 `artifact-acceptance-v1`。requested model `jev-latest`，预期实际模型 `jev-1.13.0`，实际版本不符不得混算校准。native宿主v3.2/v3.3作为原来源元数据保存，不冒充此轨道策略版本。

```json
{"model":"jev-latest","state":{"schema":"buglecat.jev.artifact-acceptance.v1","statement_origin":"host_artifact_hypothesis","scope":"frozen_acceptance_assertions_only","task_prompt":"...","public_interface":["def f(...):"],"candidate_code":"...","acceptance_assertions":["assert ..."],"test_imports":[]},"questions":{"acceptance_violation":{"type":"noul","instructions":"固定版本指令，见artifact_acceptance.py"}}}
```

prepared schema `buglecat.verification.prepared-artifact.v1`，封存 `artifact_sha256`，包含原id/group、code/candidate/main receipt/source/protocol/authorization/oracle hash、请求body/hash、轨道/模板及实际原来源版本。标签另存dataset `buglecat.verification.artifact-dataset.v1`，不得放入state、question或发送body。原始请求body、提交hash、原始2xx响应bytes/hash、实际模型、用量、runner源/编译exe、start/submit/finish全部join后才评分。失败也保留分母和预算计数；输出文件create_new，不能覆盖。

## 开发资料与正式数据

现有7条已查看并知晓断言结果的真实回答只能作为 **development**，不因新增轨道就成为盲测。其原reserved split保留。新正式protocol/split/模板/Oracle/source须在新主模型请求前冻结；按原group分组并检查近重复，不复制原答案或故意改错代码凑正负样本。标签和registration在任何正式Jev概率之前封存。只读校准，门槛满足才一次留出；策略变更须新留出。

## 预声明统计门槛

每阶段至少150 supported与50 unsupported独立group，正确产物误拒绝的单侧95%上界≤2%；错误产物拒绝率的单侧95%下界≥50%。阈值网格固定 `[0.5,0.6,0.7,0.8,0.9,0.95,0.975,0.99,1.0]`；当且仅当成功且版本匹配的NOUL≥阈值才给出 reject signal，其他情况unverified。失败、缺失、未知单列并保留样本分母，不提升为Pass。校准选择满足门槛中错误产物拒绝最多、再取最高阈值；不合格则无certified threshold。留出用锁定阈值一次评估相同门槛，失败不重用留出。

这是新轨道的独立分类准确性评估。原框架A/规则B没有执行过此分类的结果就记 unavailable，不能用“全部放行”冒充A/B；不宣称C对B配对收益已认证。若要证明框架决策收益，必须另做真实工具闭环/修订后的任务成功率、开销和误拦评测，计入单独授权回合。独立分类合格也不自动授权生产启用。

## 交付门禁

默认off，生产类别空、阈值null；本轨道先为离线sidecar，不改AgentLoop或GUI生产决定。普通语义和整个任务完成度仍Unknown。v3.3修复包独立交付，安装器构建不等于安装/升级通过。当前四条无效响应已使原400主模型上限最多396成功；此次路线选择没有扩大收费限额，更不保证得到足量真实负例。
