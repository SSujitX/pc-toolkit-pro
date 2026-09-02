use serde::Serialize;
use std::time::Duration;
use sysinfo::{Disks, System};

use crate::{PlatformError, PlatformResult};

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
    let mut sys = System::new();
    sys.refresh_cpu_usage();
    std::thread::sleep(Duration::from_millis(200));
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    let cpu = sys.global_cpu_usage();
    let memory_total = sys.total_memory();
    let memory_used = sys.used_memory();
    let memory_percent = if memory_total == 0 {
        0.0
    } else {
        (memory_used as f64 / memory_total as f64 * 100.0) as f32
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

    let gpu = crate::gpu::sample_nvidia().ok();
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
