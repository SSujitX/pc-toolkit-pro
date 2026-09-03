use serde::Serialize;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use sysinfo::{Disks, System};

use crate::{PlatformError, PlatformResult};

/// One persistent sampler: CPU usage needs prior samples to diff against, so a
/// fresh `System` per poll would always report ~0%. A single instance also
/// avoids per-second reallocation on the titlebar/tray polling path.
fn shared_system() -> &'static Mutex<System> {
    static SYSTEM: OnceLock<Mutex<System>> = OnceLock::new();
    SYSTEM.get_or_init(|| Mutex::new(System::new()))
}

/// nvidia-smi is slow; running it on every 1s titlebar poll lets late replies
/// overwrite fresher RAM% with stale values. Cache and refresh occasionally.
fn cached_gpu_sample() -> Option<crate::gpu::GpuSample> {
    struct GpuCache {
        at: Instant,
        sample: Option<crate::gpu::GpuSample>,
    }
    static CACHE: OnceLock<Mutex<GpuCache>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| {
        Mutex::new(GpuCache {
            at: Instant::now()
                .checked_sub(Duration::from_secs(60))
                .unwrap_or_else(Instant::now),
            sample: None,
        })
    });
    let mut guard = cache.lock().unwrap_or_else(|p| p.into_inner());
    if guard.at.elapsed() >= Duration::from_secs(5) {
        guard.sample = crate::gpu::sample_nvidia().ok();
        guard.at = Instant::now();
    }
    guard.sample.clone()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorSample {
    pub cpu: f32,
    pub memory_percent: f32,
    pub memory_used: u64,
    pub memory_total: u64,
    pub disk_percent: f32,
    pub disk_used: u64,
    pub disk_total: u64,
    pub uptime_seconds: u64,
    pub os_label: String,
    pub gpu_available: bool,
    pub gpu_utilization: Option<f32>,
    pub gpu_memory_used: Option<u64>,
    pub gpu_memory_total: Option<u64>,
    pub gpu_temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OsLabel {
    pub label: String,
}

pub fn sample_monitor() -> PlatformResult<MonitorSample> {
    let cpu = {
        let mut sys = shared_system()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sys.refresh_cpu_usage();
        sys.global_cpu_usage()
    };

    // Same Win32 source as the Memory page (Task Manager / WMC / IObit semantics).
    let (memory_total, memory_used, memory_percent) = match crate::memory::memory_stats() {
        Ok(stats) => (
            stats.physical_total,
            stats.physical_used,
            stats.physical_load_percent,
        ),
        Err(_) => {
            let mut sys = shared_system()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            sys.refresh_memory();
            let memory_total = sys.total_memory();
            let memory_used = sys.used_memory();
            let memory_percent = if memory_total == 0 {
                0.0
            } else {
                (memory_used as f64 / memory_total as f64 * 100.0) as f32
            };
            (memory_total, memory_used, memory_percent)
        }
    };

    let disks = Disks::new_with_refreshed_list();
    let (disk_total, disk_used) = disks
        .iter()
        .find(|d| {
            let mount = d.mount_point().to_string_lossy();
            mount.eq_ignore_ascii_case("C:\\") || mount == "/"
        })
        .map(|d| {
            let total = d.total_space();
            let available = d.available_space();
            (total, total.saturating_sub(available))
        })
        .unwrap_or((0, 0));
    let disk_percent = if disk_total == 0 {
        0.0
    } else {
        (disk_used as f64 / disk_total as f64 * 100.0) as f32
    };

    let uptime_seconds = System::uptime();
    let os_label = format!(
        "{} {}",
        System::name().unwrap_or_else(|| "Windows".into()),
        System::os_version().unwrap_or_default()
    );

    let gpu = cached_gpu_sample();
    Ok(MonitorSample {
        cpu,
        memory_percent,
        memory_used,
        memory_total,
        disk_percent,
        disk_used,
        disk_total,
        uptime_seconds,
        os_label,
        gpu_available: gpu.is_some(),
        gpu_utilization: gpu.as_ref().map(|g| g.utilization),
        gpu_memory_used: gpu.as_ref().map(|g| g.memory_used),
        gpu_memory_total: gpu.as_ref().map(|g| g.memory_total),
        gpu_temperature: gpu.as_ref().map(|g| g.temperature),
    })
}

#[allow(dead_code)]
pub fn boot_time_unix() -> u64 {
    0
}

pub fn is_user_admin() -> bool {
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("net");
        cmd.args(["session"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        crate::process::hide_console(&mut cmd);
        cmd.status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn require_admin() -> PlatformResult<()> {
    if is_user_admin() {
        Ok(())
    } else {
        Err(PlatformError::PermissionDenied)
    }
}
