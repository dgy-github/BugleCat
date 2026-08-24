//! Bounded, typed fragments and compaction contracts for model context.

pub trait ContextFragment {
    fn source(&self) -> &str;
    fn render(&self) -> String;
    fn max_chars(&self) -> usize;

    fn bounded_render(&self) -> String {
        self.render().chars().take(self.max_chars()).collect()
    }
}

#[derive(Debug, Clone)]
struct ContextSection {
    source: String,
    order: u16,
    sequence: usize,
    content: String,
}

/// Deterministically assembles bounded context fragments into provider input.
#[derive(Debug, Clone)]
pub struct ContextAssembler {
    base: String,
    sections: Vec<ContextSection>,
    next_sequence: usize,
}

impl ContextAssembler {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            sections: Vec::new(),
            next_sequence: 0,
        }
    }

    pub fn upsert(
        &mut self,
        source: impl Into<String>,
        order: u16,
        content: impl Into<String>,
    ) -> &mut Self {
        let source = source.into();
        self.sections.retain(|section| section.source != source);
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.sections.push(ContextSection {
            source,
            order,
            sequence,
            content: content.into(),
        });
        self
    }

    pub fn upsert_fragment(&mut self, order: u16, fragment: &dyn ContextFragment) -> &mut Self {
        self.upsert(fragment.source(), order, fragment.bounded_render())
    }

    pub fn remove(&mut self, source: &str) -> bool {
        let before = self.sections.len();
        self.sections.retain(|section| section.source != source);
        before != self.sections.len()
    }

    pub fn build(&self) -> String {
        let mut sections = self.sections.clone();
        sections.sort_by_key(|section| (section.order, section.sequence));
        let mut output = self.base.clone();
        for section in sections {
            if !section.content.trim().is_empty() {
                output.push_str("\n\n");
                output.push_str(section.content.trim());
            }
        }
        output
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
pub struct ContextEntry {
    pub order: u16,
    pub fragment: TextContextFragment,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextService {
    entries: Vec<ContextEntry>,
}

impl ContextService {
    pub fn new(entries: Vec<ContextEntry>) -> Self {
        Self { entries }
    }

    pub fn entries(&self) -> &[ContextEntry] {
        &self.entries
    }

    pub fn assemble(&self, base: impl Into<String>) -> String {
        let mut assembler = ContextAssembler::new(base);
        for entry in &self.entries {
            assembler.upsert_fragment(entry.order, &entry.fragment);
        }
        assembler.build()
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
        fn source(&self) -> &str {
            "test"
        }
        fn render(&self) -> String {
            "abcdef".into()
        }
        fn max_chars(&self) -> usize {
            3
        }
    }

    #[test]
    fn every_fragment_has_a_hard_output_bound() {
        assert_eq!(Fragment.bounded_render(), "abc");
    }

    #[test]
    fn assembler_orders_replaces_and_bounds_fragments() {
        let mut assembler = ContextAssembler::new("base");
        assembler
            .upsert("mode", 30, "old")
            .upsert_fragment(20, &TextContextFragment::new("skills", "abcdef", 3))
            .upsert("mode", 30, "new")
            .upsert("empty", 10, "  ");
        assert_eq!(assembler.build(), "base\n\nabc\n\nnew");
        assert!(assembler.remove("mode"));
        assert_eq!(assembler.build(), "base\n\nabc");
    }

    #[test]
    fn context_service_is_an_executable_fragment_provider() {
        let service = ContextService::new(vec![ContextEntry {
            order: 10,
            fragment: TextContextFragment::new("project", "abcdef", 4),
        }]);
        assert_eq!(service.assemble("base"), "base\n\nabcd");
    }
}
