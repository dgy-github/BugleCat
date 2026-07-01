pub const SEEDANCE_PRICING_AS_OF: &str = "2026-06-10";
pub const SEEDANCE_CNY_PER_M_NO_VIDEO_INPUT: f64 = 37.0;
pub const SEEDANCE_CNY_PER_M_WITH_VIDEO_INPUT: f64 = 22.0;
pub const SEEDANCE_TOKENS_PER_SECOND_720P: f64 = 21_770.0;

const PER_TOKENS: f64 = 1_000_000.0;

pub fn estimate_seedance_cost_cny(total_seconds: f64, has_video_input: bool) -> f64 {
    if total_seconds <= 0.0 {
        return 0.0;
    }
    let rate = if has_video_input {
        SEEDANCE_CNY_PER_M_WITH_VIDEO_INPUT
    } else {
        SEEDANCE_CNY_PER_M_NO_VIDEO_INPUT
    };
    total_seconds * SEEDANCE_TOKENS_PER_SECOND_720P * rate / PER_TOKENS
}

pub fn seedance_cost_cny(usage: &serde_json::Value, has_video_input: bool) -> Option<f64> {
    let total = usage.get("total_tokens")?.as_i64()?;
    if total <= 0 {
        return None;
    }
    let rate = if has_video_input {
        SEEDANCE_CNY_PER_M_WITH_VIDEO_INPUT
    } else {
        SEEDANCE_CNY_PER_M_NO_VIDEO_INPUT
    };
    Some(total as f64 * rate / PER_TOKENS)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn seedance_cost_uses_total_tokens_and_video_input_rate() {
        let usage = json!({"total_tokens": 108900});
        let no_video = seedance_cost_cny(&usage, false).unwrap();
        let with_video = seedance_cost_cny(&usage, true).unwrap();

        assert!((no_video - 4.0293).abs() < 0.0001);
        assert!((with_video - 2.3958).abs() < 0.0001);
    }

    #[test]
    fn seedance_cost_is_none_without_positive_tokens() {
        assert_eq!(seedance_cost_cny(&json!({}), false), None);
        assert_eq!(seedance_cost_cny(&json!({"total_tokens": 0}), false), None);
    }

    #[test]
    fn seedance_estimate_matches_measured_tokens_per_second() {
        let estimate = estimate_seedance_cost_cny(5.0, false);
        assert!((estimate - 4.02745).abs() < 0.0001);
        assert_eq!(estimate_seedance_cost_cny(0.0, false), 0.0);
    }
}
