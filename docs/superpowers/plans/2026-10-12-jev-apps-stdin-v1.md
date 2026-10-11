# Jev APPS 标准输入轨道：免费可行性验证与后续门禁

## 最新验收状态（完整终态，取代下方途中快照）

A原型完成：限定108项fixture通过；纯packet初版40项通过，独立复核补修后81项通过（4.38s），限定Ruff通过。B免费完整首参考检查完成：固定1130组、989通过/138失败/3Unknown，pilot8仅引用、新增1122次执行、3027原对保留。Root零重执行逐组复验原row/首参考/完整IO及全部收据、source/runtime。

989参考通过组逐一构造完整Jev请求，候选JSON字符串预留16000 UTF8 bytes，988资源通过：778 interview/137 competition/73 introductory；1组超过48000 bytes拒绝，不截断。独立复核补上实际loaded参数/numeric code/常量/alias闭包与候选硬限16k；修订后Root重验全1130原收据，并用当前packet重做989资源构造，仍988通过、最大35939 bytes，无参考重执行。原冻结源码和834笔main行/metadata、229份预约原bytes前后未变，新增收费0。当前公开证据：`reports/artifact-v3-apps-stdin-reference-pool-revision2-20261012.json`（SHA `21b2d2c145cd8c6bb06ebec7d63b59e33b36330a287f2dc6e21438133c0cae21`）；原报告保留历史快照。

这已证明首版受限STDIN执行和完整请求有可用复杂来源，不能证明主模型自然负例足量、官方APPS兼容或Jev收益。source authority、收费collector/intake、独立正式登记、新阈值及一次留出闭包仍待开发；当前生产off、阈值null，原运行包不含STDIN工具。

## 授权与当前状态

用户已选择扩展 APPS 标准输入轨道，继续评估主模型自然产物。本次选择只授权免费可行性验证，不启动新的收费请求。由既有 6.1 Sol 实施代理实现开发原型，Root 审查契约、来源证据与交付记录。

旧316组终态为263 supported /25 unsupported /28 Unknown，负例未达50；旧正式登记、Jev校准和留出未开启。旧名单、运行政策、源码/EXE和证据保持封存。不能挪用旧留出或把两条执行轨道拼成一份认证。

只读静态普查发现1734个没有已知历史分组、旧开发/正式allocation或实际启动/预约命中的最早STDIN代表：1241 interview、360 competition、133 introductory。1728组完整文本IO共5057对，6组空或不等长，4组IO自身超过48KiB。首参考均有原index0，1730可解析AST。静态数量不代表执行准入、语义独立、未受训练污染或自然负例足量。

私有普查报告raw SHA：`af66f7e32c8faa771296616f59d56bc97407682234b58387c61bd75c194c897b`。来源原文、2627个source/owner绑定及原834 main/229 Jev元数据前后不变；参考/候选/Oracle/模型/Jev/预约均0。

## 开发契约 v1

以下是首版原型约束；通过离线审查后才能冻结新来源准入契约。它不授予真实来源准入、正式登记或收费权限。

1. **入口与状态。** 原始Python脚本文本不修复、不重写为函数；执行环境`__name__='__main__'`。每个完整原测试对使用独立新子进程及namespace，顺序不变，不跨对共享状态。不能宣称与官方重写`code()`执行器一致。
2. **原始输入。** 只接受str或list[str]；list以LF连接，不额外补LF。原始JSON及转换规则分别绑定收据。`input()`读一行，移除单个LF及紧邻CR；末行无LF仍可读，耗尽引发EOFError。首版不提供sys、open、buffer或文件描述符。
3. **输出。** 只提供受控`print()`，捕获最多64KiB UTF-8文本，超出为Unknown；候选不能取得真实宿主stdout。比较固定为`ordered-whitespace-tokens-v1`：两侧`split()`所得token序列完全相等；只忽略空白布局，不提供浮点、数值格式、无序集合或四舍五入容差。标签仅表示是否满足这个新契约，不表示APPS官方全部容差下的正确性。
4. **语言范围。** 基于既有受限导入与builtin规则建立明确新所有者，新增input/print直接调用和只读`__name__`的精确main guard语义。input/print别名、传参和同名绑定拒绝；`__name__`写入拒绝。首版不支持class、global/nonlocal、装饰器、type、格式反射、动态代码、反射、OS/网络、导入越界、依赖属性写入或删除。新增例外须逐项测试，不能静默修改旧政策。受限语言审核不是OS安全沙箱。
5. **资源。** 每对子进程5秒，整组30秒，完整原对数最多100；超时、资源不足、输出越界和不支持的依赖均为Unknown，不作负例。不声称内存隔离。不能截断原测试对或提前成功；因整组deadline耗尽无法启动的剩余对全部保留原index，记录Unknown和`child_started=false`，不伪称已经执行。
6. **标签。** 完整范围内全部对通过为supported；语法错误、支持范围内程序异常或token不匹配为unsupported。任一对存在范围/资源Unknown则整组Unknown优先。NameError、ImportError与SystemExit首版保守Unknown。失败对保留证据并继续后续对，全部完成且无Unknown的收据才可形成二元标签。
7. **证据。** 收据绑定原code、原IO、契约与参数、全部新owner源码及实际Python runtime；执行前后图不一致即拒绝。子进程只传必要Windows运行环境，移除凭据；输出JSON必须结构验证。原型不接入任何真实收费账本或自动重试。

## 分阶段开发与验收

### A. 免费原型

- 新模块限于`evals/verification/apps_stdin_v1/`，测试限于新STDIN测试；旧已付费owner和EXE不可修改或重编译。
- 用自制fixture验证EOF、CRLF、无末行换行、list文本转换、`__main__`、输出顺序、异常、Unknown优先、跨对状态重置、资源边界、受限导入、凭据移除及源码漂移。
- 运行窄pytest/Ruff，Root独立审查关键执行边界。自制正确/错误程序不补正式自然正负样本配额。

### B. 免费来源准入试验

- A通过后，另建明确只读source准入入口。使用原最早整组代表与原index0参考，不选择后来通过的参考或成员；旧历史和完整旧allocation整组排斥。
- 在真实参考执行前冻结新契约和source/runtime图。参考执行属于免费本地验证，必须单次create-new收据保留所有结果；不能复跑来挑合格结局。
- 完整原问题、IO和运行契约必须容纳于新Jev请求上限，预留原候选空间；不得裁剪题目、IO或断言来满足资源要求。
- 只用首参考在新契约下完整通过且资源合格的组形成来源池。范围拒绝、失败或Unknown均留存，不变负例。统计难度/范围覆盖，只称新受限轨道来源，不称官方test或自然负例已经充足。
- 暂不生成真实主模型候选或JeV概率；参考/来源审查与之后的独立候选标注分离。

### C. 全新方案与收费门禁

- 根据B的实际合格池，在任何新候选/概率前固定开发排斥组、校准和留出整组成员、哈希排名、难度配比、运行顺序、主模型prompt/profile和Jev新packet。
- 原主模型累计834/1150、余316；阶段Jev229/861、余632，历史Jev27另保留。即使每阶段恰好150正/50负且没有Unknown，两阶段也至少400个主模型请求，当前理论缺84；实际来源/失败/自然负例比例可能要求更多。当前没有新增预算或新收费准入。
- 在A/B证据和完整新方案可审查后，才给出具体预算缺口与授权请求；不得清零旧账本、突破费用上限或自动沿用旧316来源授权。

### D. 自然产物校准与一次新留出

- 每组每服务最多一次，无自动重试；真实失败、Unknown和pending全部保留。先独立执行标注，标签依据明确，疑难范围由Root人工复核，标注完成前不读Jev概率。
- 每阶段至少150正/50负，误拦率单侧95%上界≤2%、负例检出率单侧95%下界≥50%；不得因数量或结果不足降低门槛。
- 新轨道重新冻结packet、全量程41点阈值网格和失败分母规则。旧轨道或开发概率不得混入。低概率不能自动标为VerifiedPass。
- 全新校准终态足量且效果合格后才一次打开预定新留出；锁定阈值，保留一次opening/consumption，失败后不再调参重测。
- 通过才授予受限STDIN契约认证；通用声明、全部APPS代码和整体agent减少幻觉仍需各自证据。

## 交付约束

更新既有HANDOFF、计划和脱敏聚合，按明确路径提交GitHub。原运行ZIP`releases/jev-apps-v3-incomplete-20261012.zip`已经验收，仍为开发交付。当前STDIN原型尚未进入该ZIP。认证未完成期间生产Jev保持off，阈值null，强制类别为空。

## A阶段实际完成记录

6.1 Sol完成3个新所有者和3个fixture测试文件；`python -m pytest tests/test_verification_apps_stdin_v1_contract.py tests/test_verification_apps_stdin_v1_worker.py tests/test_verification_apps_stdin_v1_oracle.py -q`实际108 passed（5.89s），限定新路径Ruff通过。Root复核当前三个源SHA与实施报告一致、9个旧核心文件与原提交文本一致；公开证据`reports/artifact-v3-apps-stdin-prototype-20261012.json`。这些测试为开发原型证据，不能替代真实首参考准入或自然候选标注。

已修复孤立CR与CRLF的处理区别、import后磁盘源码漂移关联旧内存policy的问题；禁止Python泛型字符串参数绑定逃过保护规则。装载契约/原常量使用原bytes编译，wrapper核对加载图与当前函数字节码；参数/源码前后漂移拒绝。30秒无法启动的剩余原index保留Unknown，`pairs_executed`明确表示子进程启动尝试，包括超时，不表示每对已完成。

新reviewer首版另禁issubclass与泛型TypeAlias/TypeVar/ParamSpec/TypeVarTuple语法。当前已开始对1734个新最早代表做固定reviewer的静态范围检查，仍不执行真实参考，不作source admission。B阶段真实本地单次参考准入入口及证据图尚待完成，收费方案尚未创建。

## B阶段静态与首参考试验记录

静态联合通过1131组（879 interview/173 competition/79intro），共3099原测试对；1组完整IO超过48KB。Root核对完整原2627绑定文件与新3owner当前字节一致，公开`reports/artifact-v3-apps-stdin-static-scope-20261012.json`。没有以静态通过授予packet或正式来源准入。

首参考小试验在执行前固定完整8组计划、源码/runtime图、原code/IO/question SHA和原费用快照，以`SHA256('stdin-feasibility-v1:'+group_id)`每难度排序取4 interview/4 competition。筛选原question+原IO escaped envelope≤32000 bytes，完整原测试不裁剪。全8首参考index0各一次、create-new/fsync marker/receipt；7 passed/1 failed/0 Unknown，共37对。失败组6 OutputMismatch/1pass保留来源拒绝，不能作模型负例。Root只读重验原row/首参考/完整IO及收据图，无重执行。公开`reports/artifact-v3-apps-stdin-reference-feasibility-20261012.json`。

正在进行完整来源池检查：固定新seed全静态/资源合格成员，先create-new/fsync完整plan，再执行未尝试首参考；pilot8的原一次结果全部只读引用，失败也保留。每组独占marker/receipt，每25组核对原图和全部费用元数据，崩溃不重跑。来源完整终态/合格难度分布及正式packet字节审查完成后，才能制定全新候选开发排斥与校准/留出名单。参考验证免费，不生成主模型候选或Jev概率，不改变原批准上限。

全pool计划已实际冻结1130成员（879 interview/172 competition/79intro），另1资源拒绝。plan raw SHA `962e443beaba3435e5beda519322e8eefaf95b2a9688c3d798a00839982ac996`，包含原source/new3owner/runtime/费用与全部source members，在1122最多新增参考执行前create-new/fsync。已有pilot8全部只读复用，包括失败。575终态/497通过/77失败/1Unknown为途中快照；具体Unknown原因不超出原收据generic reason。

## 新packet离线实现

Root新增纯`apps_stdin_v1/packet.py`和独立packet测试，40 passed（0.59s）、Ruff通过。严格整体48,000 UTF8 bytes、完整原材料/候选、不发参考/标签/Oracle结果；复用原numeric primitive，单项NOUL，actual model与u64 usage校验，仅产生未认证数值信号。主模型public prompt声明新受限政策，不含hidden IO或参考。当前构造依赖图/加载image/原runtime和前后漂移核对，原测试材料无tuple→list投射；精确canonical rebuild拒绝caller字段变更。

公开证据`reports/artifact-v3-apps-stdin-packet-20261012.json`。新builder不接入收费或正式资格化，候选reserve与全池真实packet资源门禁、未来收费collector/intake/阈值与一次留出管线仍需按新轨道建立；原API与已付费owner不修改。它不被当前首参考Oracle导入，当前参考池冻结图保持不变。

## C阶段落地计划：先离线接通，后申请具体收费准入

### C1. 新来源证明与profile

- 新STDIN source authority只读消费已经封存的最早整组、原index0首参考终态和Root资源证明。重验全原row/成员、完整IO、receipt/source/runtime及历史整组排斥；禁止仅给group_id或重算一个自签hash冒充原证据。不要重执行参考来挑通过结果。
- 准入scope固定为受限原脚本、fresh child、完整原顺序对、`ordered-whitespace-tokens-v1`；显式不声称官方浮点/无序容差或OS内存沙箱。
- profile绑定当前worker/wrapper/packet/原始runtime、主模型model/base URL标识（不含key）、public prompt、完整候选单Python代码块提取规则及所有资源参数。候选JSON字符串不得超过16000 bytes，原完整Jev请求不得超过48000 bytes；越界记Unknown，不裁剪或重写代码。
- 新authority只授予source资格，不含调用、阈值、留出或production授权；使用独立schema/owner。任一当前graph漂移关闭准入，不能修改已收费旧owner/EXE。

### C2. 固定分组方案（尚未生成正式名单）

建议仅使用资源合格的915复杂组（778 interview/137 competition），8开发+400校准+400留出，开发4 interview/4 competition，校准/留出各340 interview/60 competition。这个配比实际来源足够，余94 interview/13 competition/73 introductory，共180组不分配，也不能自动补不足。

新固定seed建议`stdin-natural-400-v1`，每难度按`SHA256(seed + ':' + group_id)`与group_id稳定排序，先开发，再校准，再留出；同一整组成员全程同split。正式名单必须在任何新候选/概率前create-new/fsync，绑定完整source authority、固定profile/protocol、源码/runtime和原账务快照。参考试验只决定来源资格；不能按主模型效果或Jev概率选择正式成员。所有开发allocation（无论是否真正发送）整组排斥正式阶段。

本方案是待审查的数量与算法，不是实际allocation或收费授权。正式freeze前还须重验整个历史/旧正式632组/新旧实际启动与预约排斥，若有效复杂池不足808则停止制定该名单，不偷偷改配比或挪用旧留出。

### C3. 共享预算采集与原始intake

- 独立新STDIN collector复用原SQLite main账本、累计预算授权链和原exclusive start语义；不能新建空账本获得重新计数。每组最多1次HTTP，禁库默认自动重试，request/body/session/event/receipt/raw response完整hash关联。
- collector只读取public question/starter/冻结policy，不读取参考或hidden IO。raw candidate保留原字节，非法正文、传输失败和候选越界保留永久终态；pending也计预算且不得重发。
- original intake验证actual EXE的内嵌source/build-info、原prepared/request/session/账本start与finish/raw response一致，然后只运行一次候选Oracle并封存全部原对和新graph。标签完成并锁定后才允许读Jev概率；范围/资源Unknown不作负例。
- 小批检查最多固定开发8组，每组每服务最多一次；只验证联接、契约可执行覆盖、概率shape、收费计数和清理。自制fixture/开发产物不进入正式认证配额，开发检查失败先修免费代码，不能收费重试已尝试组。

### C4. 新Jev与统计/留出闭包

- 新sender绑定当前STDIN packet、actual model与NOUL严格shape，共享原Jev预约账本/预算链；完整一组材料一次请求、无自动重试。任何异常和费用永久保留，不把JeV失败从质量统计分母删除。
- 新protocol单独绑定STDIN政策与新packet，不借用原call-based的声明范围/阈值认证。固定41点网格：`n/40, n=1..40`加`0.99`；与旧全量程网格同数值，不复用旧观测。阻断规则`acceptance_violation >= threshold`；合格阈值中先最大化检出的负例数，再取较大threshold，tie规则在新收费前冻结。
- 每阶段独立标签至少150 supported/50 unsupported；误拦率单侧95%上界≤2%，负例检出率单侧95%下界≥50%。显式报告全部分配组中的正文失败、policy/resource Unknown、费用失败和有效覆盖率，不能把范围外代码称为通过。
- 固定校准400组必须全部终态再判断配额，不按到达50负例停止；配额不足不发正式Jev/不打开留出。Jev校准通过才签发独立一次holdout opening，固定阈值后最多运行预定400留出一次。留出不得调参、替换成员、重跑、加入180余组或改成校准。
- 最终认证只适用于这个受限STDIN产物轨道。低NOUL不意味着VerifiedPass；agent通用决策准确性/减少幻觉仍需单独线上任务证据。

### C5. 收费上限与授权顺序

8开发+400校准+400留出最多808 main、808 Jev；有效二元标签不足或阶段门禁失败会提前停止后续阶段，但已失败/pending请求仍计预算。

当前main834/1150，余316；阶段Jev229/861，余632，历史Jev27独立保留。完整808上限缺main492、Jev176，对应未来累计main1642、阶段Jev1037。**这不是已经授权的追加，不在免费方案选择下调用，也不是key余额。** 离线source/profile/collector/intake/protocol/holdout代码及受控工具验收完成、固定全名单和预算差额可审查后，再请求这笔具体收费授权；不要重复小步试跑导致频繁追加预算。

不能据旧25/316负例比例保证新复杂STDIN400组取得50负例，也不能保证Jev统计门槛通过。若400校准配额不足，原结果如实封存，留出不打开；新的研究路线另行预注册，不通过降门槛或重解释标签获得认证。

### C6. 离线集成与交付验收

- 对来源伪造/整组跨split/旧allocation命中、资源裁剪、加载图漂移、账本并发重复预约、原始response替换、Unknown当负例、错误实际model和一次留出重复消费设置有意义的fixture测试；窄pytest/Ruff、新工具build/Clippy及actual build-info校验。
- 用临时账本和fake transport走开发、校准配额不足、Jev失败、合格阈值、一次留出完整闭包。fixture可证明管线，不计真实来源/自然标签/效果认证。
- 实际复制工具离线调用，验证GUI/CLI off/shadow/enforce和会话生命周期，再生成新manifest/ZIP并逐字复验所有成员、源码/结果/费用前后闭包。不含key、账本、原题、候选或原响应；正式认证未通过时production默认off、threshold=null。
- 逐阶段记录开发、窄测试、费用与认证状态到HANDOFF.md，按明确公开路径提交GitHub并验证实际远端。现有27.71MiB运行ZIP保持旧阶段交付，完成新包验收后才称包含STDIN工具。
