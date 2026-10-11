# MBPP 外部任务来源

来源：Google Research，Mostly Basic Python Problems，Austin et al. (2021)，
[官方说明](https://github.com/google-research/google-research/blob/master/mbpp/README.md)。
数据集许可：[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/)，见
[Google Research 官方数据卡](https://huggingface.co/datasets/google-research-datasets/mbpp/blob/main/README.md)。

本机原始文件从 Google Research 仓库固定 commit
`f82046ba5aabbbb427dbfd38a254d26bff08b533` 下载，文件未经修改；
SHA256 `ca95deaa9a01ef0a6f439f88bcf0dd3db3563d22f22aad6cae04ebb9a8d8c8e9`。
任务目录由 BugleCat 的离线索引工具派生，只包含hash、组和分割元数据。

参考解和测试是外部材料，不是未来 Agent 回答的事实标签。公开基准可能已进入
模型训练数据；独立的新回答和逐声明断言仍须另行采集。此目录没有真实新模型
调用、正负完成标签或已认证阈值，不称为真实用户部署分布。

## 2026-10-11 官方完整任务补充

完整 `mbpp.jsonl` 来自同一固定commit的[官方文件](https://github.com/google-research/google-research/blob/f82046ba5aabbbb427dbfd38a254d26bff08b533/mbpp/mbpp.jsonl)，原始bytes未经修改，SHA256 `ccf64ceae9c5403bf50a044cb6d505bfd2a2963ee58338ba268fd65beab92a9f`，同一数据集许可和署名适用。

`mbpp-expanded-20261011-v2.json` 是明确标注的派生数据：保留原sanitized任务不变，并将完整官方数据里新任务的 `text` 映射为 `prompt`、非空 `test_setup_code` 放入 `test_imports`，保留原 `code` 和 `test_list`。未采用challenge tests，也不重写官方参考或断言。排斥旧ID/旧规范化solution/旧prompt和静态不支持项；新增任务按固定规则分组。原group/split保持不变，新source不能当作已经独立标注或已经认证的结果。
