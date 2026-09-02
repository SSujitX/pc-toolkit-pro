use serde::{Deserialize, Serialize};
use std::process::Command;

use crate::gpu::sample_nvidia;
use crate::monitor::sample_monitor;
use crate::PlatformResult;

/// Typed collection stages — UI maps `phase` to locale strings.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfoProgress {
    pub phase: String,
    pub current: u64,
    pub total: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemInformation {
    pub uptime: String,
    pub cpu_name: String,
    pub cpu_cores: u32,
    pub cpu_threads: u32,
    pub cpu_usage: f32,
    pub cpu_frequency: String,
    pub cpu_cache: String,
    pub cpu_socket: String,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_available: u64,
    pub memory_percent: f32,
    pub ram_name: String,
    pub ram_type: String,
    pub ram_speed: String,
    pub ram_slots_used: String,
    pub disk_total: u64,
    pub disk_used: u64,
    pub disk_free: u64,
    pub disk_percent: f32,
    pub disk_device: String,
    pub disk_type: String,
    pub gpu_name: String,
    pub gpu_usage: Option<f32>,
    pub gpu_memory_used: Option<u64>,
    pub gpu_memory_total: Option<u64>,
    pub gpu_temperature: Option<f32>,
    pub motherboard_product: String,
    pub motherboard_manufacturer: String,
    pub motherboard_version: String,
    pub motherboard: String,
    pub chipset: String,
    pub bios: String,
    pub bios_version: String,
    pub bios_manufacturer: String,
    pub bios_date: String,
    pub system_model: String,
    pub memory_slots_total: String,
    pub max_memory_capacity: String,
    pub os_edition: String,
    pub os_version: String,
    pub os_build: String,
    pub os_experience: String,
    pub hostname: String,
    pub username: String,
    pub monitors: Vec<String>,
    pub storage_devices: Vec<String>,
    pub power_supply_name: String,
    pub power_plan: String,
    pub power_supplies: Vec<String>,
    pub batteries: Vec<String>,
    pub ac_line_status: String,
    pub copy_text: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct HwFacts {
    cpu_name: String,
    cpu_cores: u32,
    cpu_threads: u32,
    cpu_max_clock_mhz: f64,
    cpu_current_clock_mhz: f64,
    cpu_l2_kb: u32,
    cpu_l3_kb: u32,
    cpu_socket: String,
    ram_names: Vec<String>,
    ram_type: String,
    ram_speed: String,
    ram_slots_used: u32,
    memory_slots_total: u32,
    max_memory_gb: f64,
    disk_device: String,
    disk_type: String,
    disk_total_gb: f64,
    disk_free_gb: f64,
    storage_devices: Vec<String>,
    monitors: Vec<String>,
    gpu_name: String,
    board_product: String,
    board_manufacturer: String,
    board_version: String,
    chipset: String,
    bios_version: String,
    bios_manufacturer: String,
    bios_date: String,
    system_model: String,
    os_edition: String,
    os_version: String,
    os_build: String,
    os_experience: String,
    power_supply_name: String,
    power_supplies: Vec<String>,
    batteries: Vec<String>,
    power_plan: String,
    ac_line_status: String,
}

pub fn load_system_information() -> PlatformResult<SystemInformation> {
    load_system_information_with_progress(|_| {})
}

pub fn load_system_information_with_progress(
    mut on_progress: impl FnMut(SystemInfoProgress),
) -> PlatformResult<SystemInformation> {
    const TOTAL: u64 = 4;

    on_progress(SystemInfoProgress {
        phase: "metrics".into(),
        current: 1,
        total: TOTAL,
        message: "sampling live metrics".into(),
    });
    let sample = sample_monitor()?;
    let uptime = format_uptime(sample.uptime_seconds);

    on_progress(SystemInfoProgress {
        phase: "hardware".into(),
        current: 2,
        total: TOTAL,
        message: "querying hardware inventory".into(),
    });
    let facts = query_hw_facts().unwrap_or_default();

    on_progress(SystemInfoProgress {
        phase: "gpu".into(),
        current: 3,
        total: TOTAL,
        message: "reading graphics sensors".into(),
    });
    let nvidia = sample_nvidia().ok();

    on_progress(SystemInfoProgress {
        phase: "assemble".into(),
        current: 4,
        total: TOTAL,
        message: "building report".into(),
    });

    let cpu_name = non_empty(&facts.cpu_name).unwrap_or_else(|| "Unknown CPU".into());
    let cores = if facts.cpu_cores > 0 {
        facts.cpu_cores
    } else {
        sysinfo::System::physical_core_count().unwrap_or(0) as u32
    };
    let threads = if facts.cpu_threads > 0 {
        facts.cpu_threads
    } else {
        let mut sys = sysinfo::System::new();
        sys.refresh_cpu_all();
        sys.cpus().len() as u32
    };

    let freq_cur = if facts.cpu_current_clock_mhz > 0.0 {
        facts.cpu_current_clock_mhz / 1000.0
    } else {
        0.0
    };
    let freq_max = if facts.cpu_max_clock_mhz > 0.0 {
        facts.cpu_max_clock_mhz / 1000.0
    } else {
        freq_cur
    };
    let cpu_frequency = if freq_max > 0.0 {
        format!("{freq_cur:.2} GHz (Max: {freq_max:.2} GHz)")
    } else {
        "Unknown".into()
    };

    // L1 is rarely exposed cleanly via CIM; report L2/L3 and estimate L1 when missing.
    let l1 = if cores > 0 { cores * 64 } else { 0 };
    let cpu_cache = format!(
        "L1 - {} | L2 - {} | L3 - {}",
        format_cache_kb(l1),
        format_cache_kb(facts.cpu_l2_kb),
        format_cache_kb(facts.cpu_l3_kb)
    );

    let ram_name = if facts.ram_names.is_empty() {
        "Unknown".into()
    } else {
        facts.ram_names.join(", ")
    };

    let memory_available = sample.memory_total.saturating_sub(sample.memory_used);
    let disk_free = sample.disk_total.saturating_sub(sample.disk_used);

    let gpu_name = nvidia
        .as_ref()
        .map(|g| g.name.clone())
        .or_else(|| non_empty(&facts.gpu_name))
        .unwrap_or_else(|| "No dedicated GPU reported".into());

    let motherboard = format!(
        "{} {}",
        facts.board_manufacturer.trim(),
        facts.board_product.trim()
    )
    .trim()
    .to_string();
    let motherboard = if motherboard.is_empty() {
        "Unknown".into()
    } else {
        motherboard
    };

    let bios = format!(
        "{} {}",
        facts.bios_manufacturer.trim(),
        facts.bios_version.trim()
    )
    .trim()
    .to_string();
    let bios = if bios.is_empty() {
        "Unknown".into()
    } else {
        bios
    };

    let hostname = sysinfo::System::host_name().unwrap_or_default();
    let username = std::env::var("USERNAME").unwrap_or_default();

    let power_supply_name = non_empty(&facts.power_supply_name)
        .unwrap_or_else(|| "Not reported by Windows".into());

    let mut info = SystemInformation {
        uptime: uptime.clone(),
        cpu_name: cpu_name.clone(),
        cpu_cores: cores,
        cpu_threads: threads,
        cpu_usage: sample.cpu,
        cpu_frequency: cpu_frequency.clone(),
        cpu_cache: cpu_cache.clone(),
        cpu_socket: non_empty(&facts.cpu_socket).unwrap_or_else(|| "Unknown".into()),
        memory_total: sample.memory_total,
        memory_used: sample.memory_used,
        memory_available,
        memory_percent: sample.memory_percent,
        ram_name: ram_name.clone(),
        ram_type: non_empty(&facts.ram_type).unwrap_or_else(|| "Unknown".into()),
        ram_speed: non_empty(&facts.ram_speed).unwrap_or_else(|| "Unknown".into()),
        ram_slots_used: if facts.ram_slots_used > 0 {
            format!("{} slot(s) used", facts.ram_slots_used)
        } else {
            "Unknown".into()
        },
        disk_total: sample.disk_total,
        disk_used: sample.disk_used,
        disk_free,
        disk_percent: sample.disk_percent,
        disk_device: non_empty(&facts.disk_device).unwrap_or_else(|| "Unknown".into()),
        disk_type: non_empty(&facts.disk_type).unwrap_or_else(|| "Unknown".into()),
        gpu_name: gpu_name.clone(),
        gpu_usage: nvidia.as_ref().map(|g| g.utilization),
        gpu_memory_used: nvidia.as_ref().map(|g| g.memory_used),
        gpu_memory_total: nvidia.as_ref().map(|g| g.memory_total),
        gpu_temperature: nvidia.as_ref().map(|g| g.temperature),
        motherboard_product: non_empty(&facts.board_product).unwrap_or_else(|| "Unknown".into()),
        motherboard_manufacturer: non_empty(&facts.board_manufacturer)
            .unwrap_or_else(|| "Unknown".into()),
        motherboard_version: non_empty(&facts.board_version).unwrap_or_else(|| "Unknown".into()),
        motherboard: motherboard.clone(),
        chipset: non_empty(&facts.chipset).unwrap_or_else(|| "Unknown".into()),
        bios: bios.clone(),
        bios_version: non_empty(&facts.bios_version).unwrap_or_else(|| "Unknown".into()),
        bios_manufacturer: non_empty(&facts.bios_manufacturer)
            .unwrap_or_else(|| "Unknown".into()),
        bios_date: non_empty(&facts.bios_date).unwrap_or_else(|| "Unknown".into()),
        system_model: non_empty(&facts.system_model).unwrap_or_else(|| "Unknown".into()),
        memory_slots_total: if facts.memory_slots_total > 0 {
            facts.memory_slots_total.to_string()
        } else {
            "Unknown".into()
        },
        max_memory_capacity: if facts.max_memory_gb > 0.0 {
            format!("{:.0} GB", facts.max_memory_gb)
        } else {
            "Unknown".into()
        },
        os_edition: non_empty(&facts.os_edition).unwrap_or_else(|| sample.os_label.clone()),
        os_version: facts.os_version.clone(),
        os_build: facts.os_build.clone(),
        os_experience: non_empty(&facts.os_experience).unwrap_or_else(|| "Unknown".into()),
        hostname: hostname.clone(),
        username: username.clone(),
        monitors: if facts.monitors.is_empty() {
            vec!["Unknown".into()]
        } else {
            facts.monitors.clone()
        },
        storage_devices: facts.storage_devices.clone(),
        power_supply_name: power_supply_name.clone(),
        power_plan: non_empty(&facts.power_plan).unwrap_or_else(|| "Unknown".into()),
        power_supplies: if facts.power_supplies.is_empty() {
            vec![power_supply_name.clone()]
        } else {
            facts.power_supplies.clone()
        },
        batteries: facts.batteries.clone(),
        ac_line_status: non_empty(&facts.ac_line_status)
            .unwrap_or_else(|| "Unknown".into()),
        copy_text: String::new(),
    };

    info.copy_text = build_copy_text(&info);
    Ok(info)
}

fn build_copy_text(info: &SystemInformation) -> String {
    let mem_total = info.memory_total as f64 / 1e9;
    let mem_used = info.memory_used as f64 / 1e9;
    let mem_avail = info.memory_available as f64 / 1e9;
    let disk_total = info.disk_total as f64 / 1e9;
    let disk_used = info.disk_used as f64 / 1e9;
    let disk_free = info.disk_free as f64 / 1e9;

    let gpu_mem = match (info.gpu_memory_used, info.gpu_memory_total) {
        (Some(u), Some(t)) if t > 0 => format!("{:.1} / {:.1} GB", u as f64 / 1e9, t as f64 / 1e9),
        _ => "N/A".into(),
    };
    let gpu_temp = info
        .gpu_temperature
        .map(|t| format!("{t:.0}°C"))
        .unwrap_or_else(|| "N/A".into());

    let mut storage_block = String::new();
    if info.storage_devices.is_empty() {
        storage_block.push_str("None reported\n");
    } else {
        storage_block.push_str(&format!(
            "Total Storage Devices: {}\n",
            info.storage_devices.len()
        ));
        for (i, s) in info.storage_devices.iter().enumerate() {
            storage_block.push_str(&format!("Storage {}: {}\n", i + 1, s));
        }
    }

    let mut monitor_block = String::new();
    monitor_block.push_str(&format!("Monitor Count: {}\n", info.monitors.len()));
    for (i, m) in info.monitors.iter().enumerate() {
        monitor_block.push_str(&format!("Monitor {}: {}\n", i + 1, m));
    }

    let battery_block = if info.batteries.is_empty() {
        "None reported".to_string()
    } else {
        info.batteries.join("\n")
    };

    format!(
        "PC Toolkit Pro — System Information\n\
         ================================\n\n\
         Processor Information:\n\
         ----------------------\n\
         Processor: {cpu}\n\
         Cores/Threads: {cores} cores, {threads} threads\n\
         Frequency: {freq}\n\
         Cache: {cache}\n\
         Sockets: {socket}\n\
         Usage: {cpu_usage:.1}%\n\n\
         Local Disk (C:) Information:\n\
         ----------------------\n\
         Storage Device: {disk_dev}\n\
         Storage Type: {disk_type}\n\
         Local Disk (C:) Total: {disk_used:.1} GB / {disk_total:.1} GB\n\
         Local Disk (C:) Free: {disk_free:.1} GB\n\
         Local Disk (C:) Usage: {disk_pct:.1}%\n\n\
         Memory Information:\n\
         ----------------------\n\
         Ram Total: {mem_total:.1} GB\n\
         Ram Used: {mem_used:.1} GB\n\
         Ram Available: {mem_avail:.1} GB\n\
         RAM Name: {ram_name}\n\
         RAM Type: {ram_type}\n\
         RAM Speed: {ram_speed}\n\
         RAM Slots: {ram_slots}\n\n\
         Storage Information:\n\
         ----------------------\n\
         {storage_block}\n\
         Graphics Information:\n\
         ----------------------\n\
         GPU: {gpu}\n\
         GPU Memory: {gpu_mem}\n\
         GPU Temperature: {gpu_temp}\n\n\
         Monitor Information:\n\
         ----------------------\n\
         {monitor_block}\n\
         Motherboard Information:\n\
         ----------------------\n\
         Product: {board_product}\n\
         Manufacturer: {board_mfr}\n\
         Version: {board_ver}\n\
         Chipset: {chipset}\n\
         BIOS Version: {bios_ver}\n\
         BIOS Manufacturer: {bios_mfr}\n\
         BIOS Date: {bios_date}\n\
         System Model: {sys_model}\n\
         Total Memory Slots: {mem_slots}\n\
         Max Memory Capacity: {max_mem}\n\
         Memory Slots Used: {ram_used_count}\n\n\
         Power Supply Information:\n\
         ----------------------\n\
         Power Supply: {psu}\n\
         Power Plan: {plan}\n\
         AC Line: {ac}\n\
         Battery: {battery}\n\n\
         Operating System Information:\n\
         ----------------------\n\
         Device Name: {host}\n\
         User: {user}\n\
         Operating System: {os}\n\
         OS Version: {os_ver}\n\
         OS Build: {os_build}\n\
         OS Experience: {os_exp}\n\
         Uptime: {uptime}\n",
        cpu = info.cpu_name,
        cores = info.cpu_cores,
        threads = info.cpu_threads,
        freq = info.cpu_frequency,
        cache = info.cpu_cache,
        socket = info.cpu_socket,
        cpu_usage = info.cpu_usage,
        disk_dev = info.disk_device,
        disk_type = info.disk_type,
        disk_pct = info.disk_percent,
        ram_name = info.ram_name,
        ram_type = info.ram_type,
        ram_speed = info.ram_speed,
        ram_slots = info.ram_slots_used,
        gpu = info.gpu_name,
        board_product = info.motherboard_product,
        board_mfr = info.motherboard_manufacturer,
        board_ver = info.motherboard_version,
        chipset = info.chipset,
        bios_ver = info.bios_version,
        bios_mfr = info.bios_manufacturer,
        bios_date = info.bios_date,
        sys_model = info.system_model,
        mem_slots = info.memory_slots_total,
        max_mem = info.max_memory_capacity,
        ram_used_count = info.ram_slots_used,
        psu = info.power_supply_name,
        plan = info.power_plan,
        ac = info.ac_line_status,
        battery = battery_block,
        host = info.hostname,
        user = info.username,
        os = info.os_edition,
        os_ver = info.os_version,
        os_build = info.os_build,
        os_exp = info.os_experience,
        uptime = info.uptime,
    )
}

fn query_hw_facts() -> Option<HwFacts> {
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$cs = Get-CimInstance Win32_ComputerSystem | Select-Object -First 1
$bb = Get-CimInstance Win32_BaseBoard | Select-Object -First 1
$bios = Get-CimInstance Win32_BIOS | Select-Object -First 1
$pa = Get-CimInstance Win32_PhysicalMemoryArray | Select-Object -First 1
$mems = @(Get-CimInstance Win32_PhysicalMemory)
$disk = Get-CimInstance Win32_LogicalDisk -Filter "DeviceID='C:'" | Select-Object -First 1
$parts = Get-CimInstance Win32_DiskDriveToDiskPartition -ErrorAction SilentlyContinue
$ldp = Get-CimInstance Win32_LogicalDiskToPartition -ErrorAction SilentlyContinue |
  Where-Object { $_.Dependent -match 'DeviceID="C:"' } | Select-Object -First 1
$diskDrive = $null
if ($ldp) {
  $part = ($ldp.Antecedent -replace '.*DeviceID="([^"]+)".*','$1')
  $map = $parts | Where-Object { $_.Dependent -match [regex]::Escape($part) } | Select-Object -First 1
  if ($map) {
    $ddid = ($map.Antecedent -replace '.*DeviceID="([^"]+)".*','$1')
    $diskDrive = Get-CimInstance Win32_DiskDrive | Where-Object { $_.DeviceID -eq $ddid } | Select-Object -First 1
  }
}
if (-not $diskDrive) { $diskDrive = Get-CimInstance Win32_DiskDrive | Select-Object -First 1 }

$media = 'Unknown'
if ($diskDrive) {
  if ($diskDrive.Model -match 'NVMe|SSD') { $media = if ($diskDrive.Model -match 'NVMe') { 'NVMe SSD' } else { 'SSD' } }
  elseif ($diskDrive.InterfaceType -eq 'SCSI' -and $diskDrive.Model -match 'SSD') { $media = 'SSD' }
  elseif ($diskDrive.MediaType -match 'Fixed') {
    $bustype = (Get-PhysicalDisk -ErrorAction SilentlyContinue | Where-Object { $_.FriendlyName -eq $diskDrive.Model } | Select-Object -First 1).MediaType
    if ($bustype) { $media = "$bustype" } else { $media = 'HDD/SSD' }
  }
}

$ramTypeMap = @{ 20='DDR'; 21='DDR2'; 24='DDR3'; 26='DDR4'; 34='DDR5' }
$ramType = 'Unknown'
$ramSpeed = 'Unknown'
$ramNames = @()
foreach ($m in $mems) {
  if ($m.PartNumber) { $ramNames += ($m.PartNumber.Trim()) }
  elseif ($m.Manufacturer) { $ramNames += ($m.Manufacturer.Trim()) }
  if ($ramTypeMap.ContainsKey([int]$m.SMBIOSMemoryType)) { $ramType = $ramTypeMap[[int]$m.SMBIOSMemoryType] }
  if ($m.ConfiguredClockSpeed -gt 0) { $ramSpeed = "$($m.ConfiguredClockSpeed) MHz" }
  elseif ($m.Speed -gt 0) { $ramSpeed = "$($m.Speed) MHz" }
}
$ramNames = $ramNames | Where-Object { $_ } | Select-Object -Unique

$chip = 'Unknown'
$ide = Get-CimInstance Win32_IDEController -ErrorAction SilentlyContinue | Select-Object -First 1
if ($ide -and $ide.Name) { $chip = $ide.Name }
else {
  $pci = Get-CimInstance Win32_PnPEntity -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match 'Chipset|Host Bridge|PCH' } |
    Select-Object -First 1
  if ($pci) { $chip = $pci.Name }
}

$monitors = @()
try {
  Add-Type -AssemblyName System.Windows.Forms -ErrorAction SilentlyContinue
  $i = 0
  [System.Windows.Forms.Screen]::AllScreens | ForEach-Object {
    $i++
    $b = $_.Bounds
    $primary = if ($_.Primary) { ' (Primary)' } else { '' }
    $name = 'Display'
    $monitors += ("{0} | {1}x{2}{3}" -f $name, $b.Width, $b.Height, $primary)
  }
} catch {}
$cimMon = @(Get-CimInstance Win32_DesktopMonitor -ErrorAction SilentlyContinue | Where-Object { $_.Name })
$vcMon = @(Get-CimInstance WmiMonitorID -Namespace root\wmi -ErrorAction SilentlyContinue)
if ($cimMon.Count -gt 0 -or $vcMon.Count -gt 0) {
  $monitors = @()
  $n = 0
  if ($vcMon.Count -gt 0) {
    foreach ($m in $vcMon) {
      $n++
      $name = (($m.UserFriendlyName | Where-Object { $_ -ne 0 } | ForEach-Object { [char]$_ }) -join '').Trim()
      $mfr = (($m.ManufacturerName | Where-Object { $_ -ne 0 } | ForEach-Object { [char]$_ }) -join '').Trim()
      if (-not $name) { $name = "Monitor $n" }
      if (-not $mfr) { $mfr = 'Unknown' }
      $monitors += ("{0} | {1}" -f $name, $mfr)
    }
  } else {
    foreach ($m in $cimMon) {
      $n++
      $monitors += ("{0} | {1}" -f $m.Name, $(if ($m.MonitorManufacturer) { $m.MonitorManufacturer } else { 'Unknown' }))
    }
  }
}
# Merge resolution from screens when counts match
try {
  $screens = [System.Windows.Forms.Screen]::AllScreens
  for ($i = 0; $i -lt [Math]::Min($monitors.Count, $screens.Count); $i++) {
    $b = $screens[$i].Bounds
    $primary = if ($screens[$i].Primary) { ' (Primary)' } else { '' }
    $monitors[$i] = ("{0} | {1}x{2}{3}" -f $monitors[$i], $b.Width, $b.Height, $primary)
  }
} catch {}
$vc = Get-CimInstance Win32_VideoController -ErrorAction SilentlyContinue | Where-Object { $_.Name -and $_.Name -notmatch 'Microsoft Basic' }
$gpuName = if ($vc) { ($vc | Select-Object -First 1).Name } else { 'Unknown' }

$storage = @()
$idx = 0
Get-CimInstance Win32_DiskDrive | ForEach-Object {
  $idx++
  $gb = if ($_.Size) { [math]::Round($_.Size/1GB,1) } else { 0 }
  $storage += ("{0} - {1} GB" -f $_.Model.Trim(), $gb)
}

$psuNames = @()
Get-CimInstance Win32_PowerSupply -ErrorAction SilentlyContinue | ForEach-Object {
  foreach ($v in @($_.Name, $_.Description, $_.Caption)) { if ($v) { $psuNames += $v.Trim() } }
}
Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue |
  Where-Object { $_.FriendlyName -match 'Power Supply|PSU|UPS|AC Adapter|Charger|Power Adapter' } |
  ForEach-Object { $psuNames += $_.FriendlyName.Trim() }
$psuNames = $psuNames | Where-Object { $_ -and $_ -notmatch '^(Root|ACPI)' } | Select-Object -Unique
$psu = if ($psuNames) { $psuNames[0] } else { 'Not reported by Windows' }

$batteries = @()
Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | ForEach-Object {
  $pct = if ($null -ne $_.EstimatedChargeRemaining) { "$($_.EstimatedChargeRemaining)%" } else { 'n/a' }
  $batteries += ("{0} · {1}" -f $(if ($_.Name) { $_.Name } else { 'Battery' }), $pct)
}

$plan = $null
try { $plan = (Get-CimInstance -Namespace root\cimv2\power -ClassName Win32_PowerPlan -Filter "IsActive='true'").ElementName } catch {}
if (-not $plan) {
  $m = powercfg /getactivescheme 2>$null
  if ($m -match '\((.+)\)') { $plan = $Matches[1] }
}
$ac = 'Unknown'
$b = Get-CimInstance Win32_Battery | Select-Object -First 1
if (-not $b) { $ac = 'Online (no battery device)' }
elseif ($b.BatteryStatus -eq 1) { $ac = 'Offline (on battery)' }
elseif ($b.BatteryStatus -eq 2 -or $b.BatteryStatus -eq 3 -or $b.BatteryStatus -ge 6) { $ac = 'Online (AC power)' }

$cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
$maxMemGb = 0
if ($pa -and $pa.MaxCapacity) { $maxMemGb = [math]::Round($pa.MaxCapacity / 1MB, 0) }

[pscustomobject]@{
  cpuName = $cpu.Name
  cpuCores = [uint32]$cpu.NumberOfCores
  cpuThreads = [uint32]$cpu.NumberOfLogicalProcessors
  cpuMaxClockMhz = [double]$cpu.MaxClockSpeed
  cpuCurrentClockMhz = [double]$cpu.CurrentClockSpeed
  cpuL2Kb = [uint32]$cpu.L2CacheSize
  cpuL3Kb = [uint32]$cpu.L3CacheSize
  cpuSocket = $cpu.SocketDesignation
  ramNames = @($ramNames)
  ramType = $ramType
  ramSpeed = $ramSpeed
  ramSlotsUsed = [uint32]$mems.Count
  memorySlotsTotal = [uint32]$(if ($pa) { $pa.MemoryDevices } else { 0 })
  maxMemoryGb = [double]$maxMemGb
  diskDevice = $(if ($diskDrive) { $diskDrive.Model.Trim() } else { 'Unknown' })
  diskType = $media
  diskTotalGb = [double]$(if ($disk -and $disk.Size) { [math]::Round($disk.Size/1GB,1) } else { 0 })
  diskFreeGb = [double]$(if ($disk -and $disk.FreeSpace) { [math]::Round($disk.FreeSpace/1GB,1) } else { 0 })
  storageDevices = @($storage)
  monitors = @($monitors)
  gpuName = $gpuName
  boardProduct = $(if ($bb) { $bb.Product } else { '' })
  boardManufacturer = $(if ($bb) { $bb.Manufacturer } else { '' })
  boardVersion = $(if ($bb) { $bb.Version } else { '' })
  chipset = $chip
  biosVersion = $(if ($bios) { $bios.SMBIOSBIOSVersion } else { '' })
  biosManufacturer = $(if ($bios) { $bios.Manufacturer } else { '' })
  biosDate = $(if ($bios) { $bios.ReleaseDate } else { '' })
  systemModel = $(if ($cs) { $cs.Model } else { '' })
  osEdition = $cv.ProductName
  osVersion = $cv.DisplayVersion
  osBuild = ("{0}.{1}" -f $cv.CurrentBuild, $cv.UBR)
  osExperience = ("Windows Feature Experience Pack {0}.{1}" -f $cv.CurrentBuild, $cv.UBR)
  powerSupplyName = $psu
  powerSupplies = @($psuNames)
  batteries = @($batteries)
  powerPlan = $(if ($plan) { $plan } else { 'Unknown' })
  acLineStatus = $ac
} | ConvertTo-Json -Compress
"#;

    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", script]);
    crate::process::hide_console(&mut cmd);
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        return None;
    }
    let mut facts: HwFacts = serde_json::from_str(&text).ok()?;
    if let Some(d) = query_ps(
        r#"$d=(Get-CimInstance Win32_BIOS).ReleaseDate; if ($d) { $d.ToString('MM/dd/yyyy') }"#,
    ) {
        facts.bios_date = d;
    }
    Some(facts)
}

fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let mins = (seconds % 3600) / 60;
    let secs = seconds % 60;
    format!("{days}d {hours}h {mins}m {secs}s")
}

fn format_cache_kb(kb: u32) -> String {
    if kb == 0 {
        return "N/A".into();
    }
    if kb >= 1024 {
        let mb = kb as f64 / 1024.0;
        if mb >= 10.0 {
            format!("{mb:.1} MB")
        } else {
            format!("{mb:.1} MB")
        }
    } else {
        format!("{kb} KB")
    }
}

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("unknown") {
        None
    } else {
        Some(t.to_string())
    }
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
