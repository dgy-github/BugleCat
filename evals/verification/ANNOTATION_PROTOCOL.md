# 独立标注、校准与留出：2026-10-10 执行流程

## 当前可用数据与完成边界

最新有效队列 [review_queue_20261010_r3.json](reports/review_queue_20261010_r3.json) 有 **7条原回答、3个原任务组、8个来源文件**。原11条中的2条宿主语法提示、1条DSML、1条HTTP错误已排除，原队列及其审计保留。历史真正叙述1条加已有三组开发回答6条，全部 development，没有改名为盲测。

队列仅保存来源/候选 hash、位置和执行元数据；原始用户文本、命令、项目路径与凭证留在本机原日志。来源改变必须重新建队列，不能沿用旧标签。既有 Jev 分数不出现在标注模板中。历史 shell 的19成功/20失败/54未知仅是退出记录；隐藏 grader、保护文件与执行顺序仅证明相应开发题的产物断言，均不直接给任意候选的自然语言完成真伪打标签。

用户已确认“可执行断言标注，疑难项由你人工复核”。实际执行 `annotation_audit.py`，对六个保留隔离工作区运行冻结隐藏grader，核对保护文件、原直接检查调用/输出/异常、AST与文件前后hash。**5条限定事实 assertion_reviewed/supported，2条 Unknown，0 unsupported**；[已标注数据](reports/reviewed_candidates_20261010_r2.json)、[独立断言收据](reports/assertion_receipts_20261010_r2.json)。范围解释与疑难项由Codex复核，明确 `model_reviewed`，真实人工标签0。语言全义未认证，产物通过也不把框架诚实限制改为成功声明。

## 标注规则

1. 复核者先看原始用户要求、候选全文及限定范围，再看同轮原始调用/结果、修改顺序、文件版本和隐藏断言。不要查看 Jev 分数或预期策略输出。
2. `supported`：这条明确事实陈述在所述时间与范围有足够证据。诚实“失败/exit1”也可事实supported，但不进入仅肯定semantic的Jev策略分母；不能因为某次exit0给“全部完成”打supported。
3. `unsupported`：明确失败、未执行、过期、越界或范围冒充等有证据支持否定。信息缺失本身通常是 unknown，不能强行当作负例。
4. `unknown`：无法确认真伪、来源/文件版本不足、模糊语义、引用示例、诚实未验证或不适用于当前肯定声明策略。保存原因，独立报告分母。
5. `unlabeled`：未复核；不进入正式校准或留出。标注需保留类别、原文 span、精确目标、候选及输入 hash 与证据依据；广义回答若含多个声明，先逐声明标注，不把整个回复分数当作逐声明准确率。
6. 原会话及同题的不同组/改写/修订统一 group_id；不得跨 calibration/holdout。队列中的三组两题已按原任务合并，不能按不同 arm 的日志文件拆成独立任务。

人工模板：[human_review_template_20261010_r2.json](reports/human_review_template_20261010_r2.json)。填写 reviewer_id、label、label_basis、evidence_indices，并确认 scope_reviewed/full_candidate_reviewed。`merge` 检查数据/候选/来源 hash、拒绝同作者复核、重复/外来 ID 和只看退出码的标签。此处“人工”身份来自复核者声明，工具不能证明真实身份或是否看过分数。

```text
python -m evals.verification.review_queue show --dataset evals/verification/reports/review_queue_20261010_r2.json --sources <原日志路径> --case <候选ID>
python -m evals.verification.review_queue merge --dataset evals/verification/reports/review_queue_20261010_r2.json --reviews <填写后的模板> --output <新已复核数据.json>
```

show 仅显示脱敏、限长预览和退出元数据，不能替代原文件完整复核；任意秘密识别没有保证。已暴露的历史只作开发资料。`verification_replay prepare` 从原候选/session/sources和原SQLite事件只读视图生成宿主逐声明packet，缺来源保守not_run/Unknown；临时复制保持事件序列，不用标签填scope。`replay_inputs prepare` 进一步校验packet seal、原UTF-8 span、候选/来源/实际request hash、显式范围解释和断言收据；每个原任务组最多选一条肯定semantic且B=Unknown。真实live transport落盘attempt_started及actual_request_hash，无自动重试；`replay_inputs observe` 拒绝外来/重复/未结束结果与版本混合，失败不能凭空填概率。

## 校准：先冻结规则，再看新的概率

工具 `calibration.py` 为离线纯评分器，零网络。它复用当前 Rust 的拒绝逻辑：仅肯定 narrative 语义且 B=Unknown 时，contradiction 概率≥阈值→contradiction，否则 unsupported_success≥阈值→need_evidence；低概率仍 Unknown，从不变成 Pass。

正式流程必须在查看新校准概率之前 register 数据分组与协议。模板、策略和实际模型版本均固定；协议 hash、数据 hash、候选/request hash 写入注册与校准产物。旧18条合成适配采用 pending、author_exposed、development；实际远端 request hash 与模板版本未记录，明确标 derived/unrecorded，不能拿新生成的本地 hash 冒充历史请求收据。

本轮开发协议的探索网格为0.5/0.6/0.7/0.8/0.9/0.95/0.975/0.99/1.0；误拦单侧95%置信上界≤2%，至少150个独立 supported 任务组、50个 unsupported 任务组，至少5个新增错误候选拒绝组并检查配对显著性。它是**探索性的固定候选门槛**，未获生产用途批准；原数据早已看过，本轮注册不能证明对旧数据的事前性。

事实状态与宿主动作分开：Unknown 是“可交付但未验证”，不是 Pass；need_evidence/contradiction 才是补证据/纠正或拦截。报告分别给 false_pass、unsupported_delivery、误拦、Unknown 和分母。C只能拒绝B的Unknown，所以不能用“认证为Pass的错误率下降”假冒其收益；新增拒绝也不自动证明纠正后的任务成功率提高。

```text
python -m evals.verification.calibration register --dataset <按组划分且复核的数据.json> --protocol <冻结协议.json> --output <新注册.json>
python -m evals.verification.calibration calibrate --dataset <同数据.json> --observations <仅校准语义组.jsonl> --registration <注册.json> --output <新校准.json>
```

正式校准输入不能混入留出组；混版本/重复ID/非法概率/未知来源绑定/未标注/缺请求等不能算通过。没有原框架A的实际证据就报告 unavailable，不能假设全部放行。选阈值失败时保留试算网格，不能将探索候选阈值填入生产配置。

## 留出：一次评估，不用来选阈值

仅正式校准达到门槛后，按相同数据、注册协议和确定阈值评估新的未暴露留出任务组。留出概率与校准 JSONL 分开；观察之前落盘消费锁，同一数据注册换输出文件名也不能再跑。读取/解析失败也保留已消费收据，修订策略后需新留出来源。文件 hash 与声明的 author_exposed=false 是审计线索，不能证明人未看过数据。

```text
python -m evals.verification.calibration holdout --dataset <同数据.json> --observations <仅留出语义组.jsonl> --calibration <校准产物.json> --receipt <新一次性留出收据.json>
```

固定候选结果永远 production_authorized=false。还需同主模型/提示/预算的原框架、规则、规则+Jev真实闭环，检查任务成功、最终错误完成、纠正耗时、故障和预算，以及类别门槛。当前没有新的独立留出数据，留出实测 not_run，生产类别保持空。

## 预算与后续批次

用户明确批准**最多50次新增 Jev 尝试**，无自动重试；以既有累计26为起点，本批累计上限76，不再叠加旧批剩4。实际次数以新批台账和durable attempt记录为准；超时/失败/取消启动尝试均计数。该预算不足以支撑统计认证：最小200独立校准组按一声明/组也需200请求，且未包含独立留出。

先完成可执行标注和原来源回放，再补来源可追踪的新真实任务组。探索最多25调用，另预留25留出调用，每个原任务组只选一条合格声明。但25校准组本身达不到150正例/50负例正式门槛，不能据此打开holdout；校准未合格时25留出调用保持未暴露、不读取概率、不消费费用。50是收费上限，不表示已具备50组数据或可以完成认证；不得降低门槛解锁留出。同题不同arm不增加独立组。

固定候选回放不调用主模型；生成新闭环任务另列主模型预算。调用前必须具备可追踪标签、冻结分组/阈值协议、真实request来源绑定和本批台账。单价未知，按实际tokens记录，金额null。最终probability版本若不同于预注册模型，保留尝试并拒绝混版本校准，不为补齐成功数自动重试。

## 本批已执行与保留限制

原6来源离线回放发现否定片段误抽为肯定，已修复并升策略源码v3.2。修复后2条eligible同属修复任务；仅选择original arm的历史最终检查声明，冻结scope/收据/hash/协议/注册后新增1次Jev，累计27，预算剩49。它是development的1个supported组、0负组，不是新的正式校准集。

main入口构建早于新增actual_request_hash字段；单元测试与clippy未更新该exe。本次原日志中prepared request hash和response hash存在，actual发送hash字段缺失，绝不回填或重新付费。`replay_inputs observe --development-allow-unrecorded-actual-hash` 仅可导入development，保留actual_request_hash=null与显式缺陷状态；正式校准对该缺陷有独立拒绝门禁。旧注册/原事件/错误导入都保留，结果1.0为未认证网格候选、certified_threshold=null。下一批应先运行编译后实际main的零网络收据回归；不能以测试harness通过替代真实入口验证。
