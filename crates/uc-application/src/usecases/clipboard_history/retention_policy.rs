//! Enforcement of the user-configured clipboard history retention policy.
//!
//! `RetentionPolicy` (`enabled`, `rules`, `skip_pinned`, `evaluation`) was
//! previously stored and served back through settings but never read by any
//! deletion path — the file-cache TTL/quota sweep in `cleanup.rs` enforces a
//! different, disk-only budget (`file_sync.*`). This use case is the first
//! thing that actually reads `retention_policy` and deletes clipboard entries
//! because of it, covering every entry (not just disk-backed ones).
//!
//! Only `RetentionRule::ByAge` and `RetentionRule::ByCount` are implemented —
//! the only variants the settings UI and defaults ever produce today. A
//! policy containing any other variant fails closed: enforcement is skipped
//! entirely for that pass (logged, not silently accepted), rather than
//! dropping the unsupported rule from the set and risking a weaker-than-
//! configured `AllMatch` combination over-deleting history.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use tracing::{info, info_span, warn, Instrument};

use uc_core::clipboard::ClipboardEntry;
use uc_core::ids::EntryId;
use uc_core::ports::clipboard::ListClipboardEntriesPort;
use uc_core::ports::SettingsPort;
use uc_core::settings::model::{RetentionRule, RuleEvaluation};

use super::delete_entry::DeleteClipboardEntryUseCase;

#[derive(Debug, Default, Clone)]
pub struct RetentionEnforcementResult {
    /// Number of entries deleted because they matched a retention rule.
    pub entries_deleted: u32,
    /// Number of per-entry delete failures (non-fatal; enforcement continues).
    pub errors: u32,
}

const ENTRY_LIST_BATCH_SIZE: usize = 1000;

pub(crate) struct EnforceRetentionPolicyUseCase {
    settings: Arc<dyn SettingsPort>,
    list_entries: Arc<dyn ListClipboardEntriesPort>,
    delete_uc: Arc<DeleteClipboardEntryUseCase>,
}

impl EnforceRetentionPolicyUseCase {
    pub(crate) fn new(
        settings: Arc<dyn SettingsPort>,
        list_entries: Arc<dyn ListClipboardEntriesPort>,
        delete_uc: Arc<DeleteClipboardEntryUseCase>,
    ) -> Self {
        Self {
            settings,
            list_entries,
            delete_uc,
        }
    }

    #[tracing::instrument(name = "usecase.enforce_retention_policy.execute", skip(self))]
    pub(crate) async fn execute(&self) -> Result<RetentionEnforcementResult> {
        let settings = self.settings.load().await?;
        let policy = settings.retention_policy;

        if !policy.enabled {
            info!("Retention policy disabled, skipping");
            return Ok(RetentionEnforcementResult::default());
        }

        // Fail closed rather than silently drop unsupported variants from
        // the rule set: under `RuleEvaluation::AllMatch`, dropping a rule
        // from the intersection makes the *combined* policy weaker than
        // configured (e.g. `{ByAge, ByTotalSize}` would devolve into "delete
        // everything matching ByAge"), which can over-delete history the
        // user did not intend to lose. Not reachable via the UI today (only
        // ByAge/ByCount are ever produced), but a hand-edited or future
        // migrated config could set this, and getting it wrong here is
        // irreversible data loss.
        let unsupported_rules: Vec<&RetentionRule> = policy
            .rules
            .iter()
            .filter(|rule| {
                !matches!(
                    rule,
                    RetentionRule::ByAge { .. } | RetentionRule::ByCount { .. }
                )
            })
            .collect();
        if !unsupported_rules.is_empty() {
            warn!(
                rules = ?unsupported_rules,
                evaluation = ?policy.evaluation,
                "Retention policy contains unsupported rule variants; skipping enforcement entirely"
            );
            return Ok(RetentionEnforcementResult::default());
        }

        let candidates = self.collect_candidates().await?;
        let now_ms = now_millis();
        let victims = select_entries_to_evict_for_retention(
            candidates,
            &policy.rules,
            policy.evaluation,
            policy.skip_pinned,
            now_ms,
        );

        let mut result = RetentionEnforcementResult::default();
        if victims.is_empty() {
            info!("Retention policy: nothing to evict");
            return Ok(result);
        }

        let candidates = victims.len();
        for entry_id in &victims {
            match self.delete_uc.execute(entry_id).await {
                Ok(()) => result.entries_deleted += 1,
                Err(e) => {
                    warn!(entry_id = %entry_id, error = %e, "Retention delete failed");
                    result.errors += 1;
                }
            }
        }

        info!(
            candidates,
            entries_deleted = result.entries_deleted,
            errors = result.errors,
            "Retention policy enforcement complete"
        );
        Ok(result)
    }

    async fn collect_candidates(&self) -> Result<Vec<RetentionCandidate>> {
        let mut out = Vec::new();
        let mut offset = 0usize;

        loop {
            let batch = self
                .list_entries
                .list_entries(ENTRY_LIST_BATCH_SIZE, offset)
                .instrument(info_span!(
                    "list_entries_batch",
                    batch_size = ENTRY_LIST_BATCH_SIZE,
                    offset = offset
                ))
                .await
                .map_err(|e| anyhow::anyhow!("list entries for retention: {e}"))?;

            if batch.is_empty() {
                break;
            }
            let batch_len = batch.len();
            out.extend(batch.into_iter().map(RetentionCandidate::from));
            offset += batch_len;

            if batch_len < ENTRY_LIST_BATCH_SIZE {
                break;
            }
        }

        Ok(out)
    }
}

struct RetentionCandidate {
    entry_id: EntryId,
    created_at_ms: i64,
    is_favorited: bool,
}

impl From<ClipboardEntry> for RetentionCandidate {
    fn from(entry: ClipboardEntry) -> Self {
        Self {
            entry_id: entry.entry_id,
            created_at_ms: entry.created_at_ms,
            is_favorited: entry.is_favorited,
        }
    }
}

/// Decide which entries to evict per the retention policy. Pure and
/// deterministic so the policy can be unit-tested without any I/O.
///
/// `skip_pinned` removes favorited candidates before any rule is evaluated —
/// a favorited entry is never a retention victim. Each supported rule
/// independently computes its own eviction set from the remaining
/// candidates; unsupported rule variants contribute no eviction set (the
/// caller is responsible for logging that they were ignored). The per-rule
/// sets are combined per `evaluation`: `AnyMatch` unions them (an entry is
/// evicted if any rule wants it gone), `AllMatch` intersects them (an entry
/// must satisfy every rule to be evicted). An empty `rules` list, or a
/// `rules` list containing no supported variant, evicts nothing.
fn select_entries_to_evict_for_retention(
    mut candidates: Vec<RetentionCandidate>,
    rules: &[RetentionRule],
    evaluation: RuleEvaluation,
    skip_pinned: bool,
    now_ms: i64,
) -> Vec<EntryId> {
    if skip_pinned {
        candidates.retain(|c| !c.is_favorited);
    }
    if candidates.is_empty() {
        return Vec::new();
    }

    // Newest first: `ByCount` keeps a prefix of this order, and it doubles as
    // a stable, human-obvious ordering for `ByAge`.
    candidates.sort_by_key(|c| std::cmp::Reverse(c.created_at_ms));

    let mut rule_sets: Vec<HashSet<EntryId>> = Vec::new();
    for rule in rules {
        match rule {
            RetentionRule::ByAge { max_age } => {
                let cutoff_ms = now_ms.saturating_sub(max_age.as_millis() as i64);
                rule_sets.push(
                    candidates
                        .iter()
                        .filter(|c| c.created_at_ms < cutoff_ms)
                        .map(|c| c.entry_id.clone())
                        .collect(),
                );
            }
            RetentionRule::ByCount { max_items } => {
                rule_sets.push(
                    candidates
                        .iter()
                        .skip(*max_items)
                        .map(|c| c.entry_id.clone())
                        .collect(),
                );
            }
            _ => {
                // Not yet supported; `execute` already warned about this.
            }
        }
    }

    if rule_sets.is_empty() {
        return Vec::new();
    }

    match evaluation {
        RuleEvaluation::AnyMatch => {
            let mut union: HashSet<EntryId> = HashSet::new();
            for set in rule_sets {
                union.extend(set);
            }
            union.into_iter().collect()
        }
        RuleEvaluation::AllMatch => {
            let mut sets = rule_sets.into_iter();
            let first = sets.next().unwrap_or_default();
            sets.fold(first, |acc, set| acc.intersection(&set).cloned().collect())
                .into_iter()
                .collect()
        }
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn candidate(id: &str, created_at_ms: i64, is_favorited: bool) -> RetentionCandidate {
        RetentionCandidate {
            entry_id: EntryId::from_str(id),
            created_at_ms,
            is_favorited,
        }
    }

    fn ids(victims: &[EntryId]) -> HashSet<String> {
        victims.iter().map(|id| id.inner().clone()).collect()
    }

    #[test]
    fn by_age_evicts_entries_older_than_cutoff() {
        let now_ms = 1_000_000_000_000;
        let candidates = vec![
            candidate("fresh", now_ms - 10_000, false),
            candidate("stale", now_ms - 1_000_000_000, false),
        ];
        let rules = vec![RetentionRule::ByAge {
            max_age: Duration::from_secs(1000),
        }];

        let victims = select_entries_to_evict_for_retention(
            candidates,
            &rules,
            RuleEvaluation::AnyMatch,
            false,
            now_ms,
        );

        assert_eq!(ids(&victims), HashSet::from(["stale".to_string()]));
    }

    #[test]
    fn by_count_keeps_only_the_newest_n() {
        let candidates = vec![
            candidate("newest", 300, false),
            candidate("middle", 200, false),
            candidate("oldest", 100, false),
        ];
        let rules = vec![RetentionRule::ByCount { max_items: 2 }];

        let victims = select_entries_to_evict_for_retention(
            candidates,
            &rules,
            RuleEvaluation::AnyMatch,
            false,
            1000,
        );

        assert_eq!(ids(&victims), HashSet::from(["oldest".to_string()]));
    }

    // `ByAge` and `ByCount` both evict a suffix of the same recency-sorted
    // list (everything past an age cutoff, or everything past a rank
    // cutoff), so for these two rule types one rule's eviction set is always
    // a subset of the other's — there is no candidate an age rule wants gone
    // that a *stricter* count rule wouldn't also want gone, or vice versa.
    // These tests pick a `max_items` stricter than `max_age` so the two
    // rules produce genuinely different (nested) sets, and verify
    // `AnyMatch`/`AllMatch` pick the superset/subset respectively.

    #[test]
    fn any_match_unions_rule_eviction_sets() {
        let now_ms = 10_000;
        let candidates = vec![
            candidate("newest", 9_990, false),
            candidate("second", 9_980, false), // count-evicted only (not aged)
            candidate("third", 100, false),    // both count- and age-evicted
            candidate("oldest", 50, false),    // both count- and age-evicted
        ];
        let rules = vec![
            RetentionRule::ByAge {
                max_age: Duration::from_millis(100), // cutoff = 9_900: only third/oldest are aged out
            },
            RetentionRule::ByCount { max_items: 1 }, // keeps only "newest"
        ];

        let victims = select_entries_to_evict_for_retention(
            candidates,
            &rules,
            RuleEvaluation::AnyMatch,
            false,
            now_ms,
        );

        assert_eq!(
            ids(&victims),
            HashSet::from([
                "second".to_string(),
                "third".to_string(),
                "oldest".to_string(),
            ])
        );
    }

    #[test]
    fn all_match_intersects_rule_eviction_sets() {
        let now_ms = 10_000;
        let candidates = vec![
            candidate("newest", 9_990, false),
            candidate("second", 9_980, false), // count-evicted only (not aged) -> not intersected
            candidate("third", 100, false),    // both -> intersected
            candidate("oldest", 50, false),    // both -> intersected
        ];
        let rules = vec![
            RetentionRule::ByAge {
                max_age: Duration::from_millis(100),
            },
            RetentionRule::ByCount { max_items: 1 },
        ];

        let victims = select_entries_to_evict_for_retention(
            candidates,
            &rules,
            RuleEvaluation::AllMatch,
            false,
            now_ms,
        );

        assert_eq!(
            ids(&victims),
            HashSet::from(["third".to_string(), "oldest".to_string()])
        );
    }

    #[test]
    fn skip_pinned_exempts_favorited_entries() {
        let now_ms = 1_000_000_000_000;
        let candidates = vec![
            candidate("favorited_but_stale", now_ms - 1_000_000_000, true),
            candidate("stale", now_ms - 1_000_000_000, false),
        ];
        let rules = vec![RetentionRule::ByAge {
            max_age: Duration::from_secs(1000),
        }];

        let victims = select_entries_to_evict_for_retention(
            candidates,
            &rules,
            RuleEvaluation::AnyMatch,
            true,
            now_ms,
        );

        assert_eq!(ids(&victims), HashSet::from(["stale".to_string()]));
    }

    #[test]
    fn empty_rules_evict_nothing() {
        let candidates = vec![candidate("a", 100, false)];
        let victims = select_entries_to_evict_for_retention(
            candidates,
            &[],
            RuleEvaluation::AnyMatch,
            false,
            1000,
        );
        assert!(victims.is_empty());
    }

    #[test]
    fn unsupported_rule_variant_evicts_nothing_on_its_own() {
        let candidates = vec![candidate("a", 100, false)];
        let rules = vec![RetentionRule::ByTotalSize { max_bytes: 1 }];
        let victims = select_entries_to_evict_for_retention(
            candidates,
            &rules,
            RuleEvaluation::AnyMatch,
            false,
            1000,
        );
        assert!(victims.is_empty());
    }
}
