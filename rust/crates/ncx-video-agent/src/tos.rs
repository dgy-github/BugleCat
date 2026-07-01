use std::collections::BTreeMap;

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use time::format_description::FormatItem;
use time::macros::format_description;
use time::OffsetDateTime;

use crate::{Result, VideoAgentError};

const SERVICE: &str = "s3";
const ALGORITHM: &str = "AWS4-HMAC-SHA256";
const AMZ_DATE_FORMAT: &[FormatItem<'_>] =
    format_description!("[year][month][day]T[hour][minute][second]Z");
const DATE_FORMAT: &[FormatItem<'_>] = format_description!("[year][month][day]");

#[derive(Debug, Clone, PartialEq)]
pub struct TosConfig {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TosObjectRef {
    pub bucket: String,
    pub key: String,
    pub uri: String,
    pub content_hash: String,
    pub size_bytes: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TosRequest {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TosResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

pub trait TosTransport {
    fn send(&mut self, request: TosRequest) -> std::result::Result<TosResponse, String>;
}

pub struct ReqwestTosTransport {
    client: reqwest::blocking::Client,
}

impl ReqwestTosTransport {
    pub fn new() -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .build()
            .map_err(|err| VideoAgentError::Tos(format!("build HTTP client failed: {err}")))?;
        Ok(Self { client })
    }
}

impl TosTransport for ReqwestTosTransport {
    fn send(&mut self, request: TosRequest) -> std::result::Result<TosResponse, String> {
        let method = reqwest::Method::from_bytes(request.method.as_bytes())
            .map_err(|err| format!("invalid HTTP method {}: {err}", request.method))?;
        let mut builder = self.client.request(method, &request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        let response = builder
            .body(request.body)
            .send()
            .map_err(|err| format!("HTTP request failed: {err}"))?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_ascii_lowercase(),
                    value.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        let body = response
            .bytes()
            .map_err(|err| format!("read HTTP response body failed: {err}"))?
            .to_vec();
        Ok(TosResponse {
            status,
            headers,
            body,
        })
    }
}

pub struct TosClient<T> {
    config: TosConfig,
    transport: T,
    now: fn() -> OffsetDateTime,
}

impl TosConfig {
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(env_lookup)
    }

    pub fn new(
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
        endpoint: impl Into<String>,
        bucket: impl Into<String>,
        region: impl Into<String>,
    ) -> Result<Self> {
        let config = Self {
            access_key_id: access_key_id.into(),
            secret_access_key: secret_access_key.into(),
            endpoint: normalize_endpoint(&endpoint.into())?,
            bucket: bucket.into(),
            region: region.into(),
        };
        if config.access_key_id.trim().is_empty() {
            return Err(VideoAgentError::Tos("TOS access key is empty".to_string()));
        }
        if config.secret_access_key.trim().is_empty() {
            return Err(VideoAgentError::Tos("TOS secret key is empty".to_string()));
        }
        if config.bucket.trim().is_empty() {
            return Err(VideoAgentError::Tos("TOS bucket is empty".to_string()));
        }
        if config.region.trim().is_empty() {
            return Err(VideoAgentError::Tos("TOS region is empty".to_string()));
        }
        Ok(config)
    }

    fn from_lookup<F>(lookup: F) -> Result<Self>
    where
        F: Fn(&str) -> Option<String>,
    {
        let access_key_id = first_setting(
            &lookup,
            &["TOS_ACCESS_KEY_ID", "TOS_ACCESS_KEY", "AWS_ACCESS_KEY_ID"],
        )
        .ok_or_else(|| {
            missing_env(
                "TOS access key",
                &["TOS_ACCESS_KEY_ID", "TOS_ACCESS_KEY", "AWS_ACCESS_KEY_ID"],
            )
        })?;
        let secret_access_key = first_setting(
            &lookup,
            &[
                "TOS_SECRET_ACCESS_KEY",
                "TOS_SECRET_KEY",
                "AWS_SECRET_ACCESS_KEY",
            ],
        )
        .ok_or_else(|| {
            missing_env(
                "TOS secret key",
                &[
                    "TOS_SECRET_ACCESS_KEY",
                    "TOS_SECRET_KEY",
                    "AWS_SECRET_ACCESS_KEY",
                ],
            )
        })?;
        let endpoint = first_setting(&lookup, &["TOS_ENDPOINT", "AWS_ENDPOINT_URL"])
            .ok_or_else(|| missing_env("TOS endpoint", &["TOS_ENDPOINT", "AWS_ENDPOINT_URL"]))?;
        let bucket = first_setting(&lookup, &["TOS_BUCKET", "S3_BUCKET"])
            .ok_or_else(|| missing_env("TOS bucket", &["TOS_BUCKET", "S3_BUCKET"]))?;
        let endpoint = normalize_endpoint(&endpoint)?;
        let region = first_setting(&lookup, &["TOS_REGION", "AWS_REGION", "AWS_DEFAULT_REGION"])
            .or_else(|| parse_region_from_endpoint(&endpoint))
            .unwrap_or_else(|| "cn-beijing".to_string());

        Self::new(access_key_id, secret_access_key, endpoint, bucket, region)
    }
}

impl<T: TosTransport> TosClient<T> {
    pub fn new(config: TosConfig, transport: T) -> Self {
        Self {
            config,
            transport,
            now: OffsetDateTime::now_utc,
        }
    }

    pub fn put_object(
        &mut self,
        key: &str,
        body: &[u8],
        content_type: &str,
    ) -> Result<TosObjectRef> {
        let response = self.send_signed("PUT", key, body, Some(content_type))?;
        if !is_success(response.status) {
            return Err(VideoAgentError::Tos(format!(
                "put_object failed (HTTP {}): {}",
                response.status,
                preview_bytes(&response.body)
            )));
        }
        let content_hash = format!("sha256:{}", hex_sha256(body));
        Ok(TosObjectRef {
            bucket: self.config.bucket.clone(),
            key: key.to_string(),
            uri: format!("tos://{}/{}", self.config.bucket, key),
            content_hash,
            size_bytes: body.len(),
        })
    }

    pub fn get_object(&mut self, key: &str) -> Result<Vec<u8>> {
        let response = self.send_signed("GET", key, &[], None)?;
        if response.status != 200 {
            return Err(VideoAgentError::Tos(format!(
                "get_object failed (HTTP {}): {}",
                response.status,
                preview_bytes(&response.body)
            )));
        }
        Ok(response.body)
    }

    pub fn delete_object(&mut self, key: &str) -> Result<()> {
        let response = self.send_signed("DELETE", key, &[], None)?;
        if is_success(response.status) || response.status == 404 {
            return Ok(());
        }
        Err(VideoAgentError::Tos(format!(
            "delete_object failed (HTTP {}): {}",
            response.status,
            preview_bytes(&response.body)
        )))
    }

    fn send_signed(
        &mut self,
        method: &str,
        key: &str,
        body: &[u8],
        content_type: Option<&str>,
    ) -> Result<TosResponse> {
        let key = key.trim_start_matches('/');
        if key.is_empty() {
            return Err(VideoAgentError::Tos("TOS object key is empty".to_string()));
        }
        let now = (self.now)();
        let signed = sign_request(&self.config, method, key, body, content_type, now)?;
        self.transport.send(signed).map_err(VideoAgentError::Tos)
    }
}

fn sign_request(
    config: &TosConfig,
    method: &str,
    key: &str,
    body: &[u8],
    content_type: Option<&str>,
    now: OffsetDateTime,
) -> Result<TosRequest> {
    let host = endpoint_authority(&config.endpoint)?;
    let canonical_uri = canonical_uri(&config.endpoint, &config.bucket, key)?;
    let url = format!("{}{}", endpoint_origin(&config.endpoint)?, canonical_uri);
    let amz_date = now.format(AMZ_DATE_FORMAT).map_err(|err| {
        VideoAgentError::Tos(format!("format x-amz-date for TOS signature failed: {err}"))
    })?;
    let date = now.format(DATE_FORMAT).map_err(|err| {
        VideoAgentError::Tos(format!("format date for TOS signature failed: {err}"))
    })?;
    let payload_hash = hex_sha256(body);

    let mut headers = BTreeMap::new();
    if let Some(content_type) = content_type.filter(|value| !value.trim().is_empty()) {
        headers.insert("content-type".to_string(), content_type.trim().to_string());
    }
    headers.insert("host".to_string(), host);
    headers.insert("x-amz-content-sha256".to_string(), payload_hash.clone());
    headers.insert("x-amz-date".to_string(), amz_date.clone());

    let canonical_headers = headers
        .iter()
        .map(|(name, value)| format!("{}:{}\n", name.to_ascii_lowercase(), trim_header(value)))
        .collect::<String>();
    let signed_headers = headers
        .keys()
        .map(|name| name.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(";");
    let canonical_request = format!(
        "{method}\n{canonical_uri}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );
    let credential_scope = format!("{date}/{}/{SERVICE}/aws4_request", config.region);
    let string_to_sign = format!(
        "{ALGORITHM}\n{amz_date}\n{credential_scope}\n{}",
        hex_sha256(canonical_request.as_bytes())
    );
    let signing_key = signing_key(&config.secret_access_key, &date, &config.region);
    let signature = hmac_sha256_hex(&signing_key, string_to_sign.as_bytes());
    headers.insert(
        "authorization".to_string(),
        format!(
            "{ALGORITHM} Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
            config.access_key_id
        ),
    );

    Ok(TosRequest {
        method: method.to_string(),
        url,
        headers,
        body: body.to_vec(),
    })
}

fn signing_key(secret: &str, date: &str, region: &str) -> Vec<u8> {
    let date_key = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let region_key = hmac_sha256(&date_key, region.as_bytes());
    let service_key = hmac_sha256(&region_key, SERVICE.as_bytes());
    hmac_sha256(&service_key, b"aws4_request")
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hmac_sha256_hex(key: &[u8], data: &[u8]) -> String {
    hex_bytes(&hmac_sha256(key, data))
}

fn hex_sha256(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    hex_bytes(&digest)
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn canonical_uri(endpoint: &str, bucket: &str, key: &str) -> Result<String> {
    let endpoint_path = endpoint_path(endpoint)?;
    let mut segments = Vec::new();
    segments.extend(
        endpoint_path
            .trim_matches('/')
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(percent_encode_path_segment),
    );
    segments.push(percent_encode_path_segment(bucket.trim_matches('/')));
    segments.extend(key.split('/').map(percent_encode_path_segment));
    Ok(format!("/{}", segments.join("/")))
}

fn percent_encode_path_segment(segment: &str) -> String {
    let mut encoded = String::new();
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn normalize_endpoint(endpoint: &str) -> Result<String> {
    let trimmed = endpoint.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(VideoAgentError::Tos("TOS endpoint is empty".to_string()));
    }
    let normalized = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let _ = endpoint_authority(&normalized)?;
    Ok(normalized)
}

fn endpoint_origin(endpoint: &str) -> Result<String> {
    let scheme_end = endpoint
        .find("://")
        .ok_or_else(|| VideoAgentError::Tos(format!("TOS endpoint needs a scheme: {endpoint}")))?;
    let rest = &endpoint[scheme_end + 3..];
    let authority_len = rest.find('/').unwrap_or(rest.len());
    if authority_len == 0 {
        return Err(VideoAgentError::Tos(format!(
            "TOS endpoint has no host: {endpoint}"
        )));
    }
    Ok(endpoint[..scheme_end + 3 + authority_len].to_string())
}

fn endpoint_authority(endpoint: &str) -> Result<String> {
    let origin = endpoint_origin(endpoint)?;
    Ok(origin
        .split_once("://")
        .map(|(_, authority)| authority.to_string())
        .unwrap_or(origin))
}

fn endpoint_path(endpoint: &str) -> Result<String> {
    let scheme_end = endpoint
        .find("://")
        .ok_or_else(|| VideoAgentError::Tos(format!("TOS endpoint needs a scheme: {endpoint}")))?;
    let rest = &endpoint[scheme_end + 3..];
    Ok(rest
        .find('/')
        .map(|path_start| rest[path_start..].to_string())
        .unwrap_or_default())
}

fn parse_region_from_endpoint(endpoint: &str) -> Option<String> {
    let host = endpoint_authority(endpoint).ok()?;
    let host = host.split(':').next().unwrap_or(&host).to_ascii_lowercase();
    let first = host.split('.').next()?;
    if let Some(region) = first.strip_prefix("tos-") {
        if !region.is_empty() {
            return Some(region.to_string());
        }
    }
    None
}

fn trim_header(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn first_setting<F>(lookup: &F, keys: &[&str]) -> Option<String>
where
    F: Fn(&str) -> Option<String>,
{
    keys.iter()
        .filter_map(|key| lookup(key))
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

fn env_lookup(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

fn missing_env(name: &str, keys: &[&str]) -> VideoAgentError {
    VideoAgentError::Tos(format!("{name} missing one of: {}", keys.join(", ")))
}

fn is_success(status: u16) -> bool {
    (200..300).contains(&status)
}

fn preview_bytes(bytes: &[u8]) -> String {
    const MAX: usize = 300;
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= MAX {
        text.to_string()
    } else {
        format!("{}...", &text[..MAX])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeTransport {
        objects: BTreeMap<String, Vec<u8>>,
        calls: Vec<TosRequest>,
    }

    impl TosTransport for FakeTransport {
        fn send(&mut self, request: TosRequest) -> std::result::Result<TosResponse, String> {
            let key = request
                .url
                .split("/bucket/")
                .nth(1)
                .unwrap_or_default()
                .to_string();
            let response = match request.method.as_str() {
                "PUT" => {
                    self.objects.insert(key, request.body.clone());
                    TosResponse {
                        status: 200,
                        headers: BTreeMap::new(),
                        body: Vec::new(),
                    }
                }
                "GET" => match self.objects.get(&key) {
                    Some(body) => TosResponse {
                        status: 200,
                        headers: BTreeMap::new(),
                        body: body.clone(),
                    },
                    None => TosResponse {
                        status: 404,
                        headers: BTreeMap::new(),
                        body: b"not found".to_vec(),
                    },
                },
                "DELETE" => {
                    self.objects.remove(&key);
                    TosResponse {
                        status: 204,
                        headers: BTreeMap::new(),
                        body: Vec::new(),
                    }
                }
                other => return Err(format!("unexpected method {other}")),
            };
            self.calls.push(request);
            Ok(response)
        }
    }

    fn fixed_now() -> OffsetDateTime {
        time::macros::datetime!(2026-06-30 12:34:56 UTC)
    }

    fn test_client() -> TosClient<FakeTransport> {
        let config = TosConfig::new(
            "ak-test",
            "sk-test",
            "https://tos-cn-beijing.volces.com",
            "bucket",
            "cn-beijing",
        )
        .unwrap();
        TosClient {
            config,
            transport: FakeTransport::default(),
            now: fixed_now,
        }
    }

    #[test]
    fn tos_put_get_delete_roundtrip_uses_sigv4_headers() {
        let mut client = test_client();
        let object = client
            .put_object("p1 smoke/a+b.txt", b"hello", "text/plain")
            .unwrap();
        assert_eq!(object.uri, "tos://bucket/p1 smoke/a+b.txt");
        assert_eq!(
            object.content_hash,
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );

        let body = client.get_object("p1 smoke/a+b.txt").unwrap();
        assert_eq!(body, b"hello");
        client.delete_object("p1 smoke/a+b.txt").unwrap();

        let put = &client.transport.calls[0];
        assert_eq!(put.method, "PUT");
        assert!(put.url.ends_with("/bucket/p1%20smoke/a%2Bb.txt"));
        assert_eq!(
            put.headers.get("x-amz-date").map(String::as_str),
            Some("20260630T123456Z")
        );
        let auth = put.headers.get("authorization").unwrap();
        assert!(auth.contains("Credential=ak-test/20260630/cn-beijing/s3/aws4_request"));
        assert!(auth.contains("SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date"));
    }

    #[test]
    fn tos_config_normalizes_endpoint_and_parses_region() {
        let config =
            TosConfig::new("ak", "sk", "tos-cn-beijing.volces.com", "b", "cn-beijing").unwrap();
        assert_eq!(config.endpoint, "https://tos-cn-beijing.volces.com");
        assert_eq!(
            parse_region_from_endpoint("https://tos-cn-shanghai.volces.com"),
            Some("cn-shanghai".to_string())
        );
    }

    #[test]
    fn tos_config_lookup_supports_aws_aliases_and_region_inference() {
        let values = BTreeMap::from([
            ("AWS_ACCESS_KEY_ID", " ak-from-aws "),
            ("AWS_SECRET_ACCESS_KEY", " sk-from-aws "),
            ("AWS_ENDPOINT_URL", " tos-cn-shanghai.volces.com "),
            ("S3_BUCKET", " bucket-from-s3 "),
        ]);

        let config = TosConfig::from_lookup(|key| values.get(key).map(ToString::to_string))
            .expect("TOS config from AWS-compatible aliases");

        assert_eq!(config.access_key_id, "ak-from-aws");
        assert_eq!(config.secret_access_key, "sk-from-aws");
        assert_eq!(config.endpoint, "https://tos-cn-shanghai.volces.com");
        assert_eq!(config.bucket, "bucket-from-s3");
        assert_eq!(config.region, "cn-shanghai");
    }

    #[test]
    fn tos_config_lookup_prefers_tos_aliases_and_explicit_region() {
        let values = BTreeMap::from([
            ("TOS_ACCESS_KEY_ID", " ak-from-tos "),
            ("AWS_ACCESS_KEY_ID", "ak-from-aws"),
            ("TOS_SECRET_ACCESS_KEY", " sk-from-tos "),
            ("AWS_SECRET_ACCESS_KEY", "sk-from-aws"),
            ("TOS_ENDPOINT", " tos-cn-beijing.volces.com "),
            ("AWS_ENDPOINT_URL", "tos-cn-shanghai.volces.com"),
            ("TOS_BUCKET", " bucket-from-tos "),
            ("S3_BUCKET", "bucket-from-s3"),
            ("TOS_REGION", " cn-guangzhou "),
        ]);

        let config = TosConfig::from_lookup(|key| values.get(key).map(ToString::to_string))
            .expect("TOS config from preferred TOS aliases");

        assert_eq!(config.access_key_id, "ak-from-tos");
        assert_eq!(config.secret_access_key, "sk-from-tos");
        assert_eq!(config.endpoint, "https://tos-cn-beijing.volces.com");
        assert_eq!(config.bucket, "bucket-from-tos");
        assert_eq!(config.region, "cn-guangzhou");
    }

    #[test]
    fn tos_rejects_empty_key() {
        let mut client = test_client();
        let err = client.put_object("/", b"x", "text/plain").unwrap_err();
        assert!(err.to_string().contains("object key is empty"));
    }
}
