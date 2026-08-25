//! ncx-provider — DeepSeek (OpenAI-compatible) chat-completions with tool calling.
//!
//! Rust port of `nanocodex/provider/`. The module is split into:
//!
//! * [`types`] — [`ModelResponse`] / [`ToolCall`] / [`ProviderError`] (port of `base.py`).
//! * [`request`] — pure request-body shaping: reasoning-effort translation and
//!   DeepSeek reasoning-replay sanitization (the bulk of the tested behavior).
//! * [`response`] — parse a completion JSON into a [`ModelResponse`]; usage
//!   normalization including DeepSeek's cache-accounting fields.
//! * [`provider`] — [`DeepSeekProvider`], the async HTTP client over `reqwest`.

pub mod api;
pub mod dashscope_media;
pub mod provider;
pub mod request;
pub mod response;
pub mod types;
pub mod web;

pub use api::{Provider, StreamDelta};
pub use dashscope_media::{
    DashScopeMediaProvider, MediaGenerationRequest, MediaGenerationResult, MediaKind,
    MediaProvider, DASHSCOPE_MEDIA_BASE_URL, DEFAULT_IMAGE_MODEL, DEFAULT_VIDEO_MODEL,
};
pub use provider::{stream_open_timeout_s, DeepSeekProvider};
pub use request::{build_body, is_deepseek_model};
pub use response::{extract_reasoning, parse_completion};
pub use types::{ModelResponse, ProviderError, ToolCall};
pub use web::{
    bing_rss_search, ddg_instant_answer, fetch_url, html_to_text, tavily_search, wikipedia_search,
};
