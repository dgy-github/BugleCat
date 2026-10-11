//! Unauthenticated HEAD only: zero model requests and no retry.
use ncx_config::{Overrides, load_config};
use serde_json::json;
use std::time::Duration;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let result = async {
        let config = load_config(Overrides::default()).map_err(|_| "config_unavailable")?;
        let base = url::Url::parse(&config.base_url).map_err(|_| "invalid_endpoint")?;
        if base.username() != "" || base.password().is_some() || base.query().is_some() {
            return Err("endpoint_userinfo_or_query");
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|_| "client_unavailable")?;
        let sent = client
            .head(format!("{}/models", config.base_url.trim_end_matches('/')))
            .send()
            .await;
        Ok(match sent {
            Ok(response) => json!({"status":"head_completed", "http_status":response.status().as_u16()}),
            Err(error) => json!({"status":"head_failed", "connect_error":error.is_connect(),
                "timeout":error.is_timeout(), "request_error":error.is_request()}),
        })
    }.await;
    let value = match result {
        Ok(mut value) => {
            value["auth_sent"] = json!(false);
            value["model_requests"] = json!(0);
            value["automatic_retries"] = json!(0);
            value
        }
        Err(reason) => json!({"status":reason,"auth_sent":false,"model_requests":0}),
    };
    println!("{value}");
}
