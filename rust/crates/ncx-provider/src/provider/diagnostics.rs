//! Allowlisted request metadata: never log URLs, credentials or response bodies.
use super::*;

#[derive(Default)]
pub(super) struct RequestDiagnostic {
    pub attempts: u32,
    pub status: Option<u16>,
    pub kind: &'static str,
}

impl RequestDiagnostic {
    pub fn begin(&mut self) {
        self.attempts += 1;
        self.status = None;
        self.kind = "pending";
    }

    pub fn render(&self, model: &str, key: &str, elapsed: Duration) -> String {
        let model = if key.is_empty() {
            model.to_string()
        } else {
            model.replace(key, "[REDACTED]")
        };
        json!({"event":"provider_request", "model":model.chars().take(120).collect::<String>(),
            "attempts":self.attempts, "retries":self.attempts.saturating_sub(1),
            "http_status":self.status, "kind":self.kind,
            "elapsed_ms":elapsed.as_millis()})
        .to_string()
    }
}

pub(super) fn transport_kind(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() && error.is_connect() {
        "connect_timeout"
    } else if error.is_timeout() {
        "read_timeout"
    } else if error.is_connect() {
        "connect_error"
    } else if error.is_decode() {
        "decode_error"
    } else {
        "request_error"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_repair_diagnostic_redacts_key_in_model() {
        let d = RequestDiagnostic {
            attempts: 2,
            status: Some(503),
            kind: "http_error",
        };
        let text = d.render(
            "model-fixture-credential",
            "fixture-credential",
            Duration::from_millis(125),
        );
        assert!(!text.contains("fixture-credential"));
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["model"], "model-[REDACTED]");
        assert_eq!(value["http_status"], 503);
        assert_eq!(value["elapsed_ms"], 125);
    }

    #[test]
    fn self_repair_diagnostic_counts_only_retries() {
        let mut d = RequestDiagnostic::default();
        d.begin();
        let value: Value = serde_json::from_str(&d.render("model", "", Duration::ZERO)).unwrap();
        assert_eq!(value["attempts"], 1);
        assert_eq!(value["retries"], 0);
        d.begin();
        let value: Value = serde_json::from_str(&d.render("model", "", Duration::ZERO)).unwrap();
        assert_eq!(value["retries"], 1);
    }

    #[test]
    fn self_repair_diagnostic_clears_previous_http_status() {
        let mut d = RequestDiagnostic {
            attempts: 1,
            status: Some(502),
            kind: "http_error",
        };
        d.begin();
        let value: Value = serde_json::from_str(&d.render("model", "", Duration::ZERO)).unwrap();
        assert!(value["http_status"].is_null());
        assert_eq!(value["kind"], "pending");
    }
}
