//! OS facts and capabilities. No product orchestration.

mod errors;
mod gpu;
mod launch;
mod memory;
mod monitor;
mod power;
mod process;
mod recycle;
mod system_info;

pub use errors::{PlatformError, PlatformResult};
pub use gpu::GpuSample;
pub use launch::{launch_program, relaunch_self_elevated, QuickActionId};
pub use memory::{
    free_physical_memory_bytes, memory_stats, optimize_memory, optimize_memory_areas,
    ordered_areas, AreaOutcome, AreaStatus, MemoryArea, MemoryOptimizeResult, MemoryStats,
};
pub use monitor::{is_user_admin, require_admin, sample_monitor, MonitorSample, OsLabel};
pub use power::{cancel_scheduled_shutdown, execute_power_action, schedule_shutdown, PowerAction};
pub use recycle::empty_recycle_bin;
pub use system_info::{
    load_system_information, load_system_information_with_progress, SystemInfoProgress,
    SystemInformation,
};
