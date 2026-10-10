//! Validity and conflict tracking for standing instructions.
//!
//! The question this answers is not "what did the user say" — the session log
//! already holds that — but "what is still binding right now". A later
//! instruction can narrow, widen or revoke an earlier one, and two instructions
//! can genuinely disagree. When the ledger cannot decide, it reports a conflict
//! rather than picking the one that happens to match more keywords.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Polarity {
    Allow,
    Forbid,
    /// Withdraws an earlier instruction without asserting the opposite.
    Revoke,
}

/// Where an instruction came from. Long-term memory records what was true in
/// some past session; it must never override what the user said in this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    LongTermMemory,
    ProjectConfig,
    CurrentSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionStatus {
    Active,
    Superseded,
    Revoked,
    /// Still in force, but it contradicts another active instruction and the
    /// ledger could not decide which wins.
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instruction {
    pub id: String,
    pub text: String,
    pub polarity: Polarity,
    /// Optional area the instruction applies to, e.g. a path prefix.
    pub scope: Option<String>,
    /// Normalized content terms identifying what the instruction is about.
    pub subject: Vec<String>,
    pub origin: Origin,
    pub created_at: u64,
    pub supersedes: Vec<String>,
    pub revoked_by: Option<String>,
    pub status: InstructionStatus,
    /// Ids of active instructions this one contradicts without resolving.
    pub conflicts_with: Vec<String>,
}

/// Parse polarity and subject from free text. Chinese and English negation are
/// both recognized; anything not marked negative or withdrawing is permissive.
pub fn parse(text: &str) -> (Polarity, Vec<String>) {
    let lower = text.to_lowercase();
    const REVOKE: &[&str] = &[
        "取消",
        "撤销",
        "作废",
        "忽略之前",
        "不用管之前",
        "之前的不算",
        "revoke",
        "disregard",
        "ignore the previous",
        "never mind",
    ];
    const FORBID: &[&str] = &[
        "不要", "不能", "禁止", "不得", "不准", "别再", "别去", "勿", "don't", "do not", "never",
        "must not", "cannot", "avoid",
    ];
    let polarity = if REVOKE.iter().any(|needle| lower.contains(needle)) {
        Polarity::Revoke
    } else if FORBID.iter().any(|needle| lower.contains(needle)) {
        Polarity::Forbid
    } else {
        Polarity::Allow
    };
    (polarity, subject_terms(text, polarity))
}

/// Content terms with polarity and filler words removed, so "不要提交代码" and
/// "可以提交代码" resolve to the same subject.
fn subject_terms(text: &str, polarity: Polarity) -> Vec<String> {
    let mut cleaned = text.to_lowercase();
    let markers: &[&str] = &[
        "不要",
        "不能",
        "禁止",
        "不得",
        "不准",
        "别再",
        "别去",
        "取消",
        "撤销",
        "作废",
        "忽略之前",
        "不用管之前",
        "之前的不算",
        "可以",
        "允许",
        "请",
        "don't",
        "do not",
        "never",
        "must not",
        "cannot",
        "avoid",
        "revoke",
        "disregard",
        "ignore the previous",
        "never mind",
        "you may",
        "go ahead",
        "please",
    ];
    for marker in markers {
        cleaned = cleaned.replace(marker, " ");
    }
    let _ = polarity;
    let mut terms: Vec<String> = cleaned
        .split(|c: char| !c.is_alphanumeric() && !is_cjk(c))
        .flat_map(split_cjk_bigrams)
        .filter(|term| term.chars().count() >= 2)
        .collect();
    terms.sort();
    terms.dedup();
    terms
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF)
}

/// CJK text has no spaces, so a run of han characters becomes overlapping
/// bigrams. That is enough to tell "提交代码" from "删除日志" without a tokenizer.
fn split_cjk_bigrams(chunk: &str) -> Vec<String> {
    let chars: Vec<char> = chunk.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    if !chars.iter().any(|c| is_cjk(*c)) {
        return vec![chunk.to_string()];
    }
    if chars.len() < 2 {
        return vec![chunk.to_string()];
    }
    chars
        .windows(2)
        .map(|pair| pair.iter().collect::<String>())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeRelation {
    Same,
    /// The left scope covers the right one.
    Covers,
    CoveredBy,
    /// Neither covers the other; a decision cannot be derived from scope alone.
    Incomparable,
}

fn scope_relation(left: Option<&str>, right: Option<&str>) -> ScopeRelation {
    match (left, right) {
        (None, None) => ScopeRelation::Same,
        // An unscoped instruction applies everywhere, so it covers a scoped one.
        (None, Some(_)) => ScopeRelation::Covers,
        (Some(_), None) => ScopeRelation::CoveredBy,
        (Some(a), Some(b)) => {
            let a = a.trim_end_matches(['/', '\\']);
            let b = b.trim_end_matches(['/', '\\']);
            if a == b {
                ScopeRelation::Same
            } else if b.starts_with(&format!("{a}/")) {
                ScopeRelation::Covers
            } else if a.starts_with(&format!("{b}/")) {
                ScopeRelation::CoveredBy
            } else {
                ScopeRelation::Incomparable
            }
        }
    }
}

/// True when two subjects refer to the same thing. Exposed so memory recall can
/// tell whether a stored note is about what a current instruction just changed.
pub fn subjects_overlap(left: &[String], right: &[String]) -> bool {
    same_subject(left, right)
}

fn same_subject(left: &[String], right: &[String]) -> bool {
    if left.is_empty() || right.is_empty() {
        return false;
    }
    let shared = left.iter().filter(|term| right.contains(term)).count();
    // Require real overlap with the smaller subject, not one incidental word.
    shared * 2 >= left.len().min(right.len())
}

#[derive(Debug, Default, Clone)]
pub struct InstructionLedger {
    instructions: Vec<Instruction>,
    next_id: usize,
}

impl InstructionLedger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn all(&self) -> &[Instruction] {
        &self.instructions
    }

    /// Instructions still binding. Conflicted ones are included: an unresolved
    /// contradiction does not make either side disappear.
    pub fn active(&self) -> Vec<&Instruction> {
        self.instructions
            .iter()
            .filter(|item| {
                matches!(
                    item.status,
                    InstructionStatus::Active | InstructionStatus::Conflicted
                )
            })
            .collect()
    }

    /// Unresolved contradictions, as id pairs. A caller seeing a non-empty list
    /// must ask the user rather than choosing.
    pub fn conflicts(&self) -> Vec<(String, String)> {
        let mut pairs = Vec::new();
        for item in &self.instructions {
            if item.status != InstructionStatus::Conflicted {
                continue;
            }
            for other in &item.conflicts_with {
                let pair = if item.id < *other {
                    (item.id.clone(), other.clone())
                } else {
                    (other.clone(), item.id.clone())
                };
                if !pairs.contains(&pair) {
                    pairs.push(pair);
                }
            }
        }
        pairs
    }

    pub fn record(
        &mut self,
        text: &str,
        scope: Option<String>,
        origin: Origin,
        created_at: u64,
    ) -> String {
        let (polarity, subject) = parse(text);
        self.next_id += 1;
        let id = format!("i{}", self.next_id);
        let mut incoming = Instruction {
            id: id.clone(),
            text: text.to_string(),
            polarity,
            scope,
            subject,
            origin,
            created_at,
            supersedes: Vec::new(),
            revoked_by: None,
            status: InstructionStatus::Active,
            conflicts_with: Vec::new(),
        };
        self.resolve_against_existing(&mut incoming);
        self.instructions.push(incoming);
        id
    }

    /// Host parsed an explicit instruction with an exact subject. This avoids
    /// treating keyword overlap or quoted prose as permission to replace it.
    pub fn record_exact(
        &mut self,
        text: &str,
        scope: Option<String>,
        polarity: Polarity,
        subject: String,
        created_at: u64,
    ) -> String {
        self.next_id += 1;
        let id = format!("i{}", self.next_id);
        let mut incoming = Instruction {
            id: id.clone(),
            text: text.into(),
            polarity,
            scope,
            subject: vec![subject],
            origin: Origin::CurrentSession,
            created_at,
            supersedes: vec![],
            revoked_by: None,
            status: InstructionStatus::Active,
            conflicts_with: vec![],
        };
        self.resolve_against_existing(&mut incoming);
        self.instructions.push(incoming);
        id
    }

    fn resolve_against_existing(&mut self, incoming: &mut Instruction) {
        let mut newly_conflicted = Vec::new();
        for existing in self.instructions.iter_mut() {
            if existing.status == InstructionStatus::Revoked
                || existing.status == InstructionStatus::Superseded
            {
                continue;
            }
            if !same_subject(&existing.subject, &incoming.subject) {
                continue;
            }
            let relation = scope_relation(incoming.scope.as_deref(), existing.scope.as_deref());

            if incoming.polarity == Polarity::Revoke {
                // A withdrawal only reaches what it actually covers.
                if matches!(relation, ScopeRelation::Same | ScopeRelation::Covers) {
                    existing.status = InstructionStatus::Revoked;
                    existing.revoked_by = Some(incoming.id.clone());
                    existing.conflicts_with.clear();
                    incoming.supersedes.push(existing.id.clone());
                }
                continue;
            }

            if existing.polarity == incoming.polarity {
                // Agreement: the narrower one is redundant, not contradictory.
                continue;
            }

            // Past sessions never override what the user is saying now, however
            // recent the memory record claims to be.
            if incoming.origin < existing.origin {
                incoming.status = InstructionStatus::Superseded;
                incoming.revoked_by = None;
                return;
            }
            if existing.origin < incoming.origin {
                existing.status = InstructionStatus::Superseded;
                incoming.supersedes.push(existing.id.clone());
                continue;
            }

            match relation {
                // Same or wider scope, same origin, later in time: a genuine
                // replacement of the earlier rule.
                ScopeRelation::Same | ScopeRelation::Covers
                    if incoming.created_at >= existing.created_at =>
                {
                    existing.status = InstructionStatus::Superseded;
                    incoming.supersedes.push(existing.id.clone());
                }
                // A narrower exception does not cancel the broader rule, and two
                // unrelated scopes cannot be ordered at all. Both remain in
                // force and the disagreement is surfaced.
                _ => {
                    existing.status = InstructionStatus::Conflicted;
                    existing.conflicts_with.push(incoming.id.clone());
                    newly_conflicted.push(existing.id.clone());
                }
            }
        }
        if !newly_conflicted.is_empty() {
            incoming.status = InstructionStatus::Conflicted;
            incoming.conflicts_with.extend(newly_conflicted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_chinese_and_english_negation_as_prohibitions() {
        assert_eq!(parse("不要提交代码").0, Polarity::Forbid);
        assert_eq!(parse("禁止删除 releases").0, Polarity::Forbid);
        assert_eq!(parse("Do not push to main").0, Polarity::Forbid);
        assert_eq!(parse("可以提交代码").0, Polarity::Allow);
        assert_eq!(parse("取消之前不要提交的要求").0, Polarity::Revoke);
    }

    #[test]
    fn a_later_opposite_instruction_supersedes_and_keeps_the_chain() {
        let mut ledger = InstructionLedger::new();
        let old = ledger.record("不要提交代码", None, Origin::CurrentSession, 1);
        let new = ledger.record("可以提交代码了", None, Origin::CurrentSession, 2);

        let active = ledger.active();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, new);
        assert_eq!(active[0].supersedes, vec![old.clone()]);
        assert_eq!(
            ledger.all()[0].status,
            InstructionStatus::Superseded,
            "the override chain keeps the old instruction, it does not delete it"
        );
        assert!(ledger.conflicts().is_empty());
    }

    #[test]
    fn a_revocation_withdraws_without_asserting_the_opposite() {
        let mut ledger = InstructionLedger::new();
        ledger.record("不要提交代码", None, Origin::CurrentSession, 1);
        let revoke = ledger.record("撤销之前提交代码的要求", None, Origin::CurrentSession, 2);

        assert_eq!(ledger.all()[0].status, InstructionStatus::Revoked);
        assert_eq!(ledger.all()[0].revoked_by.as_deref(), Some(revoke.as_str()));
        // The revocation itself does not become a permission.
        assert_eq!(ledger.all()[1].polarity, Polarity::Revoke);
    }

    #[test]
    fn a_narrower_exception_does_not_silently_cancel_the_broader_rule() {
        let mut ledger = InstructionLedger::new();
        let broad = ledger.record("不要修改配置文件", None, Origin::CurrentSession, 1);
        let narrow = ledger.record(
            "可以修改配置文件",
            Some("tests/fixtures".into()),
            Origin::CurrentSession,
            2,
        );

        let conflicts = ledger.conflicts();
        assert_eq!(conflicts.len(), 1, "{conflicts:?}");
        assert!(conflicts[0] == (broad.clone(), narrow.clone()));
        // Both remain binding; neither was chosen for the caller.
        assert_eq!(ledger.active().len(), 2);
    }

    #[test]
    fn unrelated_scopes_are_reported_as_a_conflict_not_ordered_by_time() {
        let mut ledger = InstructionLedger::new();
        ledger.record(
            "不要删除日志文件",
            Some("src/app".into()),
            Origin::CurrentSession,
            1,
        );
        ledger.record(
            "可以删除日志文件",
            Some("vendor/lib".into()),
            Origin::CurrentSession,
            2,
        );

        assert_eq!(ledger.conflicts().len(), 1);
        assert_eq!(ledger.active().len(), 2);
    }

    #[test]
    fn long_term_memory_never_overrides_the_current_session() {
        let mut ledger = InstructionLedger::new();
        let current = ledger.record("不要提交代码", None, Origin::CurrentSession, 1);
        // A memory record claiming to be newer still loses.
        ledger.record("可以提交代码", None, Origin::LongTermMemory, 99);

        let active = ledger.active();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, current);
        assert_eq!(ledger.all()[1].status, InstructionStatus::Superseded);
        assert!(ledger.conflicts().is_empty());
    }

    #[test]
    fn a_current_instruction_overrides_an_earlier_memory_one() {
        let mut ledger = InstructionLedger::new();
        ledger.record("不要提交代码", None, Origin::LongTermMemory, 1);
        let current = ledger.record("可以提交代码", None, Origin::CurrentSession, 2);

        let active = ledger.active();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, current);
        assert_eq!(active[0].supersedes.len(), 1);
    }

    #[test]
    fn instructions_about_different_things_do_not_interact() {
        let mut ledger = InstructionLedger::new();
        ledger.record("不要提交代码", None, Origin::CurrentSession, 1);
        ledger.record("可以删除临时目录", None, Origin::CurrentSession, 2);

        assert_eq!(ledger.active().len(), 2);
        assert!(ledger.conflicts().is_empty());
    }
}
