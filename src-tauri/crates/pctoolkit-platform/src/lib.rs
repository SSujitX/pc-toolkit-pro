//! OS facts and capabilities. No product orchestration.

pub mod autostart;
mod app_icons;
mod dev_tools;
mod disk_cleanup;
mod errors;
mod file_ops;
mod gpu;
mod installed_apps;
mod launch;
mod memory;
mod monitor;
mod power;
mod process;
mod recycle;
mod hidden_child;
mod registry;
/// Typed registry setting values (read with type, write, `.reg` re-import) for reversible fixes.
pub mod registry_settings;
/// Windows Security Center products, Windows Firewall profiles, Windows Update Agent facts.
pub mod security_center;
/// Startup entries (Run / Startup folder / logon tasks / auto services) and their switches.
pub mod startup_items;
mod system_entries;
mod system_info;
mod uninstall_runner;
/// Volume / physical disk health and optimization facts (dirty bit, Storage WMI, TRIM, Optimize Drives).
pub mod disk_health;
/// Privacy trace facts (Explorer MRU value counts, Recent items folder).
pub mod privacy_traces;
/// Batch `.lnk` target resolution under one COM apartment (Care invalid-shortcut scan).
pub mod shortcut_batch;
/// Windows Package Manager (`winget`) upgrade listing and per-package upgrade.
pub mod winget;

pub use app_icons::{encode_png_rgba, icon_png_from_file};
pub use dev_tools::{
    conda_clean_execute, conda_clean_preview, count_running_images, docker_build_cache_preview,
    docker_build_cache_prune, rust_toolchains_list, rust_toolchains_uninstall, CondaCleanPreview,
    DevToolError, DevToolReason, DevToolResult, DockerBuildCachePreview, DockerBuildCachePrune,
    RustToolchainFact, RustToolchainInventory, RustToolchainUninstall, RustToolchainUninstallStatus,
};
pub use disk_cleanup::{
    volume_cache_estimate, volume_cache_estimate_cancellable, volume_cache_purge, windows_old_present,
    VolumeCacheEstimate, VolumeCacheKind, VolumeCachePurge, VolumeCachePurgeStatus, VolumeCacheStatus,
};
pub use errors::{PlatformError, PlatformResult};
pub use file_ops::{
    delete_on_reboot, fixed_drive_roots, long_path_name, recycle_path, resolve_shortcut, user_desktop_dir,
    WOULD_DELETE_PERMANENTLY,
};
pub use gpu::GpuSample;
pub use installed_apps::{
    list_store_packages, list_uninstall_entries, read_uninstall_entry, remove_store_package,
    resolve_store_logo,
    uninstall_roots, StorePackage, UninstallEntry, UNINSTALL_SUBKEY,
};
pub use launch::{launch_program, relaunch_self_elevated, relaunch_self_elevated_with_args, QuickActionId};
pub use memory::{
    free_physical_memory_bytes, memory_stats, optimize_memory, optimize_memory_areas,
    ordered_areas, AreaOutcome, AreaStatus, MemoryArea, MemoryOptimizeResult, MemoryStats,
};
pub use monitor::{is_user_admin, require_admin, sample_monitor, MonitorSample, OsLabel};
pub use power::{
    cancel_scheduled_shutdown, execute_power_action, schedule_restart, schedule_shutdown, PowerAction,
};
pub use process::{
    close_processes, hide_console, probe_running_processes, ProcessCloseBatchResult,
    ProcessCloseTargetResult, RunningProcessGroup,
};
pub use recycle::{empty_recycle_bin, query_recycle_bin, RecycleBinEmptyResult, RecycleBinInfo};
pub use registry::{
    delete_tree as delete_registry_tree, delete_value as delete_registry_value,
    export_key as export_registry_key, key_exists as registry_key_exists,
    read_value as read_registry_value, subkey_names as registry_subkey_names,
    system32_exe, values as registry_values, RegistryKeyPath, RegistryRoot, RegistryValueText, RegistryView,
};
pub use system_entries::{
    create_restore_point, delete_scheduled_task, delete_service, list_scheduled_tasks,
    list_services, RestorePointOutcome, ScheduledTaskFact, ServiceFact,
};
pub use uninstall_runner::{
    focus_uninstaller_window, path_exists, run_uninstaller, split_command_line, UninstallerRunOutcome,
    UninstallerTick,
};
pub use system_info::{
    load_system_information, load_system_information_with_progress, SystemInfoProgress,
    SystemInformation,
};
