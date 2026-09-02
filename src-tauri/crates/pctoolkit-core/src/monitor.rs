use pctoolkit_platform::sample_monitor;
use serde::Serialize;

use crate::shared::CoreResult;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorSnapshot {
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

pub fn get_monitor_snapshot() -> CoreResult<MonitorSnapshot> {
    let s = sample_monitor()?;
    Ok(MonitorSnapshot {
        cpu: s.cpu,
        memory_percent: s.memory_percent,
        memory_used: s.memory_used,
        memory_total: s.memory_total,
        disk_percent: s.disk_percent,
        disk_used: s.disk_used,
        disk_total: s.disk_total,
        uptime_seconds: s.uptime_seconds,
        os_label: s.os_label,
        gpu_available: s.gpu_available,
        gpu_utilization: s.gpu_utilization,
        gpu_memory_used: s.gpu_memory_used,
        gpu_memory_total: s.gpu_memory_total,
        gpu_temperature: s.gpu_temperature,
    })
}
