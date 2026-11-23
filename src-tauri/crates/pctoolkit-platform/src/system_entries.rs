//! Windows services, scheduled tasks, and System Restore points referenced by leftover cleanup.
//! Each mutation reports `PermissionDenied` honestly instead of silently elevating.

use crate::registry::{self, RegistryKeyPath, RegistryRoot, RegistryView};
use crate::{PlatformError, PlatformResult};

const SERVICES_SUBKEY: &str = "SYSTEM\\CurrentControlSet\\Services";

#[derive(Debug, Clone)]
pub struct ServiceFact {
    pub name: String,
    pub display_name: String,
    pub image_path: String,
}

/// Win32 services (not kernel drivers) with their image paths, read from the registry.
pub fn list_services() -> Vec<ServiceFact> {
    let root = RegistryKeyPath::new(RegistryRoot::LocalMachine, RegistryView::Native64, SERVICES_SUBKEY);
    let Ok(names) = registry::subkey_names(&root) else {
        return Vec::new();
    };
    names
        .into_iter()
        .filter_map(|name| {
            let key = root.child(&name);
            // Type 0x10 / 0x20 = own / shared process service; drivers are excluded on purpose.
            let kind: u32 = registry::read_value(&key, "Type")?.parse().ok()?;
            if kind & 0x30 == 0 {
                return None;
            }
            let image_path = registry::read_value(&key, "ImagePath")?;
            Some(ServiceFact {
                display_name: registry::read_value(&key, "DisplayName").unwrap_or_else(|| name.clone()),
                name,
                image_path,
            })
        })
        .collect()
}

/// Stop (best effort) and delete a Win32 service. Requires administrator rights.
pub fn delete_service(name: &str) -> PlatformResult<()> {
    #[cfg(windows)]
    {
        use windows::core::HSTRING;
        use windows::Win32::System::Services::{
            CloseServiceHandle, ControlService, DeleteService, OpenSCManagerW, OpenServiceW,
            SC_MANAGER_CONNECT, SERVICE_CONTROL_STOP, SERVICE_STATUS, SERVICE_STOP,
        };
        const DELETE: u32 = 0x0001_0000;
        let manager = unsafe { OpenSCManagerW(None, None, SC_MANAGER_CONNECT) }
            .map_err(|_| PlatformError::PermissionDenied)?;
        let result = (|| {
            let service = unsafe {
                OpenServiceW(manager, &HSTRING::from(name), SERVICE_STOP | DELETE)
            }
            .map_err(|e| {
                if e.code().0 as u32 == 0x8007_0005 {
                    PlatformError::PermissionDenied
                } else {
                    PlatformError::OperationFailed(format!("openService:{:?}", e.code()))
                }
            })?;
            let mut status = SERVICE_STATUS::default();
            let _ = unsafe { ControlService(service, SERVICE_CONTROL_STOP, &mut status) };
            let deleted = unsafe { DeleteService(service) };
            unsafe {
                let _ = CloseServiceHandle(service);
            }
            deleted.map_err(|e| PlatformError::OperationFailed(format!("deleteService:{:?}", e.code())))
        })();
        unsafe {
            let _ = CloseServiceHandle(manager);
        }
        result
    }
    #[cfg(not(windows))]
    {
        let _ = name;
        Err(PlatformError::Unsupported)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledTaskFact {
    /// Task path such as `\Vendor\UpdateTask`.
    pub name: String,
    /// Executables / arguments from the task's `<Exec>` actions joined for matching.
    pub commands: String,
}

/// Parse `schtasks /query /xml` output (several `<Task>` documents preceded by `<!-- \Name -->`).
pub fn parse_scheduled_tasks_xml(xml: &str) -> Vec<ScheduledTaskFact> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<!--") {
        let after = &rest[start + 4..];
        let Some(end) = after.find("-->") else { break };
        let name = after[..end].trim().to_string();
        let body_start = &after[end + 3..];
        let body_end = body_start.find("<!--").unwrap_or(body_start.len());
        if name.starts_with('\\') {
            out.push(ScheduledTaskFact {
                name,
                commands: exec_commands_from_xml(&body_start[..body_end]),
            });
        }
        rest = body_start;
    }
    out
}

/// Executables, arguments, and working directories of a task definition's `<Exec>` actions.
fn exec_commands_from_xml(body: &str) -> String {
    let mut commands = Vec::new();
    for tag in ["Command", "Arguments", "WorkingDirectory"] {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let mut cursor = body;
        while let Some(i) = cursor.find(&open) {
            let value_start = &cursor[i + open.len()..];
            let Some(j) = value_start.find(&close) else { break };
            commands.push(xml_unescape(&value_start[..j]));
            cursor = &value_start[j..];
        }
    }
    commands.join(" ")
}

fn xml_unescape(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Scheduled tasks visible to the current user. Uses the Task Scheduler COM API (fast, no
/// console); falls back to `schtasks /query /xml` when COM is unavailable. Tasks the user cannot
/// read are simply absent.
pub fn list_scheduled_tasks() -> Vec<ScheduledTaskFact> {
    #[cfg(windows)]
    {
        if let Some(tasks) = list_tasks_com() {
            return tasks;
        }
        use std::process::Command;
        let mut cmd = Command::new(crate::registry::system32_exe("schtasks.exe"));
        cmd.args(["/query", "/xml", "ONE"]);
        crate::hide_console(&mut cmd);
        let Ok(output) = cmd.output() else {
            return Vec::new();
        };
        parse_scheduled_tasks_xml(&decode_console_output(&output.stdout))
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[cfg(windows)]
fn list_tasks_com() -> Option<Vec<ScheduledTaskFact>> {
    use windows::core::BSTR;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::System::TaskScheduler::{ITaskFolder, ITaskService, TaskScheduler};
    use windows::Win32::System::Variant::VARIANT;

    // TASK_ENUM_HIDDEN: include tasks marked hidden (updaters often are).
    const TASK_ENUM_HIDDEN: i32 = 1;

    fn walk(folder: &ITaskFolder, out: &mut Vec<ScheduledTaskFact>, depth: u32) {
        unsafe {
            if let Ok(tasks) = folder.GetTasks(TASK_ENUM_HIDDEN) {
                let count = tasks.Count().unwrap_or(0);
                for i in 1..=count {
                    let Ok(task) = tasks.get_Item(&VARIANT::from(i)) else { continue };
                    let name = task.Path().map(|p| p.to_string()).unwrap_or_default();
                    let xml = task.Xml().map(|x| x.to_string()).unwrap_or_default();
                    if name.starts_with('\\') {
                        out.push(ScheduledTaskFact {
                            name,
                            commands: exec_commands_from_xml(&xml),
                        });
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

    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let result = (|| unsafe {
        let service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).ok()?;
        let empty = VARIANT::default();
        service.Connect(&empty, &empty, &empty, &empty).ok()?;
        let root = service.GetFolder(&BSTR::from("\\")).ok()?;
        let mut out = Vec::new();
        walk(&root, &mut out, 0);
        Some(out)
    })();
    if initialized {
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(windows)]
fn decode_console_output(bytes: &[u8]) -> String {
    // schtasks writes UTF-16 LE when redirected on some builds and the OEM code page on others.
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let wide: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&wide);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// Delete a scheduled task. Tasks created by elevated installers need administrator rights.
pub fn delete_scheduled_task(name: &str) -> PlatformResult<()> {
    #[cfg(windows)]
    {
        use std::process::Command;
        let mut cmd = Command::new(crate::registry::system32_exe("schtasks.exe"));
        cmd.args(["/delete", "/tn", name, "/f"]);
        crate::hide_console(&mut cmd);
        let output = cmd
            .output()
            .map_err(|e| PlatformError::OperationFailed(format!("schtasksSpawn:{}", e.kind())))?;
        if output.status.success() {
            Ok(())
        } else if crate::is_user_admin() {
            Err(PlatformError::OperationFailed(format!(
                "schtasksExit:{}",
                output.status.code().unwrap_or(-1)
            )))
        } else {
            Err(PlatformError::PermissionDenied)
        }
    }
    #[cfg(not(windows))]
    {
        let _ = name;
        Err(PlatformError::Unsupported)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestorePointOutcome {
    Created,
    /// Windows throttles restore points (default: one per 24 h) or System Restore is off.
    NotCreated,
}

/// Create a System Restore point before an uninstall. Requires administrator rights.
pub fn create_restore_point(description: &str) -> PlatformResult<RestorePointOutcome> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Restore::{
            SRSetRestorePointW, APPLICATION_UNINSTALL, BEGIN_SYSTEM_CHANGE, RESTOREPOINTINFOW,
            STATEMGRSTATUS,
        };
        if !crate::is_user_admin() {
            return Err(PlatformError::PermissionDenied);
        }
        // RESTOREPOINTINFOW is packed: build the description array first, then move it in.
        let mut text = [0u16; 256];
        for (slot, ch) in text.iter_mut().zip(description.encode_utf16().take(63)) {
            *slot = ch;
        }
        let info = RESTOREPOINTINFOW {
            dwEventType: BEGIN_SYSTEM_CHANGE,
            dwRestorePtType: APPLICATION_UNINSTALL,
            llSequenceNumber: 0,
            szDescription: text,
        };
        let mut status = STATEMGRSTATUS::default();
        let ok = unsafe { SRSetRestorePointW(&info, &mut status) }.as_bool();
        if !ok {
            return Ok(RestorePointOutcome::NotCreated);
        }
        // Close the change window immediately; the uninstall itself is the "change".
        let end = RESTOREPOINTINFOW {
            dwEventType: windows::Win32::System::Restore::END_SYSTEM_CHANGE,
            dwRestorePtType: windows::Win32::System::Restore::RESTOREPOINTINFO_TYPE(0),
            llSequenceNumber: status.llSequenceNumber,
            ..Default::default()
        };
        let mut end_status = STATEMGRSTATUS::default();
        let _ = unsafe { SRSetRestorePointW(&end, &mut end_status) };
        Ok(RestorePointOutcome::Created)
    }
    #[cfg(not(windows))]
    {
        let _ = description;
        Err(PlatformError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_schtasks_xml_blocks() {
        let xml = r#"<?xml version="1.0" encoding="UTF-16"?>
<!-- \Vendor\Updater -->
<Task><Actions><Exec><Command>"C:\Program Files\Vendor\upd.exe"</Command><Arguments>/check &amp; run</Arguments></Exec></Actions></Task>
<!-- \Other -->
<Task><Actions><Exec><Command>C:\Tools\x.exe</Command></Exec></Actions></Task>
"#;
        let tasks = parse_scheduled_tasks_xml(xml);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].name, "\\Vendor\\Updater");
        assert!(tasks[0].commands.contains("C:\\Program Files\\Vendor\\upd.exe"));
        assert!(tasks[0].commands.contains("/check & run"));
        assert_eq!(tasks[1].name, "\\Other");
    }
}
