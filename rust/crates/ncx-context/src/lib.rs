//! Bounded, typed fragments and compaction contracts for model context.

pub trait ContextFragment {
    fn source(&self) -> &str;
    fn render(&self) -> String;
    fn max_chars(&self) -> usize;

    fn bounded_render(&self) -> String {
        self.render().chars().take(self.max_chars()).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextContextFragment {
    source: String,
    content: String,
    max_chars: usize,
}

impl TextContextFragment {
    pub fn new(source: impl Into<String>, content: impl Into<String>, max_chars: usize) -> Self {
        Self {
            source: source.into(),
            content: content.into(),
            max_chars,
        }
    }
}

impl ContextFragment for TextContextFragment {
    fn source(&self) -> &str {
        &self.source
    }

    fn render(&self) -> String {
        self.content.clone()
    }

    fn max_chars(&self) -> usize {
        self.max_chars
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextEditPolicy {
    pub enabled: bool,
    pub max_chars: usize,
    pub keep_recent_messages: usize,
    pub max_tool_result_chars: usize,
}

impl Default for ContextEditPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_chars: 120_000,
            keep_recent_messages: 30,
            max_tool_result_chars: 4_000,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContextEditStats {
    pub original_chars: usize,
    pub edited_chars: usize,
    pub compressed_tool_results: usize,
    pub dropped_messages: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fragment;

    impl ContextFragment for Fragment {
        fn source(&self) -> &str { "test" }
        fn render(&self) -> String { "abcdef".into() }
        fn max_chars(&self) -> usize { 3 }
    }

    #[test]
    fn every_fragment_has_a_hard_output_bound() {
        assert_eq!(Fragment.bounded_render(), "abc");
    }
}
