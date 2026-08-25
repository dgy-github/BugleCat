---
name: dashscope-video
description: 使用阿里云百炼 Wan 经济模型生成短视频；需要文生视频时使用。
capability: video-generation
---

# 阿里百炼 Wan 视频

1. 调用 `generate_video`，传入 `prompt`、时长和画面尺寸。
2. 当前默认经济模型为 `wan2.1-t2v-turbo`，时长限制 1–10 秒。
3. 工具会等待异步任务完成，返回真实视频 URL、task_id、币种、单价来源和预估费用。没有 URL 时不得声称完成。
4. 当前内置估算为人民币 0.24 元/秒，可用 `NANOCODEX_VIDEO_PRICE_CNY_PER_SECOND` 覆盖；执行前以工具返回的价格元数据为准。
5. 缺少 `DASHSCOPE_API_KEY`/`vl_api_key` 时，明确提示用户配置，不能输出虚构视频。
