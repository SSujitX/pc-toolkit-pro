//! Product domains — no Tauri dependency.

pub mod app_settings;
pub mod care;
pub mod cleaner;
pub mod cleaner_ai_models;
pub mod cleaner_deep;
pub mod history;
pub mod memory;
pub mod monitor;
pub mod power;
pub mod shared;
pub mod startup;
pub mod system_info;
pub mod uninstaller;

pub use cleaner::{
    cancel_cleanup, execute_cleanup, scan_cleanup, scan_cleanup_with_progress, CleanerCategory,
    CleanupExecuteRequest, CleanupProgress, CleanupResult, CleanupScan,
};
pub use cleaner_deep::{
    execute_deep_cleanup, scan_deep_cleanup_with_progress, DeepCleanupExecuteRequest,
    DeepCleanupGroup, DeepCleanupProgress, DeepCleanupResult, DeepCleanupRuleResult,
    DeepCleanupScan,
};
pub use history::{
    append_history, clear_history, history_now_ms, history_settings, list_history, record_history,
    set_history_settings, HistoryOutcome, HistorySettings,
    HistoryRecord, HistoryWrite,
};
pub use memory::{
    auto_clean_decision, auto_clean_tick, cancel_memory_optimize, default_settings,
    get_memory_stats, load_settings, optimize_memory, save_settings, start_auto_clean_schedule,
    MemoryCleanerSettings, MemoryProgress, OptimizeMemoryRequest, OptimizeReason,
    AUTO_CLEAN_TICK_MS, AUTO_INTERVAL_STEPS_MINUTES, LOW_MEMORY_COOLDOWN_MS,
};
pub use monitor::get_monitor_snapshot;
pub use power::{
    cancel_scheduled_shutdown, execute_power, schedule_shutdown, PowerAction, ScheduleRequest,
};
pub use shared::{CoreError, CoreErrorCode, CoreResult};
pub use uninstaller::{
    cancel_uninstaller, clean_leftovers, dismiss_removed_program, focus_uninstaller, get_program_icons, list_programs,
    scan_program_leftovers, uninstall_program, ProgramIcon,
    InstalledProgram, LeftoverCleanRequest, LeftoverCleanResult, LeftoverScan, ProgramInventory,
    UninstallReport, UninstallRequest, UninstallerProgress,
};
pub use system_info::{get_system_information, get_system_information_with_progress};
pub use pctoolkit_platform::SystemInfoProgress;

// Re-export memory types used by the frontend adapter.
pub use pctoolkit_platform::{
    AreaOutcome, AreaStatus, MemoryArea, MemoryOptimizeResult, MemoryStats,
};
