//! AI model stores (`ai` group) over `cleaner_ai_models`.
//!
//! One result per discovered model, never recommended or preselected. The scan
//! keeps the reviewed candidates in Core; execute deletes only a reviewed
//! candidate, and `delete_ai_model` re-discovers it and refuses it when its size,
//! file count, or fingerprint changed, or when the owning app is running.

use std::collections::HashMap;
use std::sync::Mutex;

use super::provider::{DeepCleanupProvider, ProviderContext};
use super::{
    DeepCleanupExecReason, DeepCleanupExecStatus, DeepCleanupGroup, DeepCleanupRisk, DeepCleanupRuleExecResult,
    DeepCleanupRuleExtras, DeepCleanupRuleResult, DeepCleanupRuleStatus,
};
use crate::cleaner_ai_models::{
    delete_ai_model, discover_ai_models, AiModelCandidate, AiModelDeleteResult, AiModelError, AiModelProvider,
};
use crate::shared::CoreResult;

const RULE_PREFIX: &str = "ai.model.";
const NAME_KEY: &str = "deepCleaner.ai.modelName";
const DETAIL_KEY: &str = "deepCleaner.ai.modelDetail";

/// Candidate id (`huggingFace:0123abcd…`) → rule id (`ai.model.huggingFace.0123abcd…`).
pub(crate) fn rule_id(candidate_id: &str) -> String {
    format!("{RULE_PREFIX}{}", candidate_id.replace(':', "."))
}

/// Provider code for the UI tag (the first segment of the candidate id).
fn provider_tag(candidate: &AiModelCandidate) -> String {
    candidate.id.split(':').next().unwrap_or_default().to_string()
}

/// Reviewed candidates from the last scan, by rule id.
static REVIEWED: Mutex<Option<HashMap<String, AiModelCandidate>>> = Mutex::new(None);

pub(crate) fn model_result(candidate: &AiModelCandidate, running: Vec<String>) -> DeepCleanupRuleResult {
    let status = if candidate.file_count == 0 {
        DeepCleanupRuleStatus::Clean
    } else if !running.is_empty() {
        DeepCleanupRuleStatus::RequiresClose
    } else {
        DeepCleanupRuleStatus::Found
    };
    DeepCleanupRuleResult {
        id: rule_id(&candidate.id),
        group: DeepCleanupGroup::Ai,
        name_key: NAME_KEY.into(),
        detail_key: DETAIL_KEY.into(),
        risk: DeepCleanupRisk::Recoverable,
        bytes: candidate.bytes,
        item_count: candidate.file_count,
        recommended: false,
        selected: false,
        status,
        related_processes: candidate.processes_to_close.clone(),
        running_processes: running,
        requires_elevation: false,
        needs_admin: false,
        limited: false,
        refused_roots: 0,
        extras: DeepCleanupRuleExtras {
            display_name: Some(candidate.display_name.clone()),
            tags: vec![provider_tag(candidate)],
            last_modified_ms: candidate.newest_mtime_ms,
            ..DeepCleanupRuleExtras::default()
        },
    }
}

/// Pure mapping of a delete outcome to a rule result.
pub(crate) fn delete_outcome(
    id: &str,
    outcome: Result<AiModelDeleteResult, AiModelError>,
) -> DeepCleanupRuleExecResult {
    use DeepCleanupExecReason as R;
    use DeepCleanupExecStatus as S;
    match outcome {
        Ok(done) => {
            let (status, reason) = if done.complete && done.failed_files == 0 {
                (S::Completed, None)
            } else if done.removed_files > 0 {
                (S::Partial, Some(R::ItemsFailed))
            } else {
                (S::Failed, Some(R::ItemsFailed))
            };
            DeepCleanupRuleExecResult {
                id: id.to_string(),
                status,
                reason,
                freed_bytes: done.removed_bytes,
                removed_count: done.removed_files,
                failed_count: done.failed_files,
                busy_count: 0,
            }
        }
        Err(AiModelError::NotFound) => DeepCleanupRuleExecResult::without_work(id, S::Skipped, Some(R::NotApplicable)),
        Err(AiModelError::Changed) => DeepCleanupRuleExecResult::without_work(id, S::Skipped, Some(R::ChangedSinceScan)),
        Err(AiModelError::ProcessRunning { .. }) => {
            DeepCleanupRuleExecResult::without_work(id, S::Blocked, Some(R::RunningProcesses))
        }
    }
}

pub(crate) struct AiModelsProvider;

impl DeepCleanupProvider for AiModelsProvider {
    fn id(&self) -> &'static str {
        "aiModels"
    }

    fn groups(&self) -> &'static [DeepCleanupGroup] {
        &[DeepCleanupGroup::Ai]
    }

    fn owns_rule(&self, rule_id: &str) -> bool {
        rule_id.starts_with(RULE_PREFIX)
    }

    fn processes(&self) -> Vec<String> {
        AiModelProvider::ALL
            .iter()
            .flat_map(|p| p.processes_to_close().iter().map(|s| s.to_string()))
            .collect()
    }

    fn scan(&self, ctx: &ProviderContext<'_>) -> CoreResult<Vec<DeepCleanupRuleResult>> {
        let candidates = discover_ai_models(&|| ctx.is_cancelled());
        let mut reviewed = HashMap::new();
        let mut results = Vec::new();
        for candidate in candidates {
            let result = model_result(&candidate, ctx.running_of(&candidate.processes_to_close));
            if result.status == DeepCleanupRuleStatus::Clean {
                continue;
            }
            reviewed.insert(result.id.clone(), candidate);
            results.push(result);
        }
        if let Ok(mut slot) = REVIEWED.lock() {
            *slot = Some(reviewed);
        }
        results.sort_by(|a, b| b.bytes.cmp(&a.bytes));
        Ok(results)
    }

    fn execute(&self, rule_ids: &[String], ctx: &ProviderContext<'_>) -> CoreResult<Vec<DeepCleanupRuleExecResult>> {
        let reviewed = REVIEWED.lock().ok().and_then(|slot| slot.clone()).unwrap_or_default();
        let mut out = Vec::new();
        for id in rule_ids {
            if ctx.is_cancelled() {
                out.push(DeepCleanupRuleExecResult::without_work(
                    id.clone(),
                    DeepCleanupExecStatus::Skipped,
                    Some(DeepCleanupExecReason::Cancelled),
                ));
                continue;
            }
            let Some(candidate) = reviewed.get(id) else {
                out.push(DeepCleanupRuleExecResult::without_work(
                    id.clone(),
                    DeepCleanupExecStatus::Skipped,
                    Some(DeepCleanupExecReason::NotApplicable),
                ));
                continue;
            };
            if !ctx.running_of(&candidate.processes_to_close).is_empty() {
                out.push(DeepCleanupRuleExecResult::without_work(
                    id.clone(),
                    DeepCleanupExecStatus::Blocked,
                    Some(DeepCleanupExecReason::RunningProcesses),
                ));
                continue;
            }
            out.push(delete_outcome(id, delete_ai_model(candidate)));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn candidate(files: u64) -> AiModelCandidate {
        AiModelCandidate {
            id: "ollama:00ff".into(),
            provider: AiModelProvider::Ollama,
            display_name: "llama3:8b".into(),
            bytes: files * 100,
            file_count: files,
            newest_mtime_ms: Some(1),
            fingerprint: 7,
            processes_to_close: vec!["ollama.exe".into()],
            path: PathBuf::from(r"C:\nowhere"),
        }
    }

    #[test]
    fn model_results_are_never_preselected_and_carry_display_facts() {
        let r = model_result(&candidate(3), Vec::new());
        assert_eq!(r.id, "ai.model.ollama.00ff");
        assert_eq!(r.status, DeepCleanupRuleStatus::Found);
        assert!(!r.recommended && !r.selected);
        assert_eq!(r.extras.display_name.as_deref(), Some("llama3:8b"));
        assert_eq!(r.extras.tags, vec!["ollama".to_string()]);
        let busy = model_result(&candidate(3), vec!["ollama.exe".into()]);
        assert_eq!(busy.status, DeepCleanupRuleStatus::RequiresClose);
        assert_eq!(model_result(&candidate(0), Vec::new()).status, DeepCleanupRuleStatus::Clean);
    }

    #[test]
    fn delete_errors_map_to_typed_reasons() {
        use DeepCleanupExecReason as R;
        use DeepCleanupExecStatus as S;
        let changed = delete_outcome("a.b", Err(AiModelError::Changed));
        assert_eq!((changed.status, changed.reason), (S::Skipped, Some(R::ChangedSinceScan)));
        let running = delete_outcome("a.b", Err(AiModelError::ProcessRunning { images: vec![] }));
        assert_eq!(running.status, S::Blocked);
        let partial = delete_outcome(
            "a.b",
            Ok(AiModelDeleteResult {
                id: "x".into(),
                removed_bytes: 10,
                removed_files: 1,
                failed_files: 1,
                skipped_links: 0,
                complete: false,
            }),
        );
        assert_eq!((partial.status, partial.freed_bytes), (S::Partial, 10));
    }

    #[test]
    fn provider_owns_only_its_prefix() {
        assert!(AiModelsProvider.owns_rule("ai.model.huggingFace.0011"));
        assert!(!AiModelsProvider.owns_rule("ai.lmStudioCache"));
    }
}
