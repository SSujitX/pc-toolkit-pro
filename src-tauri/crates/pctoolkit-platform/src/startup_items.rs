//! Startup entries as Windows sees them: Run / RunOnce values, Startup folder files, scheduled
//! tasks with logon / boot triggers, and auto-start Win32 services, plus the switches Windows
//! itself uses to turn them off.
//!
//! Enabled state matches Task Manager: Run values and Startup folder files are disabled through
//! `Explorer\StartupApproved\{Run,Run32,StartupFolder}` (the entry itself stays, so the change is
//! fully reversible). Tasks use `IRegisteredTask::put_Enabled`; services change their start type
//! through the Service Control Manager. Machine-wide changes need an elevated process and report
//! `PermissionDenied` instead of elevating. No product policy lives here (what to show, what to
//! recommend, undo bookkeeping) — that belongs to `pctoolkit_core::startup`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::registry::{self, RegistryKeyPath, RegistryRoot, RegistryView};
use crate::{PlatformError, PlatformResult};

const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_ONCE_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce";
const APPROVED_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved";
const SERVICES_SUBKEY: &str = "SYSTEM\\CurrentControlSet\\Services";
/// Unix epoch expressed in FILETIME ticks (100 ns since 1601-01-01).
const FILETIME_UNIX_EPOCH: i64 = 116_444_736_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupSource {
    Run,
    RunOnce,
    StartupFolder,
    ScheduledTask,
    Service,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupScope {
    User,
    Machine,
}

/// Service start type as stored in the SCM database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceStartKind {
    Automatic,
    AutomaticDelayed,
    Manual,
    Disabled,
    /// Boot / system start (drivers); never offered for change.
    Other,
}

/// The `StartupApproved` value Task Manager writes next to a Run value or Startup folder file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApprovedState {
    pub enabled: bool,
    /// When the entry was disabled (FILETIME in the value), Unix ms.
    pub changed_at_ms: Option<i64>,
}

/// Which `StartupApproved` subkey governs an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovedList {
    /// `Run` (HKCU Run and native HKLM Run).
    Run,
    /// `Run32` (HKLM `WOW6432Node` Run).
    Run32,
    StartupFolder,
}

impl ApprovedList {
    fn leaf(self) -> &'static str {
        match self {
            Self::Run => "Run",
            Self::Run32 => "Run32",
            Self::StartupFolder => "StartupFolder",
        }
    }
}

/// Where an entry lives; enough to change or remove it again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum StartupLocation {
    Registry {
        key: RegistryKeyPath,
        value_name: String,
        /// `None` for RunOnce (Windows offers no disable switch for it).
        approved: Option<ApprovedList>,
    },
    Folder {
        file: PathBuf,
        approved: ApprovedList,
    },
    Task {
        path: String,
    },
    Service {
        name: String,
    },
}

/// One startup entry as read from the system.
#[derive(Debug, Clone)]
pub struct StartupEntryFact {
    pub source: StartupSource,
    pub scope: StartupScope,
    /// Registry value name, file name, task path, or service key name.
    pub entry_name: String,
    /// Service display name / task leaf name; otherwise the entry name.
    pub label: String,
    /// Command line (display only).
    pub command: String,
    /// Executable (or DLL for `rundll32`) the entry starts, when it can be determined.
    pub target: Option<PathBuf>,
    pub location: StartupLocation,
    pub enabled: bool,
    pub disabled_at_ms: Option<i64>,
    pub service_start: Option<ServiceStartKind>,
    /// Task path under `\Microsoft\` (Windows' own tasks).
    pub microsoft_task: bool,
    /// Task runs as SYSTEM / a service account / with highest privileges: changing it needs admin.
    pub task_privileged: bool,
}

/// Version-resource facts of an executable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileVersionFacts {
    pub company: Option<String>,
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Pure helpers (unit-tested)
// ---------------------------------------------------------------------------------------------

/// Decode a `StartupApproved` value. Task Manager writes `02`/`06` (enabled) or `03` (disabled)
/// in the first byte followed by a FILETIME of the change; an odd first byte means disabled.
pub fn parse_approved_bytes(bytes: &[u8]) -> Option<ApprovedState> {
    let first = *bytes.first()?;
    let enabled = first & 1 == 0;
    let changed_at_ms = if bytes.len() >= 12 {
        let mut raw = [0u8; 8];
        raw.copy_from_slice(&bytes[4..12]);
        let ticks = u64::from_le_bytes(raw) as i64;
        (ticks > FILETIME_UNIX_EPOCH).then(|| (ticks - FILETIME_UNIX_EPOCH) / 10_000)
    } else {
        None
    };
    Some(ApprovedState { enabled, changed_at_ms: if enabled { None } else { changed_at_ms } })
}

/// Encode a `StartupApproved` value exactly like Task Manager: `02` + zeros when enabled,
/// `03 00 00 00` + FILETIME of the change when disabled.
pub fn approved_bytes(enabled: bool, now_unix_ms: i64) -> [u8; 12] {
    let mut out = [0u8; 12];
    if enabled {
        out[0] = 0x02;
    } else {
        out[0] = 0x03;
        let ticks = (now_unix_ms.max(0) * 10_000 + FILETIME_UNIX_EPOCH) as u64;
        out[4..12].copy_from_slice(&ticks.to_le_bytes());
    }
    out
}

/// Expand `%VAR%` references with `env`; unknown variables are left as written.
pub fn expand_env_vars(text: &str, env: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 && !after[..end].contains(['\\', ' ', '"']) => {
                let name = &after[..end];
                match env(name) {
                    Some(value) => out.push_str(&value),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Determine which file a startup command line runs. Handles quoted / unquoted paths with
/// spaces, environment variables, NT prefixes used by service `ImagePath` values (`\??\`,
/// `\SystemRoot\`, relative `system32\`), bare System32 programs, and `rundll32 <dll>,Entry`
/// (the DLL is the real target). Returns `None` when the target cannot be determined honestly.
pub fn command_target(
    command: &str,
    env: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&str) -> bool,
) -> Option<PathBuf> {
    let system_root = env("SystemRoot").unwrap_or_else(|| "C:\\Windows".to_string());
    let mut text = expand_env_vars(command.trim(), env);
    if let Some(rest) = text.strip_prefix("\\??\\") {
        text = rest.to_string();
    }
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("\\systemroot\\") {
        text = format!("{}{}", system_root, &text["\\SystemRoot".len()..]);
    } else if lower.starts_with("system32\\") || lower.starts_with("syswow64\\") {
        text = format!("{}\\{}", system_root, text);
    }
    let (exe, args) = crate::split_command_line(&text, |p| exists(p))?;
    let exe = resolve_bare_program(&exe, &system_root, exists)?;
    let file_name = Path::new(&exe)
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if file_name == "rundll32.exe" {
        let dll = args.trim().trim_start_matches('"');
        let dll = dll.split([',', '"']).next().unwrap_or("").trim();
        if dll.is_empty() {
            return Some(PathBuf::from(exe));
        }
        return resolve_bare_program(dll, &system_root, exists).map(PathBuf::from);
    }
    if exe.contains('%') {
        // An unresolved variable would make the path look missing; refuse to guess.
        return None;
    }
    Some(PathBuf::from(exe))
}

/// A program named without a folder (`rundll32.exe`, `ctfmon`) resolves through System32.
fn resolve_bare_program(exe: &str, system_root: &str, exists: &dyn Fn(&str) -> bool) -> Option<String> {
    let exe = exe.trim().trim_matches('"');
    if exe.is_empty() {
        return None;
    }
    if exe.contains('\\') || exe.contains('/') {
        return Some(exe.to_string());
    }
    let with_ext = if Path::new(exe).extension().is_some() {
        exe.to_string()
    } else {
        format!("{exe}.exe")
    };
    for dir in ["System32", ""] {
        let candidate = if dir.is_empty() {
            format!("{system_root}\\{with_ext}")
        } else {
            format!("{system_root}\\{dir}\\{with_ext}")
        };
        if exists(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Facts from a task definition XML (what Task Scheduler stores).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskDefinitionFacts {
    pub logon_trigger: bool,
    pub boot_trigger: bool,
    /// `<Exec>` actions as (command, arguments).
    pub exec: Vec<(String, String)>,
    pub privileged: bool,
}

/// Parse the parts of a task XML that matter for startup: logon / boot triggers (disabled
/// triggers ignored), exec actions, and whether the principal needs admin to change it.
pub fn parse_task_definition(xml: &str) -> TaskDefinitionFacts {
    let mut facts = TaskDefinitionFacts::default();
    for (tag, slot) in [("LogonTrigger", 0usize), ("BootTrigger", 1usize)] {
        let mut cursor = xml;
        while let Some(body) = next_element(cursor, tag) {
            cursor = body.1;
            let enabled = tag_text(body.0, "Enabled").map(|v| v.trim() != "false").unwrap_or(true);
            if enabled {
                if slot == 0 {
                    facts.logon_trigger = true;
                } else {
                    facts.boot_trigger = true;
                }
            }
        }
    }
    let mut cursor = xml;
    while let Some((body, rest)) = next_element(cursor, "Exec") {
        cursor = rest;
        if let Some(command) = tag_text(body, "Command") {
            let args = tag_text(body, "Arguments").unwrap_or_default();
            facts.exec.push((xml_unescape(command.trim()), xml_unescape(args.trim())));
        }
    }
    if let Some((principal, _)) = next_element(xml, "Principal") {
        let user = tag_text(principal, "UserId").unwrap_or_default().to_ascii_lowercase();
        let run_level = tag_text(principal, "RunLevel").unwrap_or_default();
        let group = tag_text(principal, "GroupId").is_some();
        let account = user.trim().rsplit('\\').next().unwrap_or("").to_string();
        let service_account = [
            "s-1-5-18",
            "s-1-5-19",
            "s-1-5-20",
            "system",
            "localsystem",
            "local service",
            "network service",
            "localservice",
            "networkservice",
        ]
        .contains(&account.as_str());
        facts.privileged = service_account || run_level.trim() == "HighestAvailable" || group;
    }
    facts
}

/// Body of the next `<tag ...>body</tag>` (or empty body for `<tag/>`) and the remaining text.
fn next_element<'a>(xml: &'a str, tag: &str) -> Option<(&'a str, &'a str)> {
    let open = format!("<{tag}");
    let mut search = xml;
    loop {
        let start = search.find(&open)?;
        let after = &search[start + open.len()..];
        let next = after.chars().next()?;
        if next != '>' && next != ' ' && next != '/' && next != '\r' && next != '\n' && next != '\t' {
            search = after;
            continue;
        }
        let gt = after.find('>')?;
        if after[..gt].ends_with('/') {
            return Some(("", &after[gt + 1..]));
        }
        let body = &after[gt + 1..];
        let close = format!("</{tag}>");
        let end = body.find(&close)?;
        return Some((&body[..end], &body[end + close.len()..]));
    }
}

fn tag_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    next_element(xml, tag).map(|(body, _)| body)
}

fn xml_unescape(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// One event from `Microsoft-Windows-Diagnostics-Performance/Operational`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootPerfEvent {
    /// Event 100: a boot and how long it took.
    Boot { system_time: String, boot_ms: u64, main_path_ms: Option<u64> },
    /// Events 101–103: an app / driver / service that slowed a boot down.
    Degradation {
        system_time: String,
        event_id: u32,
        file_name: String,
        friendly_name: String,
        total_ms: u64,
        degradation_ms: u64,
    },
}

/// Parse a rendered event XML. Unknown ids and malformed events return `None`.
pub fn parse_boot_perf_event(xml: &str) -> Option<BootPerfEvent> {
    let id: u32 = tag_text(xml, "EventID")?.trim().parse().ok()?;
    let system_time = attribute(xml, "TimeCreated", "SystemTime").unwrap_or_default();
    let num = |name: &str| data_value(xml, name).and_then(|v| v.trim().parse::<u64>().ok());
    match id {
        100 => Some(BootPerfEvent::Boot {
            system_time,
            boot_ms: num("BootTime")?,
            main_path_ms: num("MainPathBootTime"),
        }),
        101..=103 => Some(BootPerfEvent::Degradation {
            system_time,
            event_id: id,
            file_name: data_value(xml, "Name").unwrap_or_default(),
            friendly_name: data_value(xml, "FriendlyName").unwrap_or_default(),
            total_ms: num("TotalTime").unwrap_or(0),
            degradation_ms: num("DegradationTime").unwrap_or(0),
        }),
        _ => None,
    }
}

fn data_value(xml: &str, name: &str) -> Option<String> {
    for quote in ['\'', '"'] {
        let needle = format!("Name={quote}{name}{quote}");
        if let Some(pos) = xml.find(&needle) {
            let after = &xml[pos + needle.len()..];
            let gt = after.find('>')?;
            if after[..gt].ends_with('/') {
                return Some(String::new());
            }
            let body = &after[gt + 1..];
            let end = body.find("</Data>")?;
            return Some(xml_unescape(&body[..end]));
        }
    }
    None
}

fn attribute(xml: &str, tag: &str, attr: &str) -> Option<String> {
    let open = format!("<{tag}");
    let start = xml.find(&open)?;
    let after = &xml[start + open.len()..];
    let gt = after.find('>')?;
    let head = &after[..gt];
    for quote in ['\'', '"'] {
        let needle = format!("{attr}={quote}");
        if let Some(pos) = head.find(&needle) {
            let value = &head[pos + needle.len()..];
            let end = value.find(quote)?;
            return Some(value[..end].to_string());
        }
    }
    None
}

/// Task XML for a per-user logon task that starts `exe args` `delay_seconds` after this user
/// signs in, without elevation. Used for "Delay" of a user startup entry.
pub fn delayed_logon_task_xml(user_id: &str, exe: &str, args: &str, delay_seconds: u32, description: &str) -> String {
    let args_xml = if args.trim().is_empty() {
        String::new()
    } else {
        format!("\n      <Arguments>{}</Arguments>", xml_escape(args.trim()))
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>PC Toolkit Pro</Author>
    <Description>{description}</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
      <Delay>PT{delay}S</Delay>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Enabled>true</Enabled>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>{args}
    </Exec>
  </Actions>
</Task>"#,
        description = xml_escape(description),
        user = xml_escape(user_id),
        delay = delay_seconds.clamp(1, 3600),
        exe = xml_escape(exe),
        args = args_xml,
    )
}

// ---------------------------------------------------------------------------------------------
// Inventory
// ---------------------------------------------------------------------------------------------

fn env_lookup(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn path_exists(path: &str) -> bool {
    Path::new(path).exists()
}

fn target_for(command: &str) -> Option<PathBuf> {
    command_target(command, &env_lookup, &path_exists)
}

fn approved_key(root: RegistryRoot, list: ApprovedList) -> RegistryKeyPath {
    RegistryKeyPath::new(root, RegistryView::Native64, format!("{APPROVED_SUBKEY}\\{}", list.leaf()))
}

fn read_approved(root: RegistryRoot, list: ApprovedList, name: &str) -> Option<ApprovedState> {
    read_binary_value(&approved_key(root, list), name).and_then(|b| parse_approved_bytes(&b))
}

fn scope_of(root: RegistryRoot) -> StartupScope {
    match root {
        RegistryRoot::CurrentUser => StartupScope::User,
        RegistryRoot::LocalMachine => StartupScope::Machine,
    }
}

/// Run and RunOnce values of HKCU and HKLM (native and `WOW6432Node` views).
pub fn list_run_entries() -> Vec<StartupEntryFact> {
    let mut out = Vec::new();
    let keys = [
        (RegistryRoot::CurrentUser, RegistryView::Native64, ApprovedList::Run),
        (RegistryRoot::LocalMachine, RegistryView::Native64, ApprovedList::Run),
        (RegistryRoot::LocalMachine, RegistryView::Wow32, ApprovedList::Run32),
    ];
    for (root, view, list) in keys {
        for (once, subkey) in [(false, RUN_SUBKEY), (true, RUN_ONCE_SUBKEY)] {
            let key = RegistryKeyPath::new(root, view, subkey);
            let Ok(values) = registry::values(&key) else { continue };
            for value in values {
                if value.name.is_empty() || value.text.trim().is_empty() {
                    continue;
                }
                let approved = if once { None } else { read_approved(root, list, &value.name) };
                let mut target = target_for(&value.text);
                if view == RegistryView::Wow32 {
                    target = target.map(|t| wow_redirected(&t));
                }
                out.push(StartupEntryFact {
                    source: if once { StartupSource::RunOnce } else { StartupSource::Run },
                    scope: scope_of(root),
                    entry_name: value.name.clone(),
                    label: value.name.clone(),
                    command: value.text.clone(),
                    target,
                    location: StartupLocation::Registry {
                        key: key.clone(),
                        value_name: value.name.clone(),
                        approved: (!once).then_some(list),
                    },
                    enabled: approved.map(|a| a.enabled).unwrap_or(true),
                    disabled_at_ms: approved.and_then(|a| a.changed_at_ms),
                    service_start: None,
                    microsoft_task: false,
                    task_privileged: false,
                });
            }
        }
    }
    out
}

/// A 32-bit entry naming `System32` really runs from `SysWOW64`.
fn wow_redirected(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    let lower = text.to_ascii_lowercase();
    if let Some(pos) = lower.find("\\system32\\") {
        let candidate = format!("{}\\SysWOW64\\{}", &text[..pos], &text[pos + "\\system32\\".len()..]);
        if Path::new(&candidate).exists() {
            return PathBuf::from(candidate);
        }
    }
    path.to_path_buf()
}

/// The per-user and all-users Startup folders.
pub fn startup_folders() -> Vec<(StartupScope, PathBuf)> {
    let mut out = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        out.push((
            StartupScope::User,
            PathBuf::from(appdata).join("Microsoft\\Windows\\Start Menu\\Programs\\Startup"),
        ));
    }
    if let Some(data) = std::env::var_os("ProgramData") {
        out.push((
            StartupScope::Machine,
            PathBuf::from(data).join("Microsoft\\Windows\\Start Menu\\Programs\\StartUp"),
        ));
    }
    out
}

/// Files in the Startup folders (`.lnk` targets resolved; `desktop.ini` ignored).
pub fn list_startup_folder_entries() -> Vec<StartupEntryFact> {
    let mut out = Vec::new();
    for (scope, folder) in startup_folders() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        let root = match scope {
            StartupScope::User => RegistryRoot::CurrentUser,
            StartupScope::Machine => RegistryRoot::LocalMachine,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            let is_link = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lnk"));
            let target = if is_link { crate::resolve_shortcut(&path) } else { Some(path.clone()) };
            let approved = read_approved(root, ApprovedList::StartupFolder, &file_name);
            let label = Path::new(&file_name)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| file_name.clone());
            out.push(StartupEntryFact {
                source: StartupSource::StartupFolder,
                scope,
                entry_name: file_name,
                label,
                command: target
                    .as_ref()
                    .map(|t| t.display().to_string())
                    .unwrap_or_else(|| path.display().to_string()),
                target,
                location: StartupLocation::Folder { file: path, approved: ApprovedList::StartupFolder },
                enabled: approved.map(|a| a.enabled).unwrap_or(true),
                disabled_at_ms: approved.and_then(|a| a.changed_at_ms),
                service_start: None,
                microsoft_task: false,
                task_privileged: false,
            });
        }
    }
    out
}

/// Win32 services (own / shared process; never drivers) with their start type.
pub fn list_service_entries() -> Vec<StartupEntryFact> {
    let root = RegistryKeyPath::new(RegistryRoot::LocalMachine, RegistryView::Native64, SERVICES_SUBKEY);
    let Ok(names) = registry::subkey_names(&root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for name in names {
        let key = root.child(&name);
        let Some(kind) = registry::read_value(&key, "Type").and_then(|v| v.parse::<u32>().ok()) else {
            continue;
        };
        // 0x10 own process / 0x20 shared process. Drivers (0x1 / 0x2) are excluded on purpose.
        if kind & 0x30 == 0 || kind & 0x0F != 0 {
            continue;
        }
        let Some(start) = registry::read_value(&key, "Start").and_then(|v| v.parse::<u32>().ok()) else {
            continue;
        };
        let delayed = registry::read_value(&key, "DelayedAutostart").as_deref() == Some("1");
        let start_kind = match (start, delayed) {
            (2, true) => ServiceStartKind::AutomaticDelayed,
            (2, false) => ServiceStartKind::Automatic,
            (3, _) => ServiceStartKind::Manual,
            (4, _) => ServiceStartKind::Disabled,
            _ => ServiceStartKind::Other,
        };
        let Some(image) = registry::read_value(&key, "ImagePath") else { continue };
        let label = registry::read_value(&key, "DisplayName")
            .filter(|l| !l.trim().is_empty())
            .unwrap_or_else(|| name.clone());
        out.push(StartupEntryFact {
            source: StartupSource::Service,
            scope: StartupScope::Machine,
            entry_name: name.clone(),
            label,
            target: target_for(&image),
            command: image,
            location: StartupLocation::Service { name },
            enabled: matches!(start_kind, ServiceStartKind::Automatic | ServiceStartKind::AutomaticDelayed),
            disabled_at_ms: None,
            service_start: Some(start_kind),
            microsoft_task: false,
            task_privileged: true,
        });
    }
    out
}

/// Resolve `@%SystemRoot%\system32\x.dll,-101` style indirect strings (service display names).
pub fn resolve_indirect_string(text: &str) -> String {
    if !text.starts_with('@') {
        return text.to_string();
    }
    #[cfg(windows)]
    {
        use windows::core::HSTRING;
        use windows::Win32::UI::Shell::SHLoadIndirectString;
        let mut buf = [0u16; 512];
        if unsafe { SHLoadIndirectString(&HSTRING::from(text), &mut buf, None) }.is_ok() {
            let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
            let resolved = String::from_utf16_lossy(&buf[..end]);
            if !resolved.trim().is_empty() {
                return resolved;
            }
        }
    }
    // Driver-package names (`@oem14.inf,%Key%;Fallback text`) carry a readable fallback after `;`.
    match text.rsplit_once(';') {
        Some((_, fallback)) if !fallback.trim().is_empty() => fallback.trim().to_string(),
        _ => text.to_string(),
    }
}

/// Scheduled tasks with an enabled logon or boot trigger.
pub fn list_task_entries() -> Vec<StartupEntryFact> {
    #[cfg(windows)]
    {
        imp::list_tasks()
            .into_iter()
            .filter_map(|(path, enabled, xml)| {
                let facts = parse_task_definition(&xml);
                if !facts.logon_trigger && !facts.boot_trigger {
                    return None;
                }
                let (command, args) = facts.exec.first().cloned().unwrap_or_default();
                let full = if args.is_empty() {
                    command.clone()
                } else if command.contains(' ') && !command.starts_with('"') {
                    format!("\"{command}\" {args}")
                } else {
                    format!("{command} {args}")
                };
                let target = if command.is_empty() { None } else { target_for(&full) };
                let label = path.rsplit('\\').next().unwrap_or(&path).to_string();
                Some(StartupEntryFact {
                    source: StartupSource::ScheduledTask,
                    scope: if facts.privileged { StartupScope::Machine } else { StartupScope::User },
                    entry_name: path.clone(),
                    label,
                    command: full,
                    target,
                    microsoft_task: path.to_ascii_lowercase().starts_with("\\microsoft\\"),
                    location: StartupLocation::Task { path },
                    enabled,
                    disabled_at_ms: None,
                    service_start: None,
                    task_privileged: facts.privileged,
                })
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Company name and description from a file's version resource.
pub fn file_version_facts(path: &Path) -> FileVersionFacts {
    #[cfg(windows)]
    {
        imp::file_version_facts(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        FileVersionFacts::default()
    }
}

// ---------------------------------------------------------------------------------------------
// Mutations
// ---------------------------------------------------------------------------------------------

/// Write the Task Manager-compatible enabled / disabled marker for a Run value or Startup folder
/// file. HKLM markers need an elevated process.
pub fn set_approved(root: RegistryRoot, list: ApprovedList, name: &str, enabled: bool, now_unix_ms: i64) -> PlatformResult<()> {
    write_binary_value(&approved_key(root, list), name, &approved_bytes(enabled, now_unix_ms))
}

/// Remove the `StartupApproved` marker (after the entry itself was deleted).
pub fn clear_approved(root: RegistryRoot, list: ApprovedList, name: &str) -> PlatformResult<()> {
    registry::delete_value(&approved_key(root, list), name)
}

/// Read a raw `REG_BINARY` value.
pub fn read_binary_value(key: &RegistryKeyPath, name: &str) -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        imp::read_binary(key, name)
    }
    #[cfg(not(windows))]
    {
        let _ = (key, name);
        None
    }
}

/// Create the key if needed and write a `REG_BINARY` value.
pub fn write_binary_value(key: &RegistryKeyPath, name: &str, data: &[u8]) -> PlatformResult<()> {
    #[cfg(windows)]
    {
        imp::write_binary(key, name, data)
    }
    #[cfg(not(windows))]
    {
        let _ = (key, name, data);
        Err(PlatformError::Unsupported)
    }
}

/// Enable or disable a scheduled task through Task Scheduler COM.
pub fn set_task_enabled(path: &str, enabled: bool) -> PlatformResult<()> {
    #[cfg(windows)]
    {
        imp::set_task_enabled(path, enabled)
    }
    #[cfg(not(windows))]
    {
        let _ = (path, enabled);
        Err(PlatformError::Unsupported)
    }
}

/// Task definition XML (kept as a backup before deleting a broken task).
pub fn task_xml(path: &str) -> Option<String> {
    #[cfg(windows)]
    {
        imp::task_xml(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

/// Delete a scheduled task through Task Scheduler COM.
pub fn delete_task(path: &str) -> PlatformResult<()> {
    #[cfg(windows)]
    {
        imp::delete_task(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(PlatformError::Unsupported)
    }
}

/// Register (create or replace) a non-elevated per-user logon task in `folder` (created when the
/// user may; otherwise the task goes to the root folder with `folder` as a name prefix).
/// Returns the task's full path.
pub fn register_user_logon_task(folder: &str, name: &str, xml: &str) -> PlatformResult<String> {
    #[cfg(windows)]
    {
        imp::register_task(folder, name, xml)
    }
    #[cfg(not(windows))]
    {
        let _ = (folder, name, xml);
        Err(PlatformError::Unsupported)
    }
}

/// `DOMAIN\user` of the signed-in user, for task principals.
pub fn current_user_id() -> Option<String> {
    let user = std::env::var("USERNAME").ok()?;
    match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => Some(format!("{domain}\\{user}")),
        _ => Some(user),
    }
}

/// Change a Win32 service's start type. Requires administrator rights.
pub fn set_service_start(name: &str, start: ServiceStartKind) -> PlatformResult<()> {
    #[cfg(windows)]
    {
        imp::set_service_start(name, start)
    }
    #[cfg(not(windows))]
    {
        let _ = (name, start);
        Err(PlatformError::Unsupported)
    }
}

/// Outcome of reading the boot performance log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BootPerfAccess {
    Available,
    /// The channel is readable only by administrators / Event Log Readers.
    NeedsAdmin,
    Unavailable,
}

/// Newest-first events 100–103 of `Microsoft-Windows-Diagnostics-Performance/Operational`.
pub fn read_boot_performance(max_events: usize) -> (BootPerfAccess, Vec<BootPerfEvent>) {
    #[cfg(windows)]
    {
        imp::read_boot_performance(max_events)
    }
    #[cfg(not(windows))]
    {
        let _ = max_events;
        (BootPerfAccess::Unavailable, Vec::new())
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::{BSTR, HSTRING, PCWSTR};
    use windows::Win32::Foundation::{ERROR_SUCCESS, VARIANT_BOOL, VARIANT_FALSE, VARIANT_TRUE};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_SET_VALUE, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_BINARY,
        REG_OPTION_NON_VOLATILE, RRF_RT_REG_BINARY, RRF_SUBKEY_WOW6432KEY, RRF_SUBKEY_WOW6464KEY,
    };
    use windows::Win32::System::TaskScheduler::{
        IRegisteredTask, ITaskFolder, ITaskService, TaskScheduler, TASK_CREATE_OR_UPDATE,
        TASK_LOGON_INTERACTIVE_TOKEN,
    };
    use windows::Win32::System::Variant::VARIANT;

    const E_ACCESSDENIED: u32 = 0x8007_0005;
    const NOT_FOUND: [u32; 2] = [0x8007_0002, 0x8007_0003];

    fn root(root: RegistryRoot) -> HKEY {
        match root {
            RegistryRoot::CurrentUser => HKEY_CURRENT_USER,
            RegistryRoot::LocalMachine => HKEY_LOCAL_MACHINE,
        }
    }

    pub fn read_binary(key: &RegistryKeyPath, name: &str) -> Option<Vec<u8>> {
        let view = match key.view {
            RegistryView::Native64 => RRF_SUBKEY_WOW6464KEY,
            RegistryView::Wow32 => RRF_SUBKEY_WOW6432KEY,
        };
        let subkey = HSTRING::from(key.subkey.as_str());
        let value = HSTRING::from(name);
        let mut size = 0u32;
        let status = unsafe {
            RegGetValueW(root(key.root), &subkey, &value, RRF_RT_REG_BINARY | view, None, None, Some(&mut size))
        };
        if status != ERROR_SUCCESS || size == 0 || size > 4096 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        let status = unsafe {
            RegGetValueW(
                root(key.root),
                &subkey,
                &value,
                RRF_RT_REG_BINARY | view,
                None,
                Some(data.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        data.truncate(size as usize);
        Some(data)
    }

    pub fn write_binary(key: &RegistryKeyPath, name: &str, data: &[u8]) -> PlatformResult<()> {
        let view = match key.view {
            RegistryView::Native64 => KEY_WOW64_64KEY,
            RegistryView::Wow32 => KEY_WOW64_32KEY,
        };
        let mut handle = HKEY::default();
        let status = unsafe {
            RegCreateKeyExW(
                root(key.root),
                &HSTRING::from(key.subkey.as_str()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | view,
                None,
                &mut handle,
                None,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(win32_error(status.0));
        }
        let status = unsafe { RegSetValueExW(handle, &HSTRING::from(name), None, REG_BINARY, Some(data)) };
        unsafe {
            let _ = RegCloseKey(handle);
        }
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(win32_error(status.0))
        }
    }

    fn win32_error(code: u32) -> PlatformError {
        if code == 5 {
            PlatformError::PermissionDenied
        } else {
            PlatformError::OperationFailed(format!("win32Error:{code}"))
        }
    }

    fn com_error(context: &str, error: &windows::core::Error) -> PlatformError {
        let code = error.code().0 as u32;
        if code == E_ACCESSDENIED {
            PlatformError::PermissionDenied
        } else {
            PlatformError::OperationFailed(format!("{context}:{code:#x}"))
        }
    }

    /// Runs `f` with a connected Task Scheduler service on an MTA-initialized thread.
    fn with_service<T>(f: impl FnOnce(&ITaskService) -> PlatformResult<T>) -> PlatformResult<T> {
        let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        let result = (|| unsafe {
            let service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| com_error("taskSchedulerCreate", &e))?;
            let empty = VARIANT::default();
            service
                .Connect(&empty, &empty, &empty, &empty)
                .map_err(|e| com_error("taskSchedulerConnect", &e))?;
            f(&service)
        })();
        if initialized {
            unsafe { CoUninitialize() };
        }
        result
    }

    fn split_task_path(path: &str) -> (String, String) {
        match path.rfind('\\') {
            Some(0) | None => ("\\".to_string(), path.trim_start_matches('\\').to_string()),
            Some(i) => (path[..i].to_string(), path[i + 1..].to_string()),
        }
    }

    fn open_task(service: &ITaskService, path: &str) -> PlatformResult<IRegisteredTask> {
        let (folder, name) = split_task_path(path);
        unsafe {
            let folder = service
                .GetFolder(&BSTR::from(folder.as_str()))
                .map_err(|e| com_error("taskFolder", &e))?;
            folder.GetTask(&BSTR::from(name.as_str())).map_err(|e| {
                if NOT_FOUND.contains(&(e.code().0 as u32)) {
                    PlatformError::OperationFailed("taskNotFound".into())
                } else {
                    com_error("taskOpen", &e)
                }
            })
        }
    }

    pub fn list_tasks() -> Vec<(String, bool, String)> {
        // TASK_ENUM_HIDDEN: include tasks marked hidden (updaters often are).
        const TASK_ENUM_HIDDEN: i32 = 1;
        fn walk(folder: &ITaskFolder, out: &mut Vec<(String, bool, String)>, depth: u32) {
            unsafe {
                if let Ok(tasks) = folder.GetTasks(TASK_ENUM_HIDDEN) {
                    let count = tasks.Count().unwrap_or(0);
                    for i in 1..=count {
                        let Ok(task) = tasks.get_Item(&VARIANT::from(i)) else { continue };
                        let path = task.Path().map(|p| p.to_string()).unwrap_or_default();
                        let enabled = task.Enabled().map(|b| b.0 != 0).unwrap_or(false);
                        let xml = task.Xml().map(|x| x.to_string()).unwrap_or_default();
                        if path.starts_with('\\') {
                            out.push((path, enabled, xml));
                        }
                    }
                }
                if depth > 8 {
                    return;
                }
                if let Ok(children) = folder.GetFolders(0) {
                    let count = children.Count().unwrap_or(0);
                    for i in 1..=count {
                        if let Ok(child) = children.get_Item(&VARIANT::from(i)) {
                            walk(&child, out, depth + 1);
                        }
                    }
                }
            }
        }
        with_service(|service| unsafe {
            let root = service.GetFolder(&BSTR::from("\\")).map_err(|e| com_error("taskFolder", &e))?;
            let mut out = Vec::new();
            walk(&root, &mut out, 0);
            Ok(out)
        })
        .unwrap_or_default()
    }

    pub fn set_task_enabled(path: &str, enabled: bool) -> PlatformResult<()> {
        with_service(|service| {
            let task = open_task(service, path)?;
            let value: VARIANT_BOOL = if enabled { VARIANT_TRUE } else { VARIANT_FALSE };
            unsafe { task.SetEnabled(value) }.map_err(|e| com_error("taskSetEnabled", &e))
        })
    }

    pub fn task_xml(path: &str) -> Option<String> {
        with_service(|service| {
            let task = open_task(service, path)?;
            unsafe { task.Xml() }.map(|x| x.to_string()).map_err(|e| com_error("taskXml", &e))
        })
        .ok()
    }

    pub fn delete_task(path: &str) -> PlatformResult<()> {
        let (folder, name) = split_task_path(path);
        with_service(|service| unsafe {
            let folder = service
                .GetFolder(&BSTR::from(folder.as_str()))
                .map_err(|e| com_error("taskFolder", &e))?;
            match folder.DeleteTask(&BSTR::from(name.as_str()), 0) {
                Ok(()) => Ok(()),
                Err(e) if NOT_FOUND.contains(&(e.code().0 as u32)) => Ok(()),
                Err(e) => Err(com_error("taskDelete", &e)),
            }
        })
    }

    pub fn register_task(folder_name: &str, name: &str, xml: &str) -> PlatformResult<String> {
        with_service(|service| unsafe {
            let root = service.GetFolder(&BSTR::from("\\")).map_err(|e| com_error("taskFolder", &e))?;
            let folder_path = format!("\\{folder_name}");
            let folder = match service.GetFolder(&BSTR::from(folder_path.as_str())) {
                Ok(folder) => Some(folder),
                Err(_) => root.CreateFolder(&BSTR::from(folder_name), &VARIANT::default()).ok(),
            };
            let empty = VARIANT::default();
            let register = |target: &ITaskFolder, task_name: &str| {
                target.RegisterTask(
                    &BSTR::from(task_name),
                    &BSTR::from(xml),
                    TASK_CREATE_OR_UPDATE.0,
                    &empty,
                    &empty,
                    TASK_LOGON_INTERACTIVE_TOKEN,
                    &empty,
                )
            };
            if let Some(folder) = folder {
                if let Ok(task) = register(&folder, name) {
                    return Ok(task.Path().map(|p| p.to_string()).unwrap_or_else(|_| format!("{folder_path}\\{name}")));
                }
            }
            // Standard users usually may not create folders in the root task folder; fall back
            // to a prefixed task name in the root, which every user may write to.
            let flat = format!("{folder_name} - {name}");
            let task = register(&root, &flat).map_err(|e| com_error("taskRegister", &e))?;
            Ok(task.Path().map(|p| p.to_string()).unwrap_or_else(|_| format!("\\{flat}")))
        })
    }

    pub fn set_service_start(name: &str, start: ServiceStartKind) -> PlatformResult<()> {
        use windows::Win32::System::Services::{
            ChangeServiceConfig2W, ChangeServiceConfigW, CloseServiceHandle, OpenSCManagerW, OpenServiceW,
            ENUM_SERVICE_TYPE, SC_MANAGER_CONNECT, SERVICE_AUTO_START, SERVICE_CHANGE_CONFIG,
            SERVICE_CONFIG_DELAYED_AUTO_START_INFO, SERVICE_DELAYED_AUTO_START_INFO, SERVICE_DEMAND_START,
            SERVICE_ERROR, SERVICE_NO_CHANGE, SERVICE_QUERY_CONFIG,
        };
        let (start_type, delayed) = match start {
            ServiceStartKind::Automatic => (SERVICE_AUTO_START, Some(false)),
            ServiceStartKind::AutomaticDelayed => (SERVICE_AUTO_START, Some(true)),
            ServiceStartKind::Manual => (SERVICE_DEMAND_START, None),
            _ => return Err(PlatformError::OperationFailed("serviceStartUnsupported".into())),
        };
        let manager = unsafe { OpenSCManagerW(None, None, SC_MANAGER_CONNECT) }
            .map_err(|e| com_error("openScManager", &e))?;
        let result = (|| unsafe {
            let service = OpenServiceW(manager, &HSTRING::from(name), SERVICE_CHANGE_CONFIG | SERVICE_QUERY_CONFIG)
                .map_err(|e| com_error("openService", &e))?;
            let changed = ChangeServiceConfigW(
                service,
                ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
                start_type,
                SERVICE_ERROR(SERVICE_NO_CHANGE),
                PCWSTR::null(),
                PCWSTR::null(),
                None,
                PCWSTR::null(),
                PCWSTR::null(),
                PCWSTR::null(),
                PCWSTR::null(),
            )
            .map_err(|e| com_error("changeServiceConfig", &e));
            let delayed_result = match (changed.is_ok(), delayed) {
                (true, Some(flag)) => {
                    let info = SERVICE_DELAYED_AUTO_START_INFO { fDelayedAutostart: flag.into() };
                    ChangeServiceConfig2W(
                        service,
                        SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
                        Some(&info as *const _ as *const core::ffi::c_void),
                    )
                    .map_err(|e| com_error("changeServiceDelayed", &e))
                }
                _ => Ok(()),
            };
            let _ = CloseServiceHandle(service);
            changed.and(delayed_result)
        })();
        unsafe {
            let _ = CloseServiceHandle(manager);
        }
        result
    }

    pub fn file_version_facts(path: &Path) -> FileVersionFacts {
        use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
        let file = HSTRING::from(path.as_os_str());
        let size = unsafe { GetFileVersionInfoSizeW(&file, None) };
        if size == 0 || size > 4 * 1024 * 1024 {
            return FileVersionFacts::default();
        }
        let mut data = vec![0u8; size as usize];
        if unsafe { GetFileVersionInfoW(&file, None, size, data.as_mut_ptr().cast()) }.is_err() {
            return FileVersionFacts::default();
        }
        let query = |sub: &str| -> Option<(*mut core::ffi::c_void, u32)> {
            let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut len = 0u32;
            let ok = unsafe { VerQueryValueW(data.as_ptr().cast(), &HSTRING::from(sub), &mut ptr, &mut len) };
            (ok.as_bool() && !ptr.is_null() && len > 0).then_some((ptr, len))
        };
        let mut translations = Vec::new();
        if let Some((ptr, len)) = query("\\VarFileInfo\\Translation") {
            let pairs = (len as usize) / 4;
            let slice = unsafe { std::slice::from_raw_parts(ptr as *const u16, pairs * 2) };
            for pair in slice.chunks_exact(2) {
                translations.push(format!("{:04x}{:04x}", pair[0], pair[1]));
            }
        }
        // Common fallbacks: US English with Unicode / Western code pages.
        translations.extend(["040904b0".to_string(), "040904e4".to_string(), "000004b0".to_string()]);
        let read = |field: &str| -> Option<String> {
            translations.iter().find_map(|lang| {
                let (ptr, len) = query(&format!("\\StringFileInfo\\{lang}\\{field}"))?;
                let slice = unsafe { std::slice::from_raw_parts(ptr as *const u16, len as usize) };
                let end = slice.iter().position(|c| *c == 0).unwrap_or(slice.len());
                let text = String::from_utf16_lossy(&slice[..end]).trim().to_string();
                (!text.is_empty()).then_some(text)
            })
        };
        FileVersionFacts { company: read("CompanyName"), description: read("FileDescription") }
    }

    pub fn read_boot_performance(max_events: usize) -> (BootPerfAccess, Vec<BootPerfEvent>) {
        use windows::Win32::System::EventLog::{
            EvtClose, EvtNext, EvtQuery, EvtQueryChannelPath, EvtQueryReverseDirection, EvtRender,
            EvtRenderEventXml, EVT_HANDLE,
        };
        const CHANNEL: &str = "Microsoft-Windows-Diagnostics-Performance/Operational";
        const QUERY: &str = "*[System[(EventID=100 or EventID=101 or EventID=102 or EventID=103)]]";
        let handle = match unsafe {
            EvtQuery(
                None,
                &HSTRING::from(CHANNEL),
                &HSTRING::from(QUERY),
                EvtQueryChannelPath.0 | EvtQueryReverseDirection.0,
            )
        } {
            Ok(handle) => handle,
            Err(e) => {
                let access = if e.code().0 as u32 == E_ACCESSDENIED {
                    BootPerfAccess::NeedsAdmin
                } else {
                    BootPerfAccess::Unavailable
                };
                return (access, Vec::new());
            }
        };
        let mut out = Vec::new();
        let mut read = 0usize;
        'outer: while read < max_events {
            let mut batch = [0isize; 32];
            let mut returned = 0u32;
            if unsafe { EvtNext(handle, &mut batch, 2_000, 0, &mut returned) }.is_err() {
                break;
            }
            let count = returned as usize;
            for index in 0..count {
                let event = EVT_HANDLE(batch[index]);
                read += 1;
                let mut used = 0u32;
                let mut props = 0u32;
                let _ = unsafe { EvtRender(None, event, EvtRenderEventXml.0, 0, None, &mut used, &mut props) };
                if used > 0 && used < 1024 * 1024 {
                    let mut buf = vec![0u16; (used as usize).div_ceil(2)];
                    if unsafe {
                        EvtRender(
                            None,
                            event,
                            EvtRenderEventXml.0,
                            (buf.len() * 2) as u32,
                            Some(buf.as_mut_ptr().cast()),
                            &mut used,
                            &mut props,
                        )
                    }
                    .is_ok()
                    {
                        let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
                        if let Some(parsed) = parse_boot_perf_event(&String::from_utf16_lossy(&buf[..end])) {
                            out.push(parsed);
                        }
                    }
                }
                unsafe {
                    let _ = EvtClose(event);
                }
                if read >= max_events {
                    // Close the remaining handles of this batch before stopping.
                    for rest in &batch[index + 1..count] {
                        unsafe {
                            let _ = EvtClose(EVT_HANDLE(*rest));
                        }
                    }
                    break 'outer;
                }
            }
        }
        unsafe {
            let _ = EvtClose(handle);
        }
        (BootPerfAccess::Available, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(name: &str) -> Option<String> {
        match name {
            "SystemRoot" => Some("C:\\Windows".into()),
            "ProgramFiles" => Some("C:\\Program Files".into()),
            "LOCALAPPDATA" => Some("C:\\Users\\u\\AppData\\Local".into()),
            _ => None,
        }
    }

    fn exists(path: &str) -> bool {
        [
            "C:\\Program Files\\Vendor App\\app.exe",
            "C:\\Windows\\System32\\rundll32.exe",
            "C:\\Windows\\System32\\ctfmon.exe",
        ]
        .iter()
        .any(|p| p.eq_ignore_ascii_case(path))
    }

    #[test]
    fn approved_bytes_round_trip_like_task_manager() {
        let enabled = approved_bytes(true, 1_700_000_000_000);
        assert_eq!(enabled, [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(parse_approved_bytes(&enabled), Some(ApprovedState { enabled: true, changed_at_ms: None }));

        let disabled = approved_bytes(false, 1_700_000_000_000);
        assert_eq!(&disabled[..4], &[3, 0, 0, 0]);
        let state = parse_approved_bytes(&disabled).unwrap();
        assert!(!state.enabled);
        assert_eq!(state.changed_at_ms, Some(1_700_000_000_000));
    }

    #[test]
    fn approved_first_byte_parity_decides_state() {
        assert!(parse_approved_bytes(&[0x06, 0, 0, 0]).unwrap().enabled);
        assert!(!parse_approved_bytes(&[0x03]).unwrap().enabled);
        assert!(!parse_approved_bytes(&[0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap().enabled);
        assert_eq!(parse_approved_bytes(&[]), None);
    }

    #[test]
    fn command_target_handles_quotes_spaces_and_env() {
        assert_eq!(
            command_target("\"C:\\Program Files\\Vendor App\\app.exe\" --tray", &env, &exists),
            Some(PathBuf::from("C:\\Program Files\\Vendor App\\app.exe"))
        );
        assert_eq!(
            command_target("C:\\Program Files\\Vendor App\\app.exe /background", &env, &exists),
            Some(PathBuf::from("C:\\Program Files\\Vendor App\\app.exe"))
        );
        assert_eq!(
            command_target("\"%ProgramFiles%\\Vendor App\\app.exe\"", &env, &exists),
            Some(PathBuf::from("C:\\Program Files\\Vendor App\\app.exe"))
        );
        // Missing targets still resolve to the named path so callers can flag them as broken.
        assert_eq!(
            command_target("\"C:\\Gone\\old.exe\" -m", &env, &exists),
            Some(PathBuf::from("C:\\Gone\\old.exe"))
        );
    }

    #[test]
    fn command_target_resolves_rundll32_dll_and_bare_programs() {
        assert_eq!(
            command_target("rundll32.exe \"C:\\Vendor\\helper.dll\",Start", &env, &exists),
            Some(PathBuf::from("C:\\Vendor\\helper.dll"))
        );
        assert_eq!(
            command_target("ctfmon", &env, &exists),
            Some(PathBuf::from("C:\\Windows\\System32\\ctfmon.exe"))
        );
        // Unknown bare programs are not guessed (would otherwise look "missing").
        assert_eq!(command_target("someapp.exe /x", &env, &exists), None);
        // Unresolved variables are refused for the same reason.
        assert_eq!(command_target("%UNKNOWN%\\x.exe", &env, &exists), None);
    }

    #[test]
    fn indirect_labels_fall_back_to_text_after_semicolon() {
        assert_eq!(resolve_indirect_string("Plain name"), "Plain name");
        assert_eq!(
            resolve_indirect_string("@oem99.inf,%NoSuchKey%;Intel(R) TPM Provisioning Service"),
            "Intel(R) TPM Provisioning Service"
        );
    }

    #[test]
    fn command_target_understands_service_image_paths() {
        assert_eq!(
            command_target("\\SystemRoot\\System32\\ctfmon.exe -k", &env, &exists),
            Some(PathBuf::from("C:\\Windows\\System32\\ctfmon.exe"))
        );
        assert_eq!(
            command_target("\\??\\C:\\Program Files\\Vendor App\\app.exe", &env, &exists),
            Some(PathBuf::from("C:\\Program Files\\Vendor App\\app.exe"))
        );
    }

    #[test]
    fn parses_task_triggers_actions_and_principal() {
        let xml = r#"<Task><Triggers><LogonTrigger id="a"><Enabled>true</Enabled></LogonTrigger>
<BootTrigger><Enabled>false</Enabled></BootTrigger></Triggers>
<Principals><Principal id="Author"><UserId>S-1-5-18</UserId><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
<Actions><Exec><Command>"C:\Vendor\upd.exe"</Command><Arguments>/c &amp; x</Arguments></Exec></Actions></Task>"#;
        let facts = parse_task_definition(xml);
        assert!(facts.logon_trigger);
        assert!(!facts.boot_trigger);
        assert!(facts.privileged);
        assert_eq!(facts.exec, vec![("\"C:\\Vendor\\upd.exe\"".to_string(), "/c & x".to_string())]);

        let user = r#"<Task><Triggers><LogonTriggerX/><TimeTrigger/></Triggers><Principals><Principal><UserId>PC\me</UserId></Principal></Principals></Task>"#;
        let facts = parse_task_definition(user);
        assert!(!facts.logon_trigger);
        assert!(!facts.privileged);
    }

    #[test]
    fn parses_boot_performance_events() {
        let boot = r#"<Event><System><EventID>100</EventID><TimeCreated SystemTime='2026-11-20T08:00:00.0000000Z'/></System>
<EventData><Data Name='BootTime'>48123</Data><Data Name='MainPathBootTime'>20000</Data></EventData></Event>"#;
        assert_eq!(
            parse_boot_perf_event(boot),
            Some(BootPerfEvent::Boot {
                system_time: "2026-11-20T08:00:00.0000000Z".into(),
                boot_ms: 48123,
                main_path_ms: Some(20000)
            })
        );
        let app = r#"<Event><System><EventID>101</EventID><TimeCreated SystemTime="2026-11-20T08:00:00Z"/></System>
<EventData><Data Name="Name">OneDrive.exe</Data><Data Name="FriendlyName">Microsoft OneDrive</Data>
<Data Name="TotalTime">3200</Data><Data Name="DegradationTime">1500</Data></EventData></Event>"#;
        match parse_boot_perf_event(app) {
            Some(BootPerfEvent::Degradation { file_name, degradation_ms, event_id, .. }) => {
                assert_eq!(file_name, "OneDrive.exe");
                assert_eq!(degradation_ms, 1500);
                assert_eq!(event_id, 101);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(parse_boot_perf_event("<Event><System><EventID>200</EventID></System></Event>"), None);
    }

    #[test]
    fn delayed_task_xml_escapes_command() {
        let xml = delayed_logon_task_xml("PC\\me", "C:\\A & B\\app.exe", "--x \"y\"", 60, "Delayed start");
        assert!(xml.contains("<Command>C:\\A &amp; B\\app.exe</Command>"));
        assert!(xml.contains("<Arguments>--x &quot;y&quot;</Arguments>"));
        assert!(xml.contains("<Delay>PT60S</Delay>"));
        assert!(xml.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        let facts = parse_task_definition(&xml);
        assert!(facts.logon_trigger);
        assert!(!facts.privileged);
    }

    /// Manual read-only smoke check (`cargo test -p pctoolkit-platform startup_inventory_smoke
    /// -- --ignored --nocapture`). Prints counts only — never names, commands, or paths.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn startup_inventory_smoke() {
        let started = std::time::Instant::now();
        let mut all = list_run_entries();
        all.extend(list_startup_folder_entries());
        all.extend(list_task_entries());
        let services = list_service_entries();
        let auto_services = services.iter().filter(|s| s.enabled).count();
        for source in [StartupSource::Run, StartupSource::RunOnce, StartupSource::StartupFolder, StartupSource::ScheduledTask] {
            let of: Vec<_> = all.iter().filter(|e| e.source == source).collect();
            let enabled = of.iter().filter(|e| e.enabled).count();
            let microsoft = of.iter().filter(|e| e.microsoft_task).count();
            let unresolved = of.iter().filter(|e| e.target.is_none()).count();
            println!("{source:?}: total={} enabled={enabled} microsoftTasks={microsoft} targetUnresolved={unresolved}", of.len());
        }
        println!("Service (Win32): total={} autoStart={auto_services}", services.len());
        let (access, events) = read_boot_performance(400);
        let boots: Vec<u64> = events
            .iter()
            .filter_map(|e| match e {
                BootPerfEvent::Boot { boot_ms, .. } => Some(*boot_ms),
                _ => None,
            })
            .collect();
        println!(
            "boot log: access={access:?} events={} boots={} lastBootMs={:?} elapsedMs={}",
            events.len(),
            boots.len(),
            boots.first(),
            started.elapsed().as_millis()
        );
    }

    /// Writes only under a dedicated test key and removes it afterwards.
    #[cfg(windows)]
    #[test]
    fn approved_value_write_read_under_test_key() {
        let key = RegistryKeyPath::new(
            RegistryRoot::CurrentUser,
            RegistryView::Native64,
            "Software\\PC Toolkit Pro Test\\StartupApproved\\Run",
        );
        let bytes = approved_bytes(false, 1_700_000_000_000);
        write_binary_value(&key, "Sample", &bytes).expect("write test value");
        let read = read_binary_value(&key, "Sample").expect("read test value");
        assert_eq!(read, bytes.to_vec());
        let cleanup = RegistryKeyPath::new(RegistryRoot::CurrentUser, RegistryView::Native64, "Software\\PC Toolkit Pro Test");
        registry::delete_tree(&cleanup).expect("remove test key");
        assert!(!registry::key_exists(&cleanup));
    }
}
