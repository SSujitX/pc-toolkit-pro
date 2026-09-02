//! Product domains — no Tauri dependency.

pub mod cleaner;
pub mod history;
pub mod monitor;
pub mod power;
pub mod shared;
pub mod system_info;

pub use cleaner::{
    cancel_cleanup, execute_cleanup, scan_cleanup, CleanerCategory, CleanupExecuteRequest,
    CleanupProgress, CleanupResult, CleanupScan,
};
pub use history::{list_history, record_history, HistoryRecord};
pub use monitor::get_monitor_snapshot;
pub use power::{
    cancel_scheduled_shutdown, execute_power, schedule_shutdown, PowerAction, ScheduleRequest,
};
pub use shared::{CoreError, CoreErrorCode, CoreResult};
pub use system_info::get_system_information;
