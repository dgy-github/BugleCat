//! Deterministic composition of named prompt sections.
//!
//! The assembler keeps prompt construction separate from the providers that
//! load each section. It is intentionally pure so callers can snapshot the
//! resulting model input without touching the filesystem or network.

#[derive(Debug, Clone)]
struct PromptSection {
    name: String,
    order: u16,
    sequence: usize,
    content: String,
}

/// Builds a system prompt from a base string and named, removable sections.
#[derive(Debug, Clone)]
pub struct PromptAssembler {
    base: String,
    sections: Vec<PromptSection>,
    next_sequence: usize,
}

impl PromptAssembler {
    /// Start an assembly with the unchanged base system prompt.
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            sections: Vec::new(),
            next_sequence: 0,
        }
    }

    /// Add or replace a named section at a deterministic order position.
    pub fn upsert(
        &mut self,
        name: impl Into<String>,
        order: u16,
        content: impl Into<String>,
    ) -> &mut Self {
        let name = name.into();
        self.sections.retain(|section| section.name != name);
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.sections.push(PromptSection {
            name,
            order,
            sequence,
            content: content.into(),
        });
        self
    }

    /// Remove a section by name and report whether it was present.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.sections.len();
        self.sections.retain(|section| section.name != name);
        before != self.sections.len()
    }

    /// Render the prompt, skipping empty sections and preserving base text.
    pub fn build(&self) -> String {
        let mut sections = self.sections.clone();
        sections.sort_by_key(|section| (section.order, section.sequence));

        let mut out = self.base.clone();
        for section in sections {
            if !section.content.trim().is_empty() {
                out.push_str("\n\n");
                out.push_str(section.content.trim());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::PromptAssembler;

    #[test]
    fn orders_named_sections_and_skips_empty_content() {
        let mut prompt = PromptAssembler::new("base");
        prompt
            .upsert("plan", 30, " plan ")
            .upsert("skills", 20, "skills")
            .upsert("empty", 10, " \n ")
            .upsert("instructions", 10, "instructions");

        assert_eq!(prompt.build(), "base\n\ninstructions\n\nskills\n\nplan");
    }

    #[test]
    fn replacement_and_removal_are_deterministic() {
        let mut prompt = PromptAssembler::new("base");
        prompt.upsert("mode", 10, "old");
        prompt.upsert("mode", 10, "new");
        assert_eq!(prompt.build(), "base\n\nnew");
        assert!(prompt.remove("mode"));
        assert!(!prompt.remove("mode"));
        assert_eq!(prompt.build(), "base");
    }
}
