use serde::Serialize;
use std::process::Command;

use crate::gpu::sample_nvidia;
use crate::monitor::sample_monitor;
use crate::PlatformResult;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemInformation {
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

pub fn load_system_information() -> PlatformResult<SystemInformation> {
    let sample = sample_monitor()?;
    let uptime = format_uptime(sample.uptime_seconds);

    let cpu_name = query_ps("(Get-CimInstance Win32_Processor | Select-Object -First 1).Name")
        .unwrap_or_else(|| "Unknown CPU".into());
    let cores = sysinfo::System::physical_core_count().unwrap_or(0) as u32;
    let threads = {
        let mut sys = sysinfo::System::new();
        sys.refresh_cpu_all();
        sys.cpus().len() as u32
    };

    let gpu = sample_nvidia().ok();
    let motherboard = query_ps(
        r#"(Get-CimInstance Win32_BaseBoard | ForEach-Object { ("$($_.Manufacturer) $($_.Product)").Trim() } | Select-Object -First 1)"#,
    )
    .unwrap_or_else(|| "Unknown".into());
    let bios = query_ps(
        r#"(Get-CimInstance Win32_BIOS | ForEach-Object { ("$($_.Manufacturer) $($_.SMBIOSBIOSVersion)").Trim() } | Select-Object -First 1)"#,
    )
    .unwrap_or_else(|| "Unknown".into());
    let os_edition = query_ps(
        "(Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion').ProductName",
    )
    .unwrap_or_else(|| sample.os_label.clone());
    let os_version = query_ps(
        "(Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion').DisplayVersion",
    )
    .unwrap_or_default();
    let os_build = query_ps(
        "(Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion').CurrentBuild",
    )
    .unwrap_or_default();

    let hostname = sysinfo::System::host_name().unwrap_or_default();
    let username = std::env::var("USERNAME").unwrap_or_default();

    let monitors = query_ps_list(
        "Get-CimInstance Win32_DesktopMonitor | ForEach-Object { $_.Name } | Where-Object { $_ }",
    );
    let storage_devices = query_ps_list(
        "Get-CimInstance Win32_DiskDrive | ForEach-Object { \"$($_.Model) ($([math]::Round($_.Size/1GB,1)) GB)\" }",
    );

    let power_plan = query_ps(
        r#"try { (Get-CimInstance -Namespace root\cimv2\power -ClassName Win32_PowerPlan -Filter "IsActive='true'").ElementName } catch { $null }"#,
    )
    .or_else(|| {
        query_ps(
            r#"$m = powercfg /getactivescheme; if ($m -match '\((.+)\)') { $Matches[1] }"#,
        )
    })
    .unwrap_or_else(|| "Unknown".into());

    let power_supplies = query_ps_list(
        r#"Get-CimInstance Win32_PowerSupply -ErrorAction SilentlyContinue | ForEach-Object {
            $name = if ($_.Name) { $_.Name } else { $_.DeviceID }
            $status = if ($_.Status) { $_.Status } else { 'Unknown' }
            $watts = if ($_.TotalPower -and $_.TotalPower -gt 0) {
                '{0} W' -f [math]::Round($_.TotalPower / 1000.0, 0)
            } else { 'wattage not reported' }
            "$name · $status · $watts"
        }"#,
    );

    let batteries = query_ps_list(
        r#"Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | ForEach-Object {
            $name = if ($_.Name) { $_.Name } else { 'Battery' }
            $pct = if ($null -ne $_.EstimatedChargeRemaining) { "$($_.EstimatedChargeRemaining)%" } else { 'n/a' }
            $status = switch ($_.BatteryStatus) {
                1 { 'Discharging' }
                2 { 'AC connected' }
                3 { 'Fully charged' }
                4 { 'Low' }
                5 { 'Critical' }
                6 { 'Charging' }
                7 { 'Charging / high' }
                8 { 'Charging / low' }
                9 { 'Charging / critical' }
                10 { 'Undefined' }
                11 { 'Partially charged' }
                default { "Status $($_.BatteryStatus)" }
            }
            $parts = @("$name · $pct · $status")
            if ($_.DesignCapacity) { $parts += "$($_.DesignCapacity) mWh design" }
            if ($_.FullChargeCapacity) { $parts += "$($_.FullChargeCapacity) mWh full" }
            $parts -join ' · '
        }"#,
    );

    let ac_line_status = query_ps(
        r#"$b = Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $b) { 'Online (no battery device)' }
elseif ($b.BatteryStatus -eq 1) { 'Offline (on battery)' }
elseif ($b.BatteryStatus -eq 2 -or $b.BatteryStatus -eq 3 -or $b.BatteryStatus -ge 6) { 'Online (AC power)' }
else { 'Unknown' }"#,
    )
    .unwrap_or_else(|| {
        if batteries.is_empty() {
            "Online (desktop / no battery reported)".into()
        } else {
            "Unknown".into()
        }
    });

    let mut info = SystemInformation {
        uptime: uptime.clone(),
        cpu_name: cpu_name.clone(),
        cpu_cores: cores,
        cpu_threads: threads,
        cpu_usage: sample.cpu,
        memory_total: sample.memory_total,
        memory_used: sample.memory_used,
        memory_percent: sample.memory_percent,
        disk_total: sample.disk_total,
        disk_used: sample.disk_used,
        disk_percent: sample.disk_percent,
        gpu_name: gpu
            .as_ref()
            .map(|g| g.name.clone())
            .unwrap_or_else(|| "Not Available".into()),
        gpu_usage: gpu.as_ref().map(|g| g.utilization),
        gpu_memory_used: gpu.as_ref().map(|g| g.memory_used),
        gpu_memory_total: gpu.as_ref().map(|g| g.memory_total),
        gpu_temperature: gpu.as_ref().map(|g| g.temperature),
        motherboard: motherboard.clone(),
        bios: bios.clone(),
        os_edition: os_edition.clone(),
        os_version: os_version.clone(),
        os_build: os_build.clone(),
        hostname: hostname.clone(),
        username: username.clone(),
        monitors: monitors.clone(),
        storage_devices: storage_devices.clone(),
        power_plan: power_plan.clone(),
        power_supplies: power_supplies.clone(),
        batteries: batteries.clone(),
        ac_line_status: ac_line_status.clone(),
        copy_text: String::new(),
    };

    let psu = if power_supplies.is_empty() {
        "Not reported by Windows (common on desktop PCs)".to_string()
    } else {
        power_supplies.join("; ")
    };
    let batt = if batteries.is_empty() {
        "None reported".to_string()
    } else {
        batteries.join("; ")
    };

    info.copy_text = format!(
        "PC Toolkit Pro System Info\n\
         Host: {hostname}\nUser: {username}\n\
         OS: {os_edition} {os_version} (Build {os_build})\n\
         Uptime: {uptime}\n\
         CPU: {cpu_name} ({cores}C/{threads}T) — {cpu:.1}%\n\
         Memory: {mem_used:.1} / {mem_total:.1} GB ({mem_pct:.1}%)\n\
         Disk C: {disk_used:.1} / {disk_total:.1} GB ({disk_pct:.1}%)\n\
         GPU: {gpu}\n\
         Motherboard: {motherboard}\nBIOS: {bios}\n\
         Power plan: {power_plan}\nAC: {ac_line_status}\n\
         Power supply: {psu}\nBattery: {batt}\n\
         Monitors: {monitors}\n\
         Storage: {storage}\n",
        cpu = sample.cpu,
        mem_used = sample.memory_used as f64 / 1e9,
        mem_total = sample.memory_total as f64 / 1e9,
        mem_pct = sample.memory_percent,
        disk_used = sample.disk_used as f64 / 1e9,
        disk_total = sample.disk_total as f64 / 1e9,
        disk_pct = sample.disk_percent,
        gpu = info.gpu_name,
        monitors = monitors.join(", "),
        storage = storage_devices.join(", "),
    );

    Ok(info)
}

fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let mins = (seconds % 3600) / 60;
    let secs = seconds % 60;
    format!("{days}d {hours}h {mins}m {secs}s")
}

fn query_ps(expression: &str) -> Option<String> {
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", expression]);
    crate::process::hide_console(&mut cmd);
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn query_ps_list(expression: &str) -> Vec<String> {
    query_ps(expression)
        .map(|s| {
            s.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}
