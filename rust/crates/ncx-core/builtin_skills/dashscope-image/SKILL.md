---
name: dashscope-image
description: 使用阿里云百炼通义万相生成图片；需要生成配图、插画或视觉素材时使用。
capability: image-generation
---

# 阿里百炼生图

1. 调用 `generate_image`，传入清晰的 `prompt`；需要排除内容时填写 `negative_prompt`。
2. 默认模型由媒体 Provider 管理，当前默认 `wan2.2-t2i-flash`，不要在提示词中伪造其他模型。
3. 工具返回 `urls`、模型、币种、单价来源和本次预估费用。只有拿到真实 URL 才能声称生成成功。
4. 当前内置估算为人民币 0.14 元/张，可用 `NANOCODEX_IMAGE_PRICE_CNY` 覆盖；执行前以工具返回的价格元数据为准。
5. 缺少 `DASHSCOPE_API_KEY`/`vl_api_key` 时，明确提示用户配置，不能伪造图片链接。
