---
name: dashscope-image
description: 使用阿里云百炼通义万相生成图片；需要生成配图、插画或视觉素材时使用。
capability: image-generation
---

# 阿里百炼生图

调用 `generate_image` 生成图片，传入清晰的 `prompt`；需要排除内容时填写 `negative_prompt`。

- 默认模型由媒体 Provider 管理，当前为 `wan2.2-t2i-flash`，不要在提示词中伪造其他模型。
- 工具返回真实 `urls`、模型、币种、单价来源和本次预估费用；只有拿到真实 URL 才能声称生成成功。
- 内置估算为人民币 0.14 元/张，可用 `NANOCODEX_IMAGE_PRICE_CNY` 覆盖；执行前以工具返回的价格元数据为准。
- 优先使用设置中的阿里百炼 Workspace Key（`dashscope_workspace_key`）；兼容 `DASHSCOPE_API_KEY` 和 `vl_api_key`。
- 缺少密钥时明确提示用户配置，不能伪造图片链接；不要把密钥写入提示词、日志或产物。
