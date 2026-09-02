use pctoolkit_platform::load_system_information;
use serde::Serialize;

use crate::shared::CoreResult;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInformationDto {
    pub uptime: String,
    pub cpu_name: String,
    pub cpu_cores: u32,
    pub cpu_threads: u32,
    pub cpu_usage: f32,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_percent: f32,
    pub disk_total: u64,
    pub disk_used: u64,
    pub disk_percent: f32,
    pub gpu_name: String,
    pub gpu_usage: Option<f32>,
    pub gpu_memory_used: Option<u64>,
    pub gpu_memory_total: Option<u64>,
    pub gpu_temperature: Option<f32>,
    pub motherboard: String,
    pub bios: String,
    pub os_edition: String,
    pub os_version: String,
    pub os_build: String,
    pub hostname: String,
    pub username: String,
    pub monitors: Vec<String>,
    pub storage_devices: Vec<String>,
    pub power_plan: String,
    pub power_supplies: Vec<String>,
    pub batteries: Vec<String>,
    pub ac_line_status: String,
    pub copy_text: String,
}

pub fn get_system_information() -> CoreResult<SystemInformationDto> {
    let info = load_system_information()?;
    Ok(SystemInformationDto {
        uptime: info.uptime,
        cpu_name: info.cpu_name,
        cpu_cores: info.cpu_cores,
        cpu_threads: info.cpu_threads,
        cpu_usage: info.cpu_usage,
        memory_total: info.memory_total,
        memory_used: info.memory_used,
        memory_percent: info.memory_percent,
        disk_total: info.disk_total,
        disk_used: info.disk_used,
        disk_percent: info.disk_percent,
        gpu_name: info.gpu_name,
        gpu_usage: info.gpu_usage,
        gpu_memory_used: info.gpu_memory_used,
        gpu_memory_total: info.gpu_memory_total,
        gpu_temperature: info.gpu_temperature,
        motherboard: info.motherboard,
        bios: info.bios,
        os_edition: info.os_edition,
        os_version: info.os_version,
        os_build: info.os_build,
        hostname: info.hostname,
        username: info.username,
        monitors: info.monitors,
        storage_devices: info.storage_devices,
        power_plan: info.power_plan,
        power_supplies: info.power_supplies,
        batteries: info.batteries,
        ac_line_status: info.ac_line_status,
        copy_text: info.copy_text,
    })
}
