# 代码产物校验轨道：首个真实开发请求

日期：2026-10-11。用户选择新增artifact轨道，沿用现有400次Jev和400次主模型授权。原声明轨道保留。本报告不认证正式阈值、留出或生产启用。

## 真实执行

- 首个预注册开发组：`mbpp-main-104`。候选为原始真实主模型代码，独立冻结接受断言通过。该回答已经查看，只能development。
- 在请求前核验源文件、原采集manifest、原主模型收据及原始response bytes、候选/代码、独立Oracle、协议、预算授权和请求hash。没有参考代码、标签或执行结果进入Jev body。
- 独立执行真实二进制validate与fake transport bytes join，均0网络；12个实际binary回归通过，重复reservation被拒绝。
- 共享全局预算reservation后实际只发送1次，无重试。actual model `jev-1.13.0`；耗时1085ms；655输入/25输出token；单NOUL `acceptance_violation=0.12`。
- `artifact_observations.py` 独立核验真实request hash、reservation原文件与启动marker、compiled source/exe、原始response bytes、model、usage和最终receipt。成功状态为 `original_transport_bytes_verified`。
- 本阶段Jev实际启动1、完成1、预留1，授权剩399；历史累计28。主模型本阶段仍12启动/11完成，授权剩388；未完成费用未知，余额与货币金额未查询。

## 构建与来源

- sidecar exe `rust/target/debug/examples/verification_artifact.exe` SHA256：`82efa727af1265ba03f3a14c44cc9ef7d179589d0c2e72822609a94e40a47b9e`
- main source：`dc4d190f83210e4c17c0962f14532c486f9cf93b2bb917b58f02d4aaac9ba9bb`
- support source：`8cd1e70036f0c4aa7b9774cd70a2dafbc235749756814cfb72ecda841a4385ed`
- provider source：`d8bccee31b4fca24d91e3593767ed162fbaadf0120349b68e358d8866c5c8871`
- 请求hash：`1a22cadf576097259bee2c9e16660eedcf2b1ab3e2d238abd1497f4bfb0fcf57`

原始收据、response和SQLite台账保留本地；开发评分见 `evals/verification/reports/artifact_smoke104_score_20261011.json`。新sidecar不在已经封存的v3.3运行包中，旧包来源图不修改。

## 验证和限制

全套Python verification回归190 passed；新artifact模块包含契约、注册、评分、真实二进制和原收据篡改回归。额外检查覆盖取消前收据、失败响应bytes、usage篡改；Windows扩展路径使用现有规范化函数，实际原收据join通过。

1个开发正组、0个开发负组；任何网格均不满足样本门槛。`certified_threshold=null`，holdout未运行、production=false、native claim certification=false、原框架A和规则B baseline unavailable。NOUL低概率不会升级为Pass。

正式来源intake、概率前正式注册、校准和一次留出仍未完成。新正式来源至少需要400组，剩388个主模型请求和399个Jev请求无法保证完成，且自然产生独立负组数量未知；不能复用已查看7组、造负例或降低150正/50负门槛。后续按既有计划先实现并核验新来源登记，不自动追加收费。

后续源盘点已经实现：只读真实主模型SQLite与Jev reservations，失败和pending也排斥，原始数据库bytes不改。实际原MBPP池剩校准194组/留出207组，校准来源自身不足200；2项额外回归通过。`artifact_source_inventory_20261011.json` 是库存检查，不是正式注册、采集或留出开启。需要补充独立外部来源后才能冻结足够的新校准池。
