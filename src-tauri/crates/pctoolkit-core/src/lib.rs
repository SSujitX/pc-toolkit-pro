//! Product domains — no Tauri dependency.

pub mod cleaner;
pub mod history;
pub mod memory;
pub mod monitor;
pub mod power;
pub mod shared;
pub mod system_info;

pub use cleaner::{
    cancel_cleanup, execute_cleanup, scan_cleanup, scan_cleanup_with_progress, CleanerCategory,
    CleanupExecuteRequest, CleanupProgress, CleanupResult, CleanupScan,
};
pub use history::{
    append_history, clear_history, history_now_ms, list_history, record_history, HistoryOutcome,
    HistoryRecord, HistoryWrite,
};
pub use memory::{
    cancel_memory_optimize, default_settings, get_memory_stats, load_settings, optimize_memory,
    save_settings, MemoryCleanerSettings, MemoryProgress, OptimizeMemoryRequest, OptimizeReason,
    AUTO_INTERVAL_STEPS_MINUTES,
};
pub use monitor::get_monitor_snapshot;
pub use power::{
    cancel_scheduled_shutdown, execute_power, schedule_shutdown, PowerAction, ScheduleRequest,
};
pub use shared::{CoreError, CoreErrorCode, CoreResult};
pub use system_info::{get_system_information, get_system_information_with_progress};
pub use pctoolkit_platform::SystemInfoProgress;

// Re-export memory types used by the frontend adapter.
pub use pctoolkit_platform::{
    AreaOutcome, AreaStatus, MemoryArea, MemoryOptimizeResult, MemoryStats,
};
