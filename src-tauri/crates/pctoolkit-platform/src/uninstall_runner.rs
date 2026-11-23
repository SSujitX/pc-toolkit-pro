//! Launch a program's own uninstaller and wait until it (and the helper processes it spawns)
//! has finished.
//!
//! Many uninstallers copy themselves to `%TEMP%` and exit immediately (Inno Setup `_iu*.tmp`,
//! NSIS `Un_A.exe`), so waiting on the first process alone would start the leftover scan while
//! the real uninstall is still running. The launched process is placed in a Job Object and we
//! wait until the job has no active processes. When the uninstaller elevates through UAC the job
//! assignment can be refused; we then fall back to waiting on the process handle plus a short
//! watch for well-known self-copy helper images.

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Process ids of the uninstaller tree currently being waited on (for window focus/detection).
static ACTIVE_PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

#[cfg_attr(not(windows), allow(dead_code))]
fn set_active_pids(pids: Vec<u32>) {
    *ACTIVE_PIDS.lock().unwrap_or_else(|e| e.into_inner()) = pids;
}

#[cfg_attr(not(windows), allow(dead_code))]
fn active_pids() -> Vec<u32> {
    ACTIVE_PIDS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Progress facts published while waiting on an uninstaller.
#[derive(Debug, Clone, Copy, Default)]
pub struct UninstallerTick {
    pub elapsed: Duration,
    /// A visible top-level window belongs to the uninstaller tree (it may be waiting for input).
    pub window_visible: bool,
    pub process_count: u32,
}

/// Bring the running uninstaller's windows to the foreground. Returns false when none is visible.
pub fn focus_uninstaller_window() -> bool {
    #[cfg(windows)]
    {
        imp::focus_windows(&active_pids())
    }
    #[cfg(not(windows))]
    {
        false
    }
}

use crate::{PlatformError, PlatformResult};

/// Split a registry `UninstallString` into executable and arguments.
///
/// Handles quoted paths, unquoted paths containing spaces (`C:\Program Files\App\uninst.exe /S`),
/// and bare commands (`MsiExec.exe /X{GUID}`). `exists` is injected so the logic stays testable.
pub fn split_command_line(command: &str, exists: impl Fn(&str) -> bool) -> Option<(String, String)> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix('"') {
        let end = rest.find('"')?;
        let exe = rest[..end].trim().to_string();
        let args = rest[end + 1..].trim().to_string();
        return (!exe.is_empty()).then_some((exe, args));
    }
    // Unquoted: prefer the longest prefix ending in ".exe" that exists on disk.
    let lower = trimmed.to_ascii_lowercase();
    let mut search_from = 0;
    let mut best: Option<usize> = None;
    while let Some(pos) = lower[search_from..].find(".exe") {
        let end = search_from + pos + 4;
        let at_boundary = trimmed[end..].is_empty() || trimmed[end..].starts_with(' ');
        if at_boundary && exists(&trimmed[..end]) {
            best = Some(end);
        }
        if best.is_none() && at_boundary && !trimmed[..end].contains('\\') {
            // Bare program names such as `MsiExec.exe` resolve through the search path.
            best = Some(end);
        }
        search_from = end;
    }
    let end = best.unwrap_or_else(|| trimmed.find(' ').unwrap_or(trimmed.len()));
    Some((
        trimmed[..end].trim().to_string(),
        trimmed[end..].trim().to_string(),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UninstallerRunOutcome {
    /// The uninstaller tree finished. `exit_code` belongs to the first process.
    Finished { exit_code: u32 },
    /// The caller cancelled waiting; the uninstaller keeps running on its own.
    StoppedWaiting,
}

/// Start `exe args` and block until the uninstaller finishes or `should_stop` returns true.
/// `on_tick` is called about twice per second so callers can publish elapsed time.
pub fn run_uninstaller(
    exe: &str,
    args: &str,
    should_stop: &dyn Fn() -> bool,
    on_tick: &mut dyn FnMut(UninstallerTick),
) -> PlatformResult<UninstallerRunOutcome> {
    #[cfg(windows)]
    {
        let result = imp::run(exe, args, should_stop, on_tick);
        set_active_pids(Vec::new());
        result
    }
    #[cfg(not(windows))]
    {
        let _ = (exe, args, should_stop, on_tick);
        Err(PlatformError::Unsupported)
    }
}

pub fn path_exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// Image names used by installer frameworks for the temporary self-copy that performs the work.
fn is_uninstall_helper_image(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    (lower.starts_with("_iu") && lower.ends_with(".tmp"))
        || lower.starts_with("un_a")
        || lower.starts_with("au_")
        || (lower.starts_with("unins") && lower.ends_with(".exe"))
    // msiexec.exe is deliberately absent: the Windows Installer service keeps an msiexec process
    // alive, and the msiexec client we launch already waits for the uninstall transaction.
}

/// Grow `tree` (process ids descended from the launched uninstaller) with every process whose
/// parent is already in it, transitively. `processes` are `(pid, parent pid)` pairs. Windows keeps
/// a child's parent id after the parent exits, so a self-copied helper (`_iu*.tmp`) started by an
/// uninstaller that already quit is still attributed to it.
#[cfg_attr(not(windows), allow(dead_code))]
fn grow_process_tree(tree: &mut std::collections::HashSet<u32>, processes: &[(u32, Option<u32>)]) {
    loop {
        let before = tree.len();
        for (pid, parent) in processes {
            if parent.is_some_and(|p| tree.contains(&p)) {
                tree.insert(*pid);
            }
        }
        if tree.len() == before {
            break;
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
        QueryInformationJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    };
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::System::JobObjects::{
        JobObjectBasicProcessIdList, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    };
    use windows::Win32::System::Threading::{GetExitCodeProcess, GetProcessId, WaitForSingleObject};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow,
        ShowWindow, SW_RESTORE,
    };
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    struct OwnedHandle(HANDLE);
    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                unsafe {
                    let _ = CloseHandle(self.0);
                }
            }
        }
    }

    pub fn run(
        exe: &str,
        args: &str,
        should_stop: &dyn Fn() -> bool,
        on_tick: &mut dyn FnMut(UninstallerTick),
    ) -> PlatformResult<UninstallerRunOutcome> {
        let file = HSTRING::from(exe);
        let params = HSTRING::from(args);
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: if args.is_empty() {
                PCWSTR::null()
            } else {
                PCWSTR(params.as_ptr())
            },
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        // ShellExecuteEx (not CreateProcess) so manifests that require elevation show the normal
        // UAC consent prompt instead of failing with ERROR_ELEVATION_REQUIRED.
        unsafe { ShellExecuteExW(&mut info) }.map_err(|e| {
            // ERROR_CANCELLED (1223) when the user declines UAC.
            if e.code().0 as u32 == 0x8007_04C7 {
                PlatformError::PermissionDenied
            } else {
                PlatformError::OperationFailed(format!("uninstallerLaunch:{:?}", e.code()))
            }
        })?;
        if info.hProcess.is_invalid() {
            // Launched through a shell handler that gave no process; nothing to wait on.
            return Ok(UninstallerRunOutcome::Finished { exit_code: 0 });
        }
        let process = OwnedHandle(info.hProcess);
        let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
            .ok()
            .map(OwnedHandle);
        let job_attached = job
            .as_ref()
            .is_some_and(|job| unsafe { AssignProcessToJobObject(job.0, process.0) }.is_ok());

        let started = Instant::now();
        let mut exit_code = 0u32;
        let mut process_done = false;
        let main_pid = unsafe { GetProcessId(process.0) };
        let mut helpers = HelperTracker::new(main_pid);
        loop {
            if should_stop() {
                return Ok(UninstallerRunOutcome::StoppedWaiting);
            }
            let pids = match job.as_ref().filter(|_| job_attached) {
                Some(job) => job_process_ids(job.0),
                None if !process_done => vec![main_pid],
                // One process-list refresh per tick serves both the window check and `tree_busy`.
                None => helpers.refresh(),
            };
            on_tick(UninstallerTick {
                elapsed: started.elapsed(),
                window_visible: !visible_windows(&pids).is_empty(),
                process_count: pids.len() as u32,
            });
            set_active_pids(pids.clone());
            if !process_done && unsafe { WaitForSingleObject(process.0, 500) } == WAIT_OBJECT_0 {
                process_done = true;
                let _ = unsafe { GetExitCodeProcess(process.0, &mut exit_code) };
            } else if process_done {
                std::thread::sleep(Duration::from_millis(500));
            }
            if !process_done {
                continue;
            }
            let tree_busy = if job_attached {
                job.as_ref().is_some_and(|job| active_processes(job.0) > 0)
            } else {
                !pids.is_empty()
            };
            if !tree_busy {
                return Ok(UninstallerRunOutcome::Finished { exit_code });
            }
        }
    }

    fn active_processes(job: HANDLE) -> u32 {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        let ok = unsafe {
            QueryInformationJobObject(
                Some(job),
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as *mut core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                None,
            )
        };
        if ok.is_ok() {
            info.ActiveProcesses
        } else {
            0
        }
    }

    fn job_process_ids(job: HANDLE) -> Vec<u32> {
        // Header + room for 64 ids; uninstaller trees are small.
        #[repr(C)]
        struct IdList {
            header: JOBOBJECT_BASIC_PROCESS_ID_LIST,
            rest: [usize; 63],
        }
        let mut list: IdList = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            QueryInformationJobObject(
                Some(job),
                JobObjectBasicProcessIdList,
                &mut list as *mut _ as *mut core::ffi::c_void,
                std::mem::size_of::<IdList>() as u32,
                None,
            )
        };
        if ok.is_err() {
            return Vec::new();
        }
        let count = (list.header.NumberOfProcessIdsInList as usize).min(64);
        let base = list.header.ProcessIdList.as_ptr();
        (0..count).map(|i| unsafe { *base.add(i) } as u32).collect()
    }

    /// Fallback when the Job Object could not be attached: follows the uninstaller's process
    /// tree by parent ids and reports live helper images (`_iu*.tmp`, `Un_A.exe`, ...) inside it,
    /// so an unrelated installer running elsewhere is not waited for.
    struct HelperTracker {
        sys: sysinfo::System,
        tree: std::collections::HashSet<u32>,
    }

    impl HelperTracker {
        fn new(main_pid: u32) -> Self {
            Self {
                sys: sysinfo::System::new(),
                tree: std::collections::HashSet::from([main_pid]),
            }
        }

        fn refresh(&mut self) -> Vec<u32> {
            use sysinfo::ProcessesToUpdate;
            self.sys.refresh_processes(ProcessesToUpdate::All, true);
            let pairs: Vec<(u32, Option<u32>)> = self
                .sys
                .processes()
                .iter()
                .map(|(pid, p)| (pid.as_u32(), p.parent().map(|pp| pp.as_u32())))
                .collect();
            grow_process_tree(&mut self.tree, &pairs);
            self.sys
                .processes()
                .iter()
                .filter(|(pid, p)| {
                    self.tree.contains(&pid.as_u32()) && is_uninstall_helper_image(&p.name().to_string_lossy())
                })
                .map(|(pid, _)| pid.as_u32())
                .collect()
        }
    }

    struct EnumState {
        pids: Vec<u32>,
        found: Vec<HWND>,
    }

    unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = unsafe { &mut *(lparam.0 as *mut EnumState) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if state.pids.contains(&pid) && unsafe { IsWindowVisible(hwnd) }.as_bool() {
            state.found.push(hwnd);
        }
        BOOL(1)
    }

    pub fn visible_windows(pids: &[u32]) -> Vec<HWND> {
        if pids.is_empty() {
            return Vec::new();
        }
        let mut state = EnumState {
            pids: pids.to_vec(),
            found: Vec::new(),
        };
        let _ = unsafe { EnumWindows(Some(collect_window), LPARAM(&mut state as *mut _ as isize)) };
        state.found
    }

    pub fn focus_windows(pids: &[u32]) -> bool {
        let windows = visible_windows(pids);
        for hwnd in &windows {
            unsafe {
                if IsIconic(*hwnd).as_bool() {
                    let _ = ShowWindow(*hwnd, SW_RESTORE);
                }
            }
        }
        windows
            .first()
            .is_some_and(|first| unsafe { SetForegroundWindow(*first) }.as_bool())
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_quoted_command() {
        let split = split_command_line("\"C:\\Program Files\\App\\unins000.exe\" /SILENT", |_| false);
        assert_eq!(
            split,
            Some(("C:\\Program Files\\App\\unins000.exe".into(), "/SILENT".into()))
        );
    }

    #[test]
    fn splits_unquoted_path_with_spaces_using_existence() {
        let exe = "C:\\Program Files\\My App\\uninst.exe";
        let split = split_command_line(&format!("{exe} /S _?=C:\\x"), |p| p == exe);
        assert_eq!(split, Some((exe.into(), "/S _?=C:\\x".into())));
    }

    #[test]
    fn splits_bare_msiexec() {
        let split = split_command_line("MsiExec.exe /X{1234}", |_| false);
        assert_eq!(split, Some(("MsiExec.exe".into(), "/X{1234}".into())));
    }

    #[test]
    fn rejects_empty_and_unterminated_quotes() {
        assert_eq!(split_command_line("   ", |_| true), None);
        assert_eq!(split_command_line("\"C:\\no-end.exe /S", |_| true), None);
    }

    #[test]
    fn process_tree_follows_children_of_exited_parents_only() {
        let mut tree = std::collections::HashSet::from([100]);
        // 100 (exited) -> 200 (_iu.tmp) -> 300; 400 is unrelated; order is arbitrary.
        let processes = [(300, Some(200)), (200, Some(100)), (400, Some(4)), (4, None)];
        grow_process_tree(&mut tree, &processes);
        assert!(tree.contains(&200) && tree.contains(&300));
        assert!(!tree.contains(&400) && !tree.contains(&4));
    }

    #[test]
    fn recognises_installer_helper_images() {
        assert!(is_uninstall_helper_image("_iu14D2N.tmp"));
        assert!(is_uninstall_helper_image("Un_A.exe"));
        assert!(is_uninstall_helper_image("unins000.exe"));
        assert!(!is_uninstall_helper_image("explorer.exe"));
    }
}
