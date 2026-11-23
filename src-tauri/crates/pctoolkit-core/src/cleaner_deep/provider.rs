//! Pluggable rule sources for Deep Cleanup.
//!
//! The TOML catalog covers static cache locations. Dynamic sources — project
//! build artifacts discovered by a scanner, AI model stores — implement
//! `DeepCleanupProvider` and are listed in `registered_providers`. Scan runs each
//! provider on the shared worker pool; execute routes rule ids to the provider
//! whose `owns_rule` returns true.
//!
//! Provider rules must:
//! - use ids that do not collide with catalog ids (prefix with the group, e.g.
//!   `projects.<hash>` or `ai.<tool>`);
//! - report `DeepCleanupRuleResult` with the provider's group and locale keys
//!   under `deepCleaner.*`; per-item facts (display name, ecosystems, artifacts,
//!   active flag) go in `DeepCleanupRuleExtras`;
//! - never preselect items the user has not reviewed (project artifacts and AI
//!   models are never recommended);
//! - delete only through `walk::clean_directory` (or an equally checked special
//!   operation) so link, root, and per-entry re-checks stay uniform;
//! - never put raw paths in result messages, logs, or history.

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::Ordering;

use super::safety::SafetyContext;
use super::{DeepCleanupGroup, DeepCleanupRuleExecResult, DeepCleanupRuleResult};
use crate::cleaner::CANCELLED;
use crate::shared::CoreResult;

/// Progress callback shared by every worker (`Sync`).
pub type ProviderProgressFn<'a> = dyn Fn(&str, &Path, u64, u64) + Sync + 'a;

/// Read-only context handed to providers for one scan or execute run.
pub struct ProviderContext<'a> {
    pub is_admin: bool,
    /// Lowercase image names (`chrome.exe`) running when the run started.
    pub running_processes: &'a HashSet<String>,
    pub safety: &'a SafetyContext,
    progress: &'a ProviderProgressFn<'a>,
}

impl<'a> ProviderContext<'a> {
    pub(crate) fn new(
        is_admin: bool,
        running_processes: &'a HashSet<String>,
        safety: &'a SafetyContext,
        progress: &'a ProviderProgressFn<'a>,
    ) -> Self {
        Self {
            is_admin,
            running_processes,
            safety,
            progress,
        }
    }

    /// True once the user pressed Cancel. Long loops must check it.
    pub fn is_cancelled(&self) -> bool {
        CANCELLED.load(Ordering::Relaxed)
    }

    /// Report work for the live progress card. `path` is shown in the UI only;
    /// `items` / `bytes` are deltas since the previous report.
    pub fn report(&self, rule_id: &str, path: &Path, items: u64, bytes: u64) {
        (self.progress)(rule_id, path, items, bytes);
    }

    /// The subset of `processes` (image names) running at run start.
    pub fn running_of(&self, processes: &[String]) -> Vec<String> {
        processes
            .iter()
            .filter(|p| self.running_processes.contains(&p.to_lowercase()))
            .cloned()
            .collect()
    }
}

pub trait DeepCleanupProvider: Send + Sync {
    /// Stable id for diagnostics (`projects`, `aiModels`).
    fn id(&self) -> &'static str;
    /// Groups this provider contributes to (drives the sidebar group list).
    fn groups(&self) -> &'static [DeepCleanupGroup];
    /// True when `rule_id` belongs to this provider.
    fn owns_rule(&self, rule_id: &str) -> bool;
    /// Process image names whose running state the provider needs in `ctx`.
    fn processes(&self) -> Vec<String> {
        Vec::new()
    }
    /// Read-only discovery + measurement. Return one result per rule.
    fn scan(&self, ctx: &ProviderContext<'_>) -> CoreResult<Vec<DeepCleanupRuleResult>>;
    /// Clean `rule_ids` (all owned by this provider). Return one result per id,
    /// re-discovering and re-validating targets instead of trusting scan state.
    fn execute(
        &self,
        rule_ids: &[String],
        ctx: &ProviderContext<'_>,
    ) -> CoreResult<Vec<DeepCleanupRuleExecResult>>;
}

/// Providers participating in Deep Cleanup. Add new providers here.
pub fn registered_providers() -> Vec<&'static dyn DeepCleanupProvider> {
    static PROJECTS: super::projects::ProjectsProvider = super::projects::ProjectsProvider;
    static AI_MODELS: super::ai_provider::AiModelsProvider = super::ai_provider::AiModelsProvider;
    vec![&PROJECTS, &AI_MODELS]
}
