# Jev APPS 标准输入轨道：免费可行性验证与后续门禁

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
