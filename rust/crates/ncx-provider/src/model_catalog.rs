//! Provider-neutral model-directory discovery.

use std::collections::HashSet;
use std::time::Duration;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_MODELS: usize = 1000;

#[derive(Clone, Debug, PartialEq)]
pub struct ProviderCatalogRequest {
    pub base_url: String,
    pub protocol: String,
    pub api_key: Option<String>,
    pub timeout: Duration,
}

impl ProviderCatalogRequest {
    pub fn new(
        base_url: impl Into<String>,
        protocol: impl Into<String>,
        api_key: Option<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            protocol: protocol.into(),
            api_key,
            timeout: Duration::from_secs(15),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct DiscoveredProviderModel {
    pub id: String,
    pub name: String,
    pub context_length: Option<u64>,
    pub input_price_per_million: Option<f64>,
    pub output_price_per_million: Option<f64>,
}

pub trait ProviderCatalogClient: Send + Sync {
    fn discover(
        &self,
        request: &ProviderCatalogRequest,
    ) -> Result<Vec<DiscoveredProviderModel>, String>;
}

#[derive(Clone, Debug, Default)]
pub struct HttpProviderCatalogClient;

impl ProviderCatalogClient for HttpProviderCatalogClient {
    fn discover(
        &self,
        request: &ProviderCatalogRequest,
    ) -> Result<Vec<DiscoveredProviderModel>, String> {
        let endpoint = catalog_endpoint(&request.base_url)?;
        let timeout = request
            .timeout
            .clamp(Duration::from_secs(1), Duration::from_secs(60));
        let client = Client::builder()
            .timeout(timeout)
            // Never forward a provider credential to a redirected host.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| format!("模型目录客户端初始化失败：{error}"))?;
        let mut http = client
            .get(endpoint)
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(key) = request
            .api_key
            .as_deref()
            .filter(|key| !key.trim().is_empty())
        {
            http = if request.protocol == "anthropic" {
                http.header("x-api-key", key)
                    .header("anthropic-version", "2023-06-01")
            } else {
                http.bearer_auth(key)
            };
        }
        let response = http
            .send()
            .map_err(|error| format!("模型目录连接失败：{error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "模型目录请求失败：HTTP {}",
                response.status().as_u16()
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err("模型目录响应超过 1 MiB 限制".to_string());
        }
        let bytes = response
            .bytes()
            .map_err(|error| format!("模型目录读取失败：{error}"))?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err("模型目录响应超过 1 MiB 限制".to_string());
        }
        let payload: Value =
            serde_json::from_slice(&bytes).map_err(|error| format!("模型目录格式无效：{error}"))?;
        let models = parse_catalog_models(&payload);
        if models.is_empty() {
            return Err("接口已连接，但没有返回可用模型；可手动填写模型 ID".to_string());
        }
        Ok(models)
    }
}

pub fn catalog_endpoint(base_url: &str) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(base_url.trim()).map_err(|_| "Base URL 无效".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.cannot_be_a_base() {
        return Err("Base URL 只支持 HTTP/HTTPS".to_string());
    }
    let mut segments = url
        .path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if segments.last().copied() == Some("completions")
        && segments.get(segments.len().saturating_sub(2)).copied() == Some("chat")
    {
        segments.truncate(segments.len().saturating_sub(2));
    } else if matches!(segments.last().copied(), Some("responses" | "messages")) {
        segments.pop();
    }
    if segments.last().copied() != Some("models") {
        segments.push("models");
    }
    url.set_path(&format!("/{}", segments.join("/")));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

pub fn parse_catalog_models(payload: &Value) -> Vec<DiscoveredProviderModel> {
    let rows = payload
        .get("data")
        .or_else(|| payload.get("models"))
        .and_then(Value::as_array)
        .or_else(|| payload.as_array())
        .into_iter()
        .flatten();
    let mut seen = HashSet::new();
    rows.filter_map(normalize_model)
        .filter(|model| seen.insert(model.id.clone()))
        .take(MAX_MODELS)
        .collect()
}

fn normalize_model(value: &Value) -> Option<DiscoveredProviderModel> {
    let id = value
        .as_str()
        .or_else(|| value.get("id").and_then(Value::as_str))?
        .trim();
    if !valid_model_id(id) {
        return None;
    }
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(id)
        .to_string();
    Some(DiscoveredProviderModel {
        id: id.to_string(),
        name,
        context_length: value.get("context_length").and_then(Value::as_u64),
        input_price_per_million: price_per_million(value, "prompt"),
        output_price_per_million: price_per_million(value, "completion"),
    })
}

fn price_per_million(value: &Value, field: &str) -> Option<f64> {
    value
        .get("pricing")
        .and_then(|pricing| pricing.get(field))
        .and_then(|price| {
            price
                .as_str()
                .and_then(|price| price.parse::<f64>().ok())
                .or_else(|| price.as_f64())
        })
        .filter(|price| price.is_finite() && *price >= 0.0)
        .map(|price| price * 1_000_000.0)
}

fn valid_model_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':' | '/')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn serve_once(status: &str, body: &str) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 1024];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..read]);
                if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8(bytes).unwrap()
        });
        (format!("http://{address}/v1"), handle)
    }

    #[test]
    fn endpoint_preserves_prefix_and_replaces_known_request_suffixes() {
        assert_eq!(
            catalog_endpoint("https://relay.example/v1/")
                .unwrap()
                .as_str(),
            "https://relay.example/v1/models"
        );
        assert_eq!(
            catalog_endpoint("https://relay.example/api/v1/chat/completions?x=1")
                .unwrap()
                .as_str(),
            "https://relay.example/api/v1/models"
        );
        assert!(catalog_endpoint("file:///tmp/provider").is_err());
    }

    #[test]
    fn parser_normalizes_common_shapes_prices_and_deduplicates() {
        let payload = serde_json::json!({"data":[
            {"id":"gpt-5.6-sol","name":"GPT 5.6 Sol","context_length":1000000,"pricing":{"prompt":"0.000001","completion":0.000002}},
            {"id":"gpt-5.6-sol"}, {"id":"bad id"}
        ]});
        let models = parse_catalog_models(&payload);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gpt-5.6-sol");
        assert_eq!(models[0].input_price_per_million, Some(1.0));
        assert_eq!(models[0].output_price_per_million, Some(2.0));
    }

    #[test]
    fn parser_accepts_models_and_root_array_variants() {
        assert_eq!(
            parse_catalog_models(&serde_json::json!({"models":["claude-sonnet-4-5"]}))[0].id,
            "claude-sonnet-4-5"
        );
        assert_eq!(
            parse_catalog_models(&serde_json::json!([{"id":"deepseek-v4"}]))[0].id,
            "deepseek-v4"
        );
    }

    #[test]
    fn http_client_uses_protocol_specific_auth_and_never_echoes_error_body() {
        let client = HttpProviderCatalogClient;
        let (openai_url, openai_request) =
            serve_once("200 OK", r#"{"data":[{"id":"gpt-5.6-sol"}]}"#);
        client
            .discover(&ProviderCatalogRequest::new(
                openai_url,
                "openai",
                Some("openai-test-secret".into()),
            ))
            .unwrap();
        let openai_request = openai_request.join().unwrap().to_ascii_lowercase();
        assert!(openai_request.starts_with("get /v1/models "));
        assert!(openai_request.contains("authorization: bearer openai-test-secret"));
        assert!(!openai_request.contains("x-api-key:"));

        let (anthropic_url, anthropic_request) =
            serve_once("200 OK", r#"{"data":[{"id":"claude-sonnet-4-5"}]}"#);
        client
            .discover(&ProviderCatalogRequest::new(
                anthropic_url,
                "anthropic",
                Some("anthropic-test-secret".into()),
            ))
            .unwrap();
        let anthropic_request = anthropic_request.join().unwrap().to_ascii_lowercase();
        assert!(anthropic_request.contains("x-api-key: anthropic-test-secret"));
        assert!(anthropic_request.contains("anthropic-version: 2023-06-01"));
        assert!(!anthropic_request.contains("authorization:"));

        let (error_url, _) = serve_once("403 Forbidden", r#"{"error":"private upstream detail"}"#);
        let error = client
            .discover(&ProviderCatalogRequest::new(
                error_url,
                "openai",
                Some("never-return-this".into()),
            ))
            .unwrap_err();
        assert!(error.contains("HTTP 403"));
        assert!(!error.contains("private upstream detail"));
        assert!(!error.contains("never-return-this"));
    }
}
