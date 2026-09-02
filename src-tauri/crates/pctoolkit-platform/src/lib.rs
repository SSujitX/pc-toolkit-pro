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
pub use launch::{launch_program, QuickActionId};
pub use memory::{free_physical_memory_bytes, optimize_memory, MemoryOptimizeResult};
pub use monitor::{is_user_admin, require_admin, sample_monitor, MonitorSample, OsLabel};
pub use power::{cancel_scheduled_shutdown, execute_power_action, schedule_shutdown, PowerAction};
pub use recycle::empty_recycle_bin;
pub use system_info::{load_system_information, SystemInformation};
