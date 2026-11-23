//! Volume and physical disk facts for Care (Disk health / Disk optimization), plus the Windows
//! tools that act on them.
//!
//! Facts are read-only and work without administrator rights where Windows allows it:
//! free space and file system (`GetDiskFreeSpaceExW` / `GetVolumeInformationW`), Storage WMI
//! (`Get-PhysicalDisk` / `Get-Disk` health, media and bus type, via one hidden PowerShell call,
//! cached briefly), seek penalty (`IOCTL_STORAGE_QUERY_PROPERTY`, fallback media detection),
//! TRIM (`DisableDeleteNotification`), Optimize Drives' last run (`Dfrg\Statistics`), and the
//! `ScheduledDefrag` task state. The volume dirty bit (`FSCTL_IS_VOLUME_DIRTY`) needs a volume
//! handle that Windows only grants to administrators; a denial is reported as `AccessDenied`,
//! never guessed.
//!
//! Actions only run Windows' own tools (`chkdsk /scan`, `chkntfs /c`, `defrag /O`,
//! `schtasks /Change /ENABLE`, `fsutil behavior set DisableDeleteNotify 0`). They need an
//! elevated process; callers check elevation first and never elevate silently. Nothing here
//! ever disables scheduled optimization.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::winget::{run_hidden, RunFailure};

// ------------------------------------------------------------------------------------------------
// Volumes

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixedVolume {
    /// Upper-case drive letter.
    pub letter: char,
    /// `NTFS`, `ReFS`, `FAT32`, `exFAT`, … when readable.
    pub file_system: Option<String>,
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// `Volume{GUID}` (the key name Optimize Drives uses for its statistics).
    pub volume_guid: Option<String>,
}

/// Local fixed volumes that are mounted and readable (locked BitLocker volumes are left out
/// because their size cannot be read).
pub fn list_fixed_volumes() -> Vec<FixedVolume> {
    crate::fixed_drive_roots()
        .into_iter()
        .filter_map(|root| {
            let letter = root.to_string_lossy().chars().next()?.to_ascii_uppercase();
            imp::volume_facts(letter)
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum VolumeDirtyState {
    Clean,
    /// Windows marked the volume as possibly corrupted; it wants a check.
    Dirty,
    /// Opening the volume for the query needs administrator rights.
    AccessDenied,
    Unknown,
}

/// The NTFS/ReFS dirty bit of `letter:` (`FSCTL_IS_VOLUME_DIRTY`).
pub fn volume_dirty_state(letter: char) -> VolumeDirtyState {
    imp::dirty_state(letter)
}

/// True when the volume's device reports a seek penalty (rotational HDD), false for SSD-like
/// devices, `None` when the device does not answer.
pub fn volume_incurs_seek_penalty(letter: char) -> Option<bool> {
    imp::seek_penalty(letter)
}

// ------------------------------------------------------------------------------------------------
// Storage WMI facts

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageMediaKind {
    Ssd,
    Hdd,
    /// Storage-class memory.
    Scm,
    Unspecified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageHealth {
    Healthy,
    Unknown,
    Warning,
    Unhealthy,
}

/// Physical disk facts for the disk holding one drive letter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeDiskFacts {
    pub letter: char,
    pub disk_number: u32,
    pub media: StorageMediaKind,
    pub bus_type: Option<String>,
    /// Worse of `Get-Disk` and `Get-PhysicalDisk` HealthStatus.
    pub health: StorageHealth,
    /// `Get-PhysicalDisk` OperationalStatus values (e.g. `OK`, `Predictive Failure`).
    pub operational_status: Vec<String>,
    pub friendly_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiskToolStatus {
    Ok,
    Timeout,
    Cancelled,
    ToolUnavailable,
    InvalidOutput,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskToolOutcome {
    pub status: DiskToolStatus,
    pub exit_code: Option<i32>,
}

impl DiskToolOutcome {
    fn from_run(result: Result<crate::winget::HiddenOutput, RunFailure>) -> (Self, String) {
        match result {
            Ok(out) => {
                let status = if out.exit_code == Some(0) { DiskToolStatus::Ok } else { DiskToolStatus::Failed };
                (Self { status, exit_code: out.exit_code }, out.stdout)
            }
            Err(failure) => (
                Self { status: failure_status(failure), exit_code: None },
                String::new(),
            ),
        }
    }
}

fn failure_status(failure: RunFailure) -> DiskToolStatus {
    match failure {
        RunFailure::NotFound => DiskToolStatus::ToolUnavailable,
        RunFailure::Timeout => DiskToolStatus::Timeout,
        RunFailure::Cancelled => DiskToolStatus::Cancelled,
        RunFailure::SpawnFailed => DiskToolStatus::Failed,
    }
}

const STORAGE_FACTS_SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$pd = @{}
foreach ($p in @(Get-PhysicalDisk)) { $pd[[string]$p.DeviceId] = $p }
$out = foreach ($part in @(Get-Partition | Where-Object { [string]$_.DriveLetter -match '^[A-Za-z]$' })) {
  $d = Get-Disk -Number $part.DiskNumber
  $p = $pd[[string]$part.DiskNumber]
  [pscustomobject]@{
    letter = [string]$part.DriveLetter
    disk = [int]$part.DiskNumber
    media = [string]$p.MediaType
    bus = [string]$d.BusType
    diskHealth = [string]$d.HealthStatus
    physicalHealth = [string]$p.HealthStatus
    op = @($p.OperationalStatus | ForEach-Object { [string]$_ })
    name = [string]$d.FriendlyName
  }
}
ConvertTo-Json -Compress -Depth 3 -InputObject @($out)
"#;

const STORAGE_FACTS_TTL: Duration = Duration::from_secs(60);

type StorageFactsCache = Mutex<Option<(Instant, Result<Vec<VolumeDiskFacts>, DiskToolStatus>)>>;

fn storage_facts_cache() -> &'static StorageFactsCache {
    static CACHE: std::sync::OnceLock<StorageFactsCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Storage WMI facts per drive letter. One hidden PowerShell call (non-admin), cached for a
/// minute so parallel Care categories share it.
pub fn volume_disk_facts(should_cancel: &dyn Fn() -> bool) -> Result<Vec<VolumeDiskFacts>, DiskToolStatus> {
    let mut guard = storage_facts_cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, cached)) = guard.as_ref() {
        if at.elapsed() < STORAGE_FACTS_TTL {
            return cached.clone();
        }
    }
    let result = run_hidden(
        &powershell_exe(),
        &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", STORAGE_FACTS_SCRIPT],
        Duration::from_secs(25),
        should_cancel,
        None,
    );
    let facts = match result {
        Ok(out) if out.exit_code == Some(0) => parse_storage_facts(&out.stdout).ok_or(DiskToolStatus::InvalidOutput),
        Ok(_) => Err(DiskToolStatus::Failed),
        Err(failure) => Err(failure_status(failure)),
    };
    // A cancelled run is not cached; the next scan retries.
    if facts != Err(DiskToolStatus::Cancelled) {
        *guard = Some((Instant::now(), facts.clone()));
    }
    facts
}

#[derive(Deserialize)]
struct RawStorageFacts {
    letter: String,
    disk: u32,
    #[serde(default)]
    media: String,
    #[serde(default)]
    bus: String,
    #[serde(default, rename = "diskHealth")]
    disk_health: String,
    #[serde(default, rename = "physicalHealth")]
    physical_health: String,
    #[serde(default)]
    op: Vec<String>,
    #[serde(default)]
    name: String,
}

fn parse_media(text: &str) -> StorageMediaKind {
    match text.trim().to_ascii_uppercase().as_str() {
        "SSD" | "4" => StorageMediaKind::Ssd,
        "HDD" | "3" => StorageMediaKind::Hdd,
        "SCM" | "5" => StorageMediaKind::Scm,
        _ => StorageMediaKind::Unspecified,
    }
}

fn parse_health(text: &str) -> StorageHealth {
    match text.trim().to_ascii_lowercase().as_str() {
        "healthy" | "0" => StorageHealth::Healthy,
        "warning" | "1" => StorageHealth::Warning,
        "unhealthy" | "2" => StorageHealth::Unhealthy,
        _ => StorageHealth::Unknown,
    }
}

fn non_empty(text: String) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Parse the JSON array printed by [`STORAGE_FACTS_SCRIPT`]. `None` when it is not valid JSON.
pub(crate) fn parse_storage_facts(stdout: &str) -> Option<Vec<VolumeDiskFacts>> {
    let text = stdout.trim().trim_start_matches('\u{feff}');
    if text.is_empty() {
        return Some(Vec::new());
    }
    let raw: Vec<RawStorageFacts> = serde_json::from_str(text).ok()?;
    let mut out = Vec::new();
    for r in raw {
        let Some(letter) = r.letter.trim().chars().next().filter(char::is_ascii_alphabetic) else {
            continue;
        };
        let disk_health = parse_health(&r.disk_health);
        let physical_health = parse_health(&r.physical_health);
        // Prefer a known value; when both are known, report the worse one.
        let health = match (disk_health, physical_health) {
            (StorageHealth::Unknown, other) | (other, StorageHealth::Unknown) => other,
            (a, b) => a.max(b),
        };
        out.push(VolumeDiskFacts {
            letter: letter.to_ascii_uppercase(),
            disk_number: r.disk,
            media: parse_media(&r.media),
            bus_type: non_empty(r.bus),
            health,
            operational_status: r.op.into_iter().filter(|s| !s.trim().is_empty()).collect(),
            friendly_name: non_empty(r.name),
        });
    }
    Some(out)
}

// ------------------------------------------------------------------------------------------------
// Optimization facts

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrimState {
    /// `Some(true)` when NTFS sends TRIM (`DisableDeleteNotification` = 0 or absent).
    pub ntfs_enabled: Option<bool>,
    pub refs_enabled: Option<bool>,
}

const FILE_SYSTEM_KEY: &str = r"SYSTEM\CurrentControlSet\Control\FileSystem";

/// TRIM (delete notification) state from the registry, readable without admin.
pub fn trim_state() -> TrimState {
    use crate::{read_registry_value, RegistryKeyPath, RegistryRoot, RegistryView};
    let key = RegistryKeyPath::new(RegistryRoot::LocalMachine, RegistryView::Native64, FILE_SYSTEM_KEY);
    let flag = |name: &str| -> Option<bool> {
        if !crate::registry_key_exists(&key) {
            return None;
        }
        // Absent value = Windows default (TRIM on).
        Some(read_registry_value(&key, name).map_or(true, |v| v.trim() == "0"))
    };
    TrimState {
        ntfs_enabled: flag("DisableDeleteNotification"),
        refs_enabled: flag("RefsDisableDeleteNotification"),
    }
}

/// Convert a little-endian `SYSTEMTIME` blob to Unix seconds (treated as UTC; day precision is
/// what callers need). `None` for a zeroed or malformed value.
pub(crate) fn systemtime_blob_to_unix(blob: &[u8]) -> Option<i64> {
    if blob.len() < 16 {
        return None;
    }
    let w = |i: usize| u16::from_le_bytes([blob[i * 2], blob[i * 2 + 1]]) as i64;
    let (year, month, day, hour, minute, second) = (w(0), w(1), w(3), w(4), w(5), w(6));
    if !(1990..=9999).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    // Days from civil (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// When Optimize Drives last finished on this volume (`Dfrg\Statistics\Volume{GUID}\LastRunTime`).
pub fn last_optimization_unix(volume_guid: &str) -> Option<i64> {
    if !volume_guid.starts_with("Volume{") || volume_guid.contains('\\') {
        return None;
    }
    let blob = imp::read_hklm_binary(&format!(r"SOFTWARE\Microsoft\Dfrg\Statistics\{volume_guid}"), "LastRunTime")?;
    systemtime_blob_to_unix(&blob)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScheduledOptimizationState {
    Enabled,
    Disabled,
    /// The task does not exist (removed by a "tuning" tool or a stripped image).
    Missing,
    Unknown,
}

pub const SCHEDULED_DEFRAG_TASK: &str = r"\Microsoft\Windows\Defrag\ScheduledDefrag";

/// `ScheduledDefrag` task state from its XML definition (`schtasks /Query /XML`, non-admin).
pub fn scheduled_optimization_state() -> ScheduledOptimizationState {
    let result = run_hidden(
        &system32("schtasks.exe"),
        &["/Query", "/TN", SCHEDULED_DEFRAG_TASK, "/XML"],
        Duration::from_secs(15),
        &|| false,
        None,
    );
    match result {
        Ok(out) if out.exit_code == Some(0) => parse_task_enabled(&out.stdout),
        // schtasks exits 1 for a task that does not exist (and for other errors); only call it
        // missing when the output is empty, i.e. no definition was printed.
        Ok(out) if out.stdout.trim().is_empty() => ScheduledOptimizationState::Missing,
        _ => ScheduledOptimizationState::Unknown,
    }
}

/// Whether a Task Scheduler XML definition is enabled: `<Settings><Enabled>false</Enabled>`
/// disables it; absent means enabled (schema default). Trigger-level `<Enabled>` is ignored.
pub(crate) fn parse_task_enabled(xml: &str) -> ScheduledOptimizationState {
    let Some(start) = xml.find("<Settings>") else {
        return ScheduledOptimizationState::Unknown;
    };
    let Some(len) = xml[start..].find("</Settings>") else {
        return ScheduledOptimizationState::Unknown;
    };
    let mut settings = xml[start + "<Settings>".len()..start + len].to_string();
    // Drop nested blocks so only direct children of <Settings> remain.
    for block in ["IdleSettings", "MaintenanceSettings", "NetworkSettings", "RestartOnFailure"] {
        let open = format!("<{block}>");
        let close = format!("</{block}>");
        while let Some(a) = settings.find(&open) {
            match settings[a..].find(&close) {
                Some(b) => settings.replace_range(a..a + b + close.len(), ""),
                None => break,
            }
        }
    }
    let compact: String = settings.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.contains("<Enabled>false</Enabled>") {
        ScheduledOptimizationState::Disabled
    } else {
        ScheduledOptimizationState::Enabled
    }
}

// ------------------------------------------------------------------------------------------------
// Actions (administrator only; callers check elevation first)

fn system32(name: &str) -> PathBuf {
    crate::registry::system32_exe(name)
}

fn powershell_exe() -> PathBuf {
    crate::registry::powershell_exe()
}

fn drive_arg(letter: char) -> Option<String> {
    letter.is_ascii_alphabetic().then(|| format!("{}:", letter.to_ascii_uppercase()))
}

/// Re-enable the built-in `ScheduledDefrag` task (`schtasks /Change /ENABLE`).
pub fn enable_scheduled_optimization() -> DiskToolOutcome {
    let result = run_hidden(
        &system32("schtasks.exe"),
        &["/Change", "/TN", SCHEDULED_DEFRAG_TASK, "/ENABLE"],
        Duration::from_secs(30),
        &|| false,
        None,
    );
    DiskToolOutcome::from_run(result).0
}

/// Re-enable NTFS TRIM (`fsutil behavior set DisableDeleteNotify NTFS 0`, Windows default).
pub fn enable_ntfs_trim() -> DiskToolOutcome {
    let result = run_hidden(
        &system32("fsutil.exe"),
        &["behavior", "set", "DisableDeleteNotify", "NTFS", "0"],
        Duration::from_secs(30),
        &|| false,
        None,
    );
    DiskToolOutcome::from_run(result).0
}

/// Last `NN%` in a chunk of tool output.
pub(crate) fn last_percent(text: &str) -> Option<u8> {
    let bytes = text.as_bytes();
    let mut found = None;
    for (i, b) in bytes.iter().enumerate() {
        if *b != b'%' {
            continue;
        }
        let mut start = i;
        while start > 0 && bytes[start - 1].is_ascii_digit() && i - start < 3 {
            start -= 1;
        }
        if start < i {
            if let Ok(value) = text[start..i].parse::<u16>() {
                if value <= 100 {
                    found = Some(value as u8);
                }
            }
        }
    }
    found
}

/// Run Windows Optimize Drives on one volume (`defrag X: /O /U`): ReTrim on SSDs, defragment on
/// HDDs — Windows picks the right operation. `on_progress` receives percent values when the tool
/// prints them. Cancel stops the client, which ends the optimization pass safely.
pub fn optimize_volume(
    letter: char,
    timeout: Duration,
    should_cancel: &dyn Fn() -> bool,
    on_progress: Arc<dyn Fn(u8) + Send + Sync>,
) -> DiskToolOutcome {
    let Some(drive) = drive_arg(letter) else {
        return DiskToolOutcome { status: DiskToolStatus::Failed, exit_code: None };
    };
    let sink: Arc<dyn Fn(&[u8]) + Send + Sync> = Arc::new(move |chunk: &[u8]| {
        if let Some(p) = last_percent(&String::from_utf8_lossy(chunk)) {
            on_progress(p);
        }
    });
    let result = run_hidden(&system32("defrag.exe"), &[&drive, "/O", "/U"], timeout, should_cancel, Some(sink));
    DiskToolOutcome::from_run(result).0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OnlineScanResult {
    /// `chkdsk` found no problems (exit 0).
    NoErrors,
    /// Problems were found and fixed online (exit 1), or cleanup was performed (exit 2).
    Fixed,
    /// Problems need an offline repair (exit 3): schedule a check at restart.
    OfflineRepairNeeded,
    NotRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineScanOutcome {
    pub result: OnlineScanResult,
    pub tool: DiskToolOutcome,
}

pub(crate) fn online_scan_result(exit_code: Option<i32>) -> OnlineScanResult {
    match exit_code {
        Some(0) => OnlineScanResult::NoErrors,
        Some(1) | Some(2) => OnlineScanResult::Fixed,
        Some(3) => OnlineScanResult::OfflineRepairNeeded,
        _ => OnlineScanResult::NotRun,
    }
}

/// Online NTFS scan (`chkdsk X: /scan`): runs while the volume stays mounted.
pub fn online_scan_volume(letter: char, timeout: Duration, should_cancel: &dyn Fn() -> bool) -> OnlineScanOutcome {
    let Some(drive) = drive_arg(letter) else {
        return OnlineScanOutcome {
            result: OnlineScanResult::NotRun,
            tool: DiskToolOutcome { status: DiskToolStatus::Failed, exit_code: None },
        };
    };
    let result = run_hidden(&system32("chkdsk.exe"), &[&drive, "/scan"], timeout, should_cancel, None);
    let (mut tool, _) = DiskToolOutcome::from_run(result);
    let scan = if tool.status == DiskToolStatus::Failed || tool.status == DiskToolStatus::Ok {
        online_scan_result(tool.exit_code)
    } else {
        OnlineScanResult::NotRun
    };
    // chkdsk exit 1..3 are results, not tool failures.
    if scan != OnlineScanResult::NotRun {
        tool.status = DiskToolStatus::Ok;
    }
    OnlineScanOutcome { result: scan, tool }
}

/// Ask Windows to check the volume at the next restart when it is dirty (`chkntfs /c X:`).
pub fn schedule_boot_check(letter: char) -> DiskToolOutcome {
    let Some(drive) = drive_arg(letter) else {
        return DiskToolOutcome { status: DiskToolStatus::Failed, exit_code: None };
    };
    let result = run_hidden(&system32("chkntfs.exe"), &["/c", &drive], Duration::from_secs(30), &|| false, None);
    DiskToolOutcome::from_run(result).0
}

// ------------------------------------------------------------------------------------------------
// Win32

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_SUCCESS, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, GetDiskFreeSpaceExW, GetVolumeInformationW, GetVolumeNameForVolumeMountPointW,
        FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE,
        KEY_WOW64_64KEY, REG_BINARY, REG_VALUE_TYPE,
    };
    use windows::Win32::System::IO::DeviceIoControl;

    const FSCTL_IS_VOLUME_DIRTY: u32 = 0x0009_0078;
    const VOLUME_IS_DIRTY: u32 = 0x0000_0001;
    const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
    const STORAGE_DEVICE_SEEK_PENALTY_PROPERTY: u32 = 7;
    const PROPERTY_STANDARD_QUERY: u32 = 0;
    const GENERIC_READ: u32 = 0x8000_0000;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub(super) fn volume_facts(letter: char) -> Option<FixedVolume> {
        let root = wide(&format!("{letter}:\\"));
        let mut free = 0u64;
        let mut total = 0u64;
        unsafe { GetDiskFreeSpaceExW(PCWSTR(root.as_ptr()), Some(&mut free), Some(&mut total), None) }.ok()?;
        let mut fs_name = [0u16; 64];
        let file_system = unsafe {
            GetVolumeInformationW(PCWSTR(root.as_ptr()), None, None, None, None, Some(&mut fs_name))
        }
        .ok()
        .map(|_| {
            let end = fs_name.iter().position(|c| *c == 0).unwrap_or(fs_name.len());
            String::from_utf16_lossy(&fs_name[..end])
        })
        .filter(|s| !s.is_empty());
        let mut guid_buf = [0u16; 64];
        let volume_guid = unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(root.as_ptr()), &mut guid_buf) }
            .ok()
            .and_then(|_| {
                let end = guid_buf.iter().position(|c| *c == 0).unwrap_or(guid_buf.len());
                let name = String::from_utf16_lossy(&guid_buf[..end]);
                let start = name.find("Volume{")?;
                let close = name[start..].find('}')?;
                Some(name[start..start + close + 1].to_string())
            });
        Some(FixedVolume { letter, file_system, total_bytes: total, free_bytes: free, volume_guid })
    }

    struct OwnedHandle(HANDLE);
    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    fn open_volume(letter: char, access: u32) -> Result<OwnedHandle, bool> {
        let path = wide(&format!("\\\\.\\{letter}:"));
        let handle = unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        };
        match handle {
            Ok(h) if !h.is_invalid() => Ok(OwnedHandle(h)),
            _ => Err(unsafe { GetLastError() } == ERROR_ACCESS_DENIED),
        }
    }

    pub(super) fn dirty_state(letter: char) -> VolumeDirtyState {
        let mut denied = false;
        // Try a query-only handle first, then a read handle (the latter needs admin for volumes).
        for access in [0u32, GENERIC_READ] {
            let handle = match open_volume(letter, access) {
                Ok(h) => h,
                Err(was_denied) => {
                    denied |= was_denied;
                    continue;
                }
            };
            let mut flags = 0u32;
            let mut returned = 0u32;
            let ok = unsafe {
                DeviceIoControl(
                    handle.0,
                    FSCTL_IS_VOLUME_DIRTY,
                    None,
                    0,
                    Some(&mut flags as *mut u32 as *mut _),
                    std::mem::size_of::<u32>() as u32,
                    Some(&mut returned),
                    None,
                )
            };
            match ok {
                Ok(()) if returned >= 4 => {
                    return if flags & VOLUME_IS_DIRTY != 0 { VolumeDirtyState::Dirty } else { VolumeDirtyState::Clean };
                }
                Err(e) if e.code() == ERROR_ACCESS_DENIED.to_hresult() => denied = true,
                _ => {}
            }
        }
        if denied {
            VolumeDirtyState::AccessDenied
        } else {
            VolumeDirtyState::Unknown
        }
    }

    #[repr(C)]
    struct StoragePropertyQuery {
        property_id: u32,
        query_type: u32,
        additional: [u8; 4],
    }

    #[repr(C)]
    #[derive(Default)]
    struct SeekPenaltyDescriptor {
        version: u32,
        size: u32,
        incurs_seek_penalty: u8,
        _pad: [u8; 3],
    }

    pub(super) fn seek_penalty(letter: char) -> Option<bool> {
        let handle = open_volume(letter, 0).ok()?;
        let query = StoragePropertyQuery {
            property_id: STORAGE_DEVICE_SEEK_PENALTY_PROPERTY,
            query_type: PROPERTY_STANDARD_QUERY,
            additional: [0; 4],
        };
        let mut out = SeekPenaltyDescriptor::default();
        let mut returned = 0u32;
        unsafe {
            DeviceIoControl(
                handle.0,
                IOCTL_STORAGE_QUERY_PROPERTY,
                Some(&query as *const _ as *const _),
                std::mem::size_of::<StoragePropertyQuery>() as u32,
                Some(&mut out as *mut _ as *mut _),
                std::mem::size_of::<SeekPenaltyDescriptor>() as u32,
                Some(&mut returned),
                None,
            )
        }
        .ok()?;
        (returned >= 9 && out.size >= 9).then_some(out.incurs_seek_penalty != 0)
    }

    pub(super) fn read_hklm_binary(subkey: &str, name: &str) -> Option<Vec<u8>> {
        let mut key = HKEY::default();
        let sub = HSTRING::from(subkey);
        let status = unsafe {
            RegOpenKeyExW(HKEY_LOCAL_MACHINE, &sub, Some(0), KEY_QUERY_VALUE | KEY_WOW64_64KEY, &mut key)
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let value = HSTRING::from(name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut data = vec![0u8; 64];
        let mut size = data.len() as u32;
        let status = unsafe {
            RegQueryValueExW(key, &value, None, Some(&mut kind), Some(data.as_mut_ptr()), Some(&mut size))
        };
        unsafe {
            let _ = RegCloseKey(key);
        }
        if status != ERROR_SUCCESS || kind != REG_BINARY {
            return None;
        }
        data.truncate(size as usize);
        Some(data)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub(super) fn volume_facts(_letter: char) -> Option<FixedVolume> {
        None
    }
    pub(super) fn dirty_state(_letter: char) -> VolumeDirtyState {
        VolumeDirtyState::Unknown
    }
    pub(super) fn seek_penalty(_letter: char) -> Option<bool> {
        None
    }
    pub(super) fn read_hklm_binary(_subkey: &str, _name: &str) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_facts_json_parses_and_takes_worse_health() {
        let json = r#"[{"letter":"C","disk":0,"media":"SSD","bus":"NVMe","diskHealth":"Healthy","physicalHealth":"Warning","op":["OK","Predictive Failure"],"name":"Disk A"},
                       {"letter":"d","disk":1,"media":"HDD","bus":"SATA","diskHealth":"","physicalHealth":"Healthy","op":[],"name":""},
                       {"letter":"","disk":2,"media":"","bus":"","diskHealth":"","physicalHealth":"","op":[],"name":""}]"#;
        let facts = parse_storage_facts(json).expect("json");
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0].media, StorageMediaKind::Ssd);
        assert_eq!(facts[0].health, StorageHealth::Warning);
        assert_eq!(facts[0].operational_status, vec!["OK", "Predictive Failure"]);
        assert_eq!(facts[1].letter, 'D');
        assert_eq!(facts[1].media, StorageMediaKind::Hdd);
        assert_eq!(facts[1].health, StorageHealth::Healthy);
        assert_eq!(facts[1].friendly_name, None);
        assert_eq!(parse_storage_facts("[]"), Some(Vec::new()));
        assert_eq!(parse_storage_facts("not json"), None);
    }

    #[test]
    fn systemtime_blob_converts() {
        // 2026-11-19 00:16:25 (bytes as stored by Optimize Drives).
        let blob = [234u8, 7, 11, 0, 4, 0, 19, 0, 0, 0, 16, 0, 25, 0, 27, 3];
        assert_eq!(systemtime_blob_to_unix(&blob), Some(1_795_047_385));
        assert_eq!(systemtime_blob_to_unix(&[0u8; 16]), None);
        assert_eq!(systemtime_blob_to_unix(&[1u8; 4]), None);
    }

    #[test]
    fn task_xml_enabled_state() {
        let enabled = "<Task><Triggers><CalendarTrigger><Enabled>false</Enabled></CalendarTrigger></Triggers><Settings>\n  <IdleSettings>\n  </IdleSettings>\n  <MaintenanceSettings>\n  </MaintenanceSettings>\n</Settings></Task>";
        assert_eq!(parse_task_enabled(enabled), ScheduledOptimizationState::Enabled);
        let disabled = "<Task><Settings><IdleSettings><StopOnIdleEnd>true</StopOnIdleEnd></IdleSettings>\n <Enabled>false</Enabled>\n</Settings></Task>";
        assert_eq!(parse_task_enabled(disabled), ScheduledOptimizationState::Disabled);
        assert_eq!(parse_task_enabled("garbage"), ScheduledOptimizationState::Unknown);
    }

    #[test]
    fn percent_parsing() {
        assert_eq!(last_percent("\tRetrim:  12% complete...\r\tRetrim:  47% complete..."), Some(47));
        assert_eq!(last_percent("100%"), Some(100));
        assert_eq!(last_percent("no progress"), None);
        assert_eq!(last_percent("1234%"), None);
    }

    #[test]
    fn chkdsk_exit_codes() {
        assert_eq!(online_scan_result(Some(0)), OnlineScanResult::NoErrors);
        assert_eq!(online_scan_result(Some(1)), OnlineScanResult::Fixed);
        assert_eq!(online_scan_result(Some(3)), OnlineScanResult::OfflineRepairNeeded);
        assert_eq!(online_scan_result(Some(7)), OnlineScanResult::NotRun);
    }

    #[test]
    #[ignore = "reads this machine's disks"]
    fn smoke_disk_facts() {
        let started = Instant::now();
        let volumes = list_fixed_volumes();
        let facts = volume_disk_facts(&|| false);
        eprintln!(
            "volumes={} facts={:?} trim={:?} task={:?} dirtyC={:?} seekC={:?} lastC={:?} elapsed={:?}",
            volumes.len(),
            facts.as_ref().map(|f| f.iter().map(|x| (x.letter, x.media, x.health)).collect::<Vec<_>>()),
            trim_state(),
            scheduled_optimization_state(),
            volume_dirty_state('C'),
            volume_incurs_seek_penalty('C'),
            volumes.first().and_then(|v| v.volume_guid.as_deref()).and_then(last_optimization_unix),
            started.elapsed()
        );
    }
}
